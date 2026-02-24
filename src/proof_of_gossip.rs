use crate::args::BerachainArgs;
use alloy_consensus::{EthereumTxEnvelope, SignableTransaction, TxEip1559};
use alloy_primitives::{Address, Bytes, TxHash, U256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use eyre::Result;
use rand::Rng;
use rand::seq::SliceRandom;
use reth_metrics::{
    Metrics,
    metrics,
    metrics::{Counter, Gauge},
};
use reth::providers::{BlockReaderIdExt, StateProviderFactory};
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_api::{NetworkInfo, PeerInfo, Peers, ReputationChangeKind};
use reth_network_peers::PeerId;
use rusqlite::{Connection, params};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use tracing::{info, warn};

const CANARY_GAS_LIMIT: u64 = 21000;
const MAX_FEE_BUFFER_MULTIPLIER: u128 = 2;
const CANARY_PRIORITY_FEE_WEI: u128 = 1_000_000_000;
const MIN_CANARY_VALUE: u64 = 1;
const MAX_CANARY_VALUE: u64 = 1000;
const LOOP_TICK_INTERVAL_SECS: u64 = 10;
const LATE_CONFIRMATION_TRACK_WINDOW_SECS: u64 = 900;
const MIN_FUNDING_BACKOFF_SECS: u64 = 30;
const MAX_FUNDING_BACKOFF_SECS: u64 = 86400;

pub trait NetworkOps: Send + Sync {
    fn is_syncing(&self) -> bool;
    fn get_all_peers(&self) -> impl Future<Output = Result<Vec<PeerInfo>>> + Send;
    fn reputation_change(&self, peer_id: PeerId, kind: ReputationChangeKind);
    fn disconnect_peer(&self, peer: PeerId);
    fn send_canary(&self, peer_id: PeerId, tx: crate::transaction::BerachainTxEnvelope);
}

impl<N: NetworkPrimitives<BroadcastedTransaction = crate::transaction::BerachainTxEnvelope>>
    NetworkOps for NetworkHandle<N>
{
    fn is_syncing(&self) -> bool {
        NetworkInfo::is_syncing(self)
    }

    async fn get_all_peers(&self) -> Result<Vec<PeerInfo>> {
        Ok(Peers::get_all_peers(self).await?)
    }

    fn reputation_change(&self, peer_id: PeerId, kind: ReputationChangeKind) {
        Peers::reputation_change(self, peer_id, kind)
    }

    fn disconnect_peer(&self, peer: PeerId) {
        Peers::disconnect_peer(self, peer)
    }

    fn send_canary(&self, peer_id: PeerId, tx: crate::transaction::BerachainTxEnvelope) {
        NetworkHandle::send_transactions(self, peer_id, vec![Arc::new(tx)])
    }
}

pub trait PogProvider: Send + Sync {
    fn receipt_exists(&self, hash: TxHash) -> Result<bool>;
    fn account_nonce(&self, address: &Address) -> Result<Option<u64>>;
    fn account_balance(&self, address: &Address) -> Result<Option<U256>>;
    fn latest_base_fee(&self) -> Result<u128>;
}

impl<P> PogProvider for P
where
    P: StateProviderFactory
        + BlockReaderIdExt<Header = crate::primitives::BerachainHeader>
        + Send
        + Sync,
{
    fn receipt_exists(&self, hash: TxHash) -> Result<bool> {
        Ok(self.receipt_by_hash(hash)?.is_some())
    }

    fn account_nonce(&self, address: &Address) -> Result<Option<u64>> {
        Ok(self.latest()?.account_nonce(address)?)
    }

    fn account_balance(&self, address: &Address) -> Result<Option<U256>> {
        Ok(self.latest()?.account_balance(address)?)
    }

    fn latest_base_fee(&self) -> Result<u128> {
        let header = self
            .latest_header()?
            .ok_or_else(|| eyre::eyre!("Failed to fetch latest block header"))?
            .into_header();
        let base_fee = header
            .base_fee_per_gas
            .ok_or_else(|| eyre::eyre!("Latest block has no base fee - pre-EIP-1559 chain?"))?;
        Ok(base_fee as u128)
    }
}

struct ActiveCanary {
    tx_hash: TxHash,
    peer_id: PeerId,
    sent_at: Instant,
}

struct TimedOutCanary {
    peer_id: PeerId,
    timed_out_at: Instant,
}

#[derive(Metrics)]
#[metrics(scope = "bera_reth.pog")]
struct PoGMetrics {
    /// Number of canary transactions sent.
    canaries_sent_total: Counter,
    /// Number of canary transactions confirmed before timeout.
    canary_confirmed_total: Counter,
    /// Number of canary transactions that timed out.
    canary_timeout_total: Counter,
    /// Number of timed-out canaries that later confirmed.
    canary_late_confirmed_total: Counter,
    /// Number of reputation penalties applied.
    penalties_total: Counter,
    /// Number of peer bans/disconnect actions applied.
    bans_total: Counter,
    /// Number of currently active canaries.
    inflight_canaries: Gauge,
}

fn pog_metrics() -> &'static PoGMetrics {
    static METRICS: OnceLock<PoGMetrics> = OnceLock::new();
    METRICS.get_or_init(PoGMetrics::default)
}

pub struct ProofOfGossipService<Network, Provider> {
    network: Network,
    provider: Provider,
    signer: PrivateKeySigner,
    chain_id: u64,
    db: Connection,
    confirmed_peers: HashSet<PeerId>,
    failure_counts: HashMap<PeerId, u32>,
    reputation_penalty: i32,
    active: Option<ActiveCanary>,
    timed_out_canaries: HashMap<TxHash, TimedOutCanary>,
    nonce: u64,
    timeout: Duration,
    warned_syncing: bool,
    funding_backoff: Option<Instant>,
    funding_backoff_secs: u64,
}

impl<Network, Provider> ProofOfGossipService<Network, Provider>
where
    Network: NetworkOps + 'static,
    Provider: PogProvider + 'static,
{
    pub fn new(
        network: Network,
        provider: Provider,
        chain_id: u64,
        datadir: PathBuf,
        args: &BerachainArgs,
    ) -> Result<Option<Self>> {
        let Some(private_key_hex) = &args.pog_private_key else {
            return Ok(None);
        };

        let signer = private_key_hex.parse::<PrivateKeySigner>()?;
        let address = signer.address();

        let db_path = datadir.join("proof_of_gossip.db");
        let db = Connection::open(&db_path)?;

        db.execute(
            "CREATE TABLE IF NOT EXISTS peer_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                peer_id TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
            [],
        )?;

        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_peer_tests_peer_id ON peer_tests(peer_id)",
            [],
        )?;

        db.pragma_update(None, "journal_mode", "WAL")?;

        let confirmed_peers: HashSet<PeerId> = {
            let mut stmt = db.prepare(
                "SELECT DISTINCT peer_id FROM peer_tests WHERE result IN ('confirmed', 'late_confirmed')",
            )?;
            stmt.query_map([], |row| {
                let peer_id_str: String = row.get(0)?;
                peer_id_str.parse::<PeerId>().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })?
            .collect::<Result<_, _>>()?
        };

        let failure_counts: HashMap<PeerId, u32> = {
            let mut stmt = db.prepare(
                "SELECT peer_id, COUNT(*) FROM peer_tests WHERE result = 'timeout' GROUP BY peer_id",
            )?;
            stmt.query_map([], |row| {
                let peer_id_str: String = row.get(0)?;
                let count: u32 = row.get(1)?;
                Ok((
                    peer_id_str.parse::<PeerId>().map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
                    count,
                ))
            })?
            .collect::<Result<_, _>>()?
        };

        info!(
            target: "bera_reth::pog",
            address = %address,
            confirmed_peers = confirmed_peers.len(),
            failed_peers = failure_counts.len(),
            "Proof of Gossip service initialized"
        );
        pog_metrics().inflight_canaries.set(0.0);

        Ok(Some(Self {
            network,
            provider,
            signer,
            chain_id,
            db,
            confirmed_peers,
            failure_counts,
            reputation_penalty: -(args.pog_reputation_penalty.abs()),
            active: None,
            timed_out_canaries: HashMap::new(),
            nonce: 0,
            timeout: Duration::from_secs(args.pog_timeout),
            warned_syncing: false,
            funding_backoff: None,
            funding_backoff_secs: 0,
        }))
    }

    pub async fn run(mut self, mut shutdown: reth::tasks::shutdown::GracefulShutdown) {
        info!(target: "bera_reth::pog", "PoG service started");

        loop {
            tokio::select! {
                guard = &mut shutdown => {
                    info!(target: "bera_reth::pog", "PoG service shutting down");
                    pog_metrics().inflight_canaries.set(0.0);
                    drop(guard);
                    return;
                }
                _ = sleep(Duration::from_secs(LOOP_TICK_INTERVAL_SECS)) => {
                    if let Err(e) = self.tick().await {
                        warn!(target: "bera_reth::pog", error = %e, "Error in PoG service tick");
                    }
                }
            }
        }
    }

    async fn tick(&mut self) -> Result<()> {
        self.reconcile_late_confirmations()?;

        if self.network.is_syncing() {
            if !self.warned_syncing {
                info!(target: "bera_reth::pog", "PoG paused while node is syncing");
                self.warned_syncing = true;
            }

            if let Some(active) = self.active.as_mut() {
                active.sent_at = Instant::now();
            }

            return Ok(());
        }

        if self.warned_syncing {
            info!(target: "bera_reth::pog", "PoG resumed after sync");
            self.warned_syncing = false;
        }

        if let Some(deadline) = self.funding_backoff {
            if Instant::now() < deadline {
                return Ok(());
            }
            self.funding_backoff = None;
        }

        if let Some(canary) = &self.active {
            let tx_hash = canary.tx_hash;
            let peer_id = canary.peer_id;
            let elapsed = canary.sent_at.elapsed();

            if self.provider.receipt_exists(tx_hash)? {
                info!(
                    target: "bera_reth::pog",
                    peer_id = %peer_id,
                    tx_hash = %tx_hash,
                    "Canary transaction confirmed"
                );

                self.active = None;
                self.persist_result(&peer_id, tx_hash, "confirmed")?;
                self.confirmed_peers.insert(peer_id);
                pog_metrics().canary_confirmed_total.increment(1);
                pog_metrics().inflight_canaries.set(0.0);
                self.refresh_nonce()?;
            } else if elapsed > self.timeout {
                warn!(
                    target: "bera_reth::pog",
                    peer_id = %peer_id,
                    tx_hash = %tx_hash,
                    elapsed_secs = elapsed.as_secs(),
                    "Canary transaction timed out"
                );

                self.active = None;
                self.persist_result(&peer_id, tx_hash, "timeout")?;
                pog_metrics().canary_timeout_total.increment(1);
                pog_metrics().inflight_canaries.set(0.0);

                let failure_count =
                    self.failure_counts.entry(peer_id).and_modify(|c| *c += 1).or_insert(1);
                let failure_count = *failure_count;

                self.network
                    .reputation_change(peer_id, ReputationChangeKind::Other(self.reputation_penalty));
                self.network.disconnect_peer(peer_id);
                pog_metrics().penalties_total.increment(1);
                pog_metrics().bans_total.increment(1);

                self.timed_out_canaries
                    .insert(tx_hash, TimedOutCanary { peer_id, timed_out_at: Instant::now() });

                self.refresh_nonce()?;

                info!(
                    target: "bera_reth::pog",
                    nonce = self.nonce,
                    failure_count = failure_count,
                    "Re-queried on-chain nonce after timeout"
                );
            }
        } else if self.check_funding()? {
            let all_peers = self.network.get_all_peers().await?;

            let eligible: Vec<_> =
                all_peers.iter().filter(|p| !self.confirmed_peers.contains(&p.remote_id)).collect();

            if let Some(peer) = eligible.choose(&mut rand::thread_rng()) {
                let peer_id = peer.remote_id;
                self.refresh_nonce()?;
                let base_fee = self.provider.latest_base_fee()?;
                let canary_tx =
                    create_canary_tx(&self.signer, self.nonce, self.chain_id, base_fee)?;
                let tx_hash = *canary_tx.hash();

                self.network.send_canary(peer_id, canary_tx);
                pog_metrics().canaries_sent_total.increment(1);
                pog_metrics().inflight_canaries.set(1.0);

                info!(
                    target: "bera_reth::pog",
                    peer_id = %peer_id,
                    tx_hash = %tx_hash,
                    nonce = self.nonce,
                    "Sent canary transaction to peer"
                );

                self.active = Some(ActiveCanary { tx_hash, peer_id, sent_at: Instant::now() });
            }
        }

        Ok(())
    }

    fn check_funding(&mut self) -> Result<bool> {
        let address = self.signer.address();
        let balance = self.provider.account_balance(&address)?;
        let base_fee = self.provider.latest_base_fee().unwrap_or(CANARY_PRIORITY_FEE_WEI);
        let max_fee =
            (base_fee * MAX_FEE_BUFFER_MULTIPLIER).max(CANARY_PRIORITY_FEE_WEI + 1);
        let min_balance =
            U256::from(CANARY_GAS_LIMIT) * U256::from(max_fee) + U256::from(MAX_CANARY_VALUE);

        match balance {
            Some(b) if b >= min_balance => {
                self.funding_backoff_secs = 0;
                Ok(true)
            }
            _ => {
                self.funding_backoff_secs = if self.funding_backoff_secs == 0 {
                    MIN_FUNDING_BACKOFF_SECS
                } else {
                    (self.funding_backoff_secs * 2).min(MAX_FUNDING_BACKOFF_SECS)
                };
                self.funding_backoff = Some(Instant::now() + Duration::from_secs(self.funding_backoff_secs));

                warn!(
                    target: "bera_reth::pog",
                    address = %address,
                    balance = ?balance,
                    backoff_secs = self.funding_backoff_secs,
                    "PoG wallet underfunded, backing off"
                );
                Ok(false)
            }
        }
    }

    fn refresh_nonce(&mut self) -> Result<()> {
        let address = self.signer.address();
        self.nonce = self
            .provider
            .account_nonce(&address)?
            .ok_or_else(|| eyre::eyre!("PoG wallet {address} not found in state - is it funded?"))?;
        Ok(())
    }

    fn reconcile_late_confirmations(&mut self) -> Result<()> {
        if self.timed_out_canaries.is_empty() {
            return Ok(());
        }

        let mut confirmed_late = Vec::new();
        for (&tx_hash, timed_out) in &self.timed_out_canaries {
            if self.provider.receipt_exists(tx_hash)? {
                confirmed_late.push((tx_hash, timed_out.peer_id));
            }
        }

        for (tx_hash, peer_id) in confirmed_late {
            self.persist_result(&peer_id, tx_hash, "late_confirmed")?;
            self.confirmed_peers.insert(peer_id);
            self.timed_out_canaries.remove(&tx_hash);
            pog_metrics().canary_late_confirmed_total.increment(1);
            info!(
                target: "bera_reth::pog",
                peer_id = %peer_id,
                tx_hash = %tx_hash,
                "Timed-out canary confirmed later; marked peer as confirmed"
            );
        }

        let window = Duration::from_secs(LATE_CONFIRMATION_TRACK_WINDOW_SECS);
        self.timed_out_canaries.retain(|_, timed_out| timed_out.timed_out_at.elapsed() <= window);

        Ok(())
    }

    fn persist_result(&mut self, peer_id: &PeerId, tx_hash: TxHash, result: &str) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        self.db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), result, timestamp],
        )?;

        Ok(())
    }
}

pub fn new_pog_service<Network, Provider>(
    network: Network,
    provider: Provider,
    chain_id: u64,
    datadir: PathBuf,
    args: &BerachainArgs,
) -> Result<Option<ProofOfGossipService<Network, Provider>>>
where
    Network: NetworkOps + 'static,
    Provider: PogProvider + 'static,
{
    ProofOfGossipService::new(network, provider, chain_id, datadir, args)
}

pub fn create_canary_tx(
    signer: &PrivateKeySigner,
    nonce: u64,
    chain_id: u64,
    base_fee: u128,
) -> Result<crate::transaction::BerachainTxEnvelope> {
    let to = signer.address();
    let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);
    let max_priority_fee_per_gas = CANARY_PRIORITY_FEE_WEI;
    let max_fee_per_gas = (base_fee * MAX_FEE_BUFFER_MULTIPLIER).max(max_priority_fee_per_gas + 1);

    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit: CANARY_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: alloy_primitives::TxKind::Call(to),
        value: U256::from(value),
        access_list: Default::default(),
        input: Bytes::default(),
    };

    let signature = signer.sign_hash_sync(&tx.signature_hash())?;
    let signed = tx.into_signed(signature);
    let eth_envelope = EthereumTxEnvelope::Eip1559(signed);

    Ok(crate::transaction::BerachainTxEnvelope::Ethereum(eth_envelope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::Transaction;
    use alloy_primitives::B256;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    const ONE_BERA: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);

    #[test]
    fn test_canary_tx_construction() {
        let private_key = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let signer: PrivateKeySigner = private_key.parse().unwrap();
        let nonce = 42;
        let chain_id = 80094;
        let base_fee = 1_000_000_000;

        let tx = create_canary_tx(&signer, nonce, chain_id, base_fee).unwrap();

        let eth_envelope = match &tx {
            crate::transaction::BerachainTxEnvelope::Ethereum(eth) => eth,
            _ => panic!("Expected Ethereum transaction"),
        };

        let inner = match eth_envelope {
            EthereumTxEnvelope::Eip1559(signed) => signed,
            _ => panic!("Expected EIP-1559 transaction"),
        };

        assert_eq!(inner.to(), Some(signer.address()));
        assert!(inner.value() >= U256::from(MIN_CANARY_VALUE));
        assert!(inner.value() <= U256::from(MAX_CANARY_VALUE));
        assert_eq!(inner.gas_limit(), CANARY_GAS_LIMIT);
        assert_eq!(inner.nonce(), nonce);
        assert_eq!(inner.chain_id(), Some(chain_id));

        let recovered = inner.recover_signer().unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn test_sqlite_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = create_test_db(temp_file.path());

        let peer_id = PeerId::random();
        let tx_hash = B256::random();
        let timestamp = 1234567890i64;

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), "confirmed", timestamp],
        )
        .unwrap();

        let mut stmt =
            db.prepare("SELECT peer_id, tx_hash, result, tested_at FROM peer_tests").unwrap();
        let mut rows = stmt.query([]).unwrap();

        let row = rows.next().unwrap().unwrap();
        let loaded_peer_id: String = row.get(0).unwrap();
        let loaded_tx_hash: String = row.get(1).unwrap();
        let loaded_result: String = row.get(2).unwrap();
        let loaded_timestamp: i64 = row.get(3).unwrap();

        assert_eq!(loaded_peer_id, peer_id.to_string());
        assert_eq!(loaded_tx_hash, tx_hash.to_string());
        assert_eq!(loaded_result, "confirmed");
        assert_eq!(loaded_timestamp, timestamp);
    }

    #[test]
    fn test_sqlite_reload() {
        let temp_file = NamedTempFile::new().unwrap();

        {
            let db = create_test_db(temp_file.path());
            db.execute(
                "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
                params![PeerId::random().to_string(), B256::random().to_string(), "timeout", 9999999],
            )
            .unwrap();
        }

        let db = Connection::open(temp_file.path()).unwrap();
        let mut stmt = db.prepare("SELECT peer_id FROM peer_tests").unwrap();
        let count = stmt.query_map([], |_| Ok(())).unwrap().count();

        assert_eq!(count, 1);
    }

    fn create_test_db(path: &std::path::Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute(
            "CREATE TABLE IF NOT EXISTS peer_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                peer_id TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        db
    }

    #[test]
    fn test_confirmed_excludes_from_eligible() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = create_test_db(temp_file.path());

        let confirmed = PeerId::random();
        let timed_out = PeerId::random();

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![confirmed.to_string(), B256::random().to_string(), "confirmed", 1000],
        )
        .unwrap();
        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![timed_out.to_string(), B256::random().to_string(), "timeout", 2000],
        )
        .unwrap();

        let confirmed_peers: HashSet<PeerId> = {
            let mut stmt = db
                .prepare("SELECT DISTINCT peer_id FROM peer_tests WHERE result IN ('confirmed', 'late_confirmed')")
                .unwrap();
            stmt.query_map([], |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<PeerId>().unwrap())
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };

        assert!(confirmed_peers.contains(&confirmed));
        assert!(!confirmed_peers.contains(&timed_out));
    }

    #[test]
    fn test_failure_count_reload() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = create_test_db(temp_file.path());

        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let peer_c = PeerId::random();

        for _ in 0..3 {
            db.execute(
                "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
                params![peer_a.to_string(), B256::random().to_string(), "timeout", 1000],
            )
            .unwrap();
        }

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_b.to_string(), B256::random().to_string(), "timeout", 2000],
        )
        .unwrap();
        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_b.to_string(), B256::random().to_string(), "confirmed", 3000],
        )
        .unwrap();

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_c.to_string(), B256::random().to_string(), "confirmed", 4000],
        )
        .unwrap();

        let failure_counts: HashMap<PeerId, u32> = {
            let mut stmt = db
                .prepare(
                    "SELECT peer_id, COUNT(*) FROM peer_tests WHERE result = 'timeout' GROUP BY peer_id",
                )
                .unwrap();
            stmt.query_map([], |row| {
                let s: String = row.get(0)?;
                let count: u32 = row.get(1)?;
                Ok((s.parse::<PeerId>().unwrap(), count))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };

        let confirmed_peers: HashSet<PeerId> = {
            let mut stmt = db
                .prepare("SELECT DISTINCT peer_id FROM peer_tests WHERE result IN ('confirmed', 'late_confirmed')")
                .unwrap();
            stmt.query_map([], |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<PeerId>().unwrap())
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };

        assert_eq!(failure_counts.get(&peer_a), Some(&3));
        assert_eq!(failure_counts.get(&peer_b), Some(&1));
        assert_eq!(failure_counts.get(&peer_c), None);

        assert!(!confirmed_peers.contains(&peer_a));
        assert!(confirmed_peers.contains(&peer_b));
        assert!(confirmed_peers.contains(&peer_c));
    }

    #[test]
    fn test_sqlite_multiple_results_per_peer() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = create_test_db(temp_file.path());

        let peer_id = PeerId::random();
        let tx_hash1 = B256::random();
        let tx_hash2 = B256::random();

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash1.to_string(), "timeout", 1111111],
        )
        .unwrap();

        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash2.to_string(), "confirmed", 2222222],
        )
        .unwrap();

        let mut stmt = db.prepare("SELECT COUNT(*) FROM peer_tests WHERE peer_id = ?1").unwrap();
        let count: i64 = stmt.query_row(params![peer_id.to_string()], |row| row.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    // --- Mock types for tick() testing ---

    #[derive(Default)]
    struct MockProviderState {
        receipts: HashSet<TxHash>,
        nonce: Option<u64>,
        balance: Option<U256>,
        base_fee: u128,
    }

    struct MockProvider {
        state: Mutex<MockProviderState>,
    }

    impl MockProvider {
        fn new(nonce: u64, balance: U256, base_fee: u128) -> Self {
            Self {
                state: Mutex::new(MockProviderState {
                    receipts: HashSet::new(),
                    nonce: Some(nonce),
                    balance: Some(balance),
                    base_fee,
                }),
            }
        }

        fn add_receipt(&self, hash: TxHash) {
            self.state.lock().unwrap().receipts.insert(hash);
        }

        fn set_balance(&self, balance: U256) {
            self.state.lock().unwrap().balance = Some(balance);
        }
    }

    impl PogProvider for MockProvider {
        fn receipt_exists(&self, hash: TxHash) -> Result<bool> {
            Ok(self.state.lock().unwrap().receipts.contains(&hash))
        }

        fn account_nonce(&self, _address: &Address) -> Result<Option<u64>> {
            Ok(self.state.lock().unwrap().nonce)
        }

        fn account_balance(&self, _address: &Address) -> Result<Option<U256>> {
            Ok(self.state.lock().unwrap().balance)
        }

        fn latest_base_fee(&self) -> Result<u128> {
            Ok(self.state.lock().unwrap().base_fee)
        }
    }

    struct MockNetworkState {
        syncing: bool,
        peers: Vec<PeerInfo>,
        sent_canaries: Vec<(PeerId, TxHash)>,
        reputation_changes: Vec<(PeerId, i32)>,
        disconnected: Vec<PeerId>,
    }

    struct MockNetwork {
        state: Mutex<MockNetworkState>,
    }

    impl MockNetwork {
        fn new(peer_ids: Vec<PeerId>) -> Self {
            let peers = peer_ids.into_iter().map(make_peer_info).collect();

            Self {
                state: Mutex::new(MockNetworkState {
                    syncing: false,
                    peers,
                    sent_canaries: Vec::new(),
                    reputation_changes: Vec::new(),
                    disconnected: Vec::new(),
                }),
            }
        }

        fn set_syncing(&self, syncing: bool) {
            self.state.lock().unwrap().syncing = syncing;
        }

        fn sent_canaries(&self) -> Vec<(PeerId, TxHash)> {
            self.state.lock().unwrap().sent_canaries.clone()
        }

        fn reputation_changes(&self) -> Vec<(PeerId, i32)> {
            self.state.lock().unwrap().reputation_changes.clone()
        }

        fn disconnected_peers(&self) -> Vec<PeerId> {
            self.state.lock().unwrap().disconnected.clone()
        }
    }

    impl NetworkOps for MockNetwork {
        fn is_syncing(&self) -> bool {
            self.state.lock().unwrap().syncing
        }

        async fn get_all_peers(&self) -> Result<Vec<PeerInfo>> {
            Ok(self.state.lock().unwrap().peers.clone())
        }

        fn reputation_change(&self, peer_id: PeerId, kind: ReputationChangeKind) {
            if let ReputationChangeKind::Other(val) = kind {
                self.state.lock().unwrap().reputation_changes.push((peer_id, val));
            }
        }

        fn disconnect_peer(&self, peer: PeerId) {
            self.state.lock().unwrap().disconnected.push(peer);
        }

        fn send_canary(&self, peer_id: PeerId, tx: crate::transaction::BerachainTxEnvelope) {
            let tx_hash = *tx.hash();
            self.state.lock().unwrap().sent_canaries.push((peer_id, tx_hash));
        }
    }

    fn make_peer_info(id: PeerId) -> PeerInfo {
        use reth_eth_wire_types::{
            Capability, EthVersion, UnifiedStatus,
            capability::Capabilities,
        };
        PeerInfo {
            capabilities: Arc::new(Capabilities::from(vec![Capability::eth(EthVersion::Eth68)])),
            remote_id: id,
            client_version: Arc::from("test/1.0"),
            enode: String::new(),
            enr: None,
            remote_addr: "127.0.0.1:30303".parse().unwrap(),
            local_addr: None,
            direction: reth_network_api::Direction::Incoming,
            eth_version: EthVersion::Eth68,
            status: Arc::new(UnifiedStatus::default()),
            session_established: Instant::now(),
            kind: reth_network_api::PeerKind::Basic,
        }
    }

    fn make_service(
        network: MockNetwork,
        provider: MockProvider,
        db_path: &std::path::Path,
    ) -> ProofOfGossipService<MockNetwork, MockProvider> {
        let db = create_test_db(db_path);
        let signer: PrivateKeySigner =
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .parse()
                .unwrap();

        ProofOfGossipService {
            network,
            provider,
            signer,
            chain_id: 80094,
            db,
            confirmed_peers: HashSet::new(),
            failure_counts: HashMap::new(),
            reputation_penalty: -25600,
            active: None,
            timed_out_canaries: HashMap::new(),
            nonce: 0,
            timeout: Duration::from_secs(120),
            warned_syncing: false,
            funding_backoff: None,
            funding_backoff_secs: 0,
        }
    }

    #[tokio::test]
    async fn test_tick_skips_when_syncing() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        network.set_syncing(true);

        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.tick().await.unwrap();

        assert!(service.network.sent_canaries().is_empty());
        assert!(service.warned_syncing);
    }

    #[tokio::test]
    async fn test_tick_sends_canary_to_untested_peer() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.tick().await.unwrap();

        let canaries = service.network.sent_canaries();
        assert_eq!(canaries.len(), 1);
        assert_eq!(canaries[0].0, peer);
        assert!(service.active.is_some());
    }

    #[tokio::test]
    async fn test_tick_confirms_canary() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.tick().await.unwrap();
        let tx_hash = service.active.as_ref().unwrap().tx_hash;

        service.provider.add_receipt(tx_hash);
        service.tick().await.unwrap();

        assert!(service.active.is_none());
        assert!(service.confirmed_peers.contains(&peer));
    }

    #[tokio::test]
    async fn test_tick_times_out_and_penalizes() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());
        service.timeout = Duration::from_millis(0);

        service.tick().await.unwrap();
        assert!(service.active.is_some());

        sleep(Duration::from_millis(10)).await;
        service.tick().await.unwrap();

        assert!(service.active.is_none());
        assert!(!service.confirmed_peers.contains(&peer));

        let penalties = service.network.reputation_changes();
        assert_eq!(penalties.len(), 1);
        assert_eq!(penalties[0].0, peer);
        assert_eq!(penalties[0].1, -25600);

        let disconnected = service.network.disconnected_peers();
        assert_eq!(disconnected, vec![peer]);
    }

    #[tokio::test]
    async fn test_tick_late_confirmation_reconciles() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());
        service.timeout = Duration::from_millis(0);

        service.tick().await.unwrap();
        let tx_hash = service.active.as_ref().unwrap().tx_hash;

        sleep(Duration::from_millis(10)).await;
        service.tick().await.unwrap();
        assert!(service.timed_out_canaries.contains_key(&tx_hash));
        assert!(!service.confirmed_peers.contains(&peer));

        service.provider.add_receipt(tx_hash);
        service.tick().await.unwrap();

        assert!(!service.timed_out_canaries.contains_key(&tx_hash));
        assert!(service.confirmed_peers.contains(&peer));
    }

    #[tokio::test]
    async fn test_tick_skips_confirmed_peers() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, ONE_BERA, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.confirmed_peers.insert(peer);
        service.tick().await.unwrap();

        assert!(service.network.sent_canaries().is_empty());
    }

    #[tokio::test]
    async fn test_tick_backs_off_when_underfunded() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, U256::ZERO, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.tick().await.unwrap();

        assert!(service.network.sent_canaries().is_empty());
        assert!(service.funding_backoff.is_some());
        assert_eq!(service.funding_backoff_secs, MIN_FUNDING_BACKOFF_SECS);

        service.funding_backoff = Some(Instant::now() - Duration::from_secs(1));
        service.tick().await.unwrap();

        assert_eq!(service.funding_backoff_secs, MIN_FUNDING_BACKOFF_SECS * 2);
    }

    #[tokio::test]
    async fn test_funding_backoff_resets_on_fund() {
        let temp_file = NamedTempFile::new().unwrap();
        let peer = PeerId::random();
        let network = MockNetwork::new(vec![peer]);
        let provider = MockProvider::new(0, U256::ZERO, 1_000_000_000);
        let mut service = make_service(network, provider, temp_file.path());

        service.tick().await.unwrap();
        assert_eq!(service.funding_backoff_secs, MIN_FUNDING_BACKOFF_SECS);

        service.provider.set_balance(ONE_BERA);
        service.funding_backoff = Some(Instant::now() - Duration::from_secs(1));
        service.tick().await.unwrap();

        assert_eq!(service.funding_backoff_secs, 0);
        assert!(!service.network.sent_canaries().is_empty());
    }
}
