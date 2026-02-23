use crate::args::BerachainArgs;
use alloy_consensus::{EthereumTxEnvelope, SignableTransaction, TxEip1559};
use alloy_primitives::{Bytes, TxHash, U256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use eyre::Result;
use rand::Rng;
use rand::seq::SliceRandom;
use reth::providers::{BlockReaderIdExt, StateProviderFactory};
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_api::{NetworkInfo, Peers, ReputationChangeKind};
use reth_network_peers::PeerId;
use rusqlite::{Connection, params};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use tracing::warn;

const CANARY_GAS_LIMIT: u64 = 21000;
const MAX_FEE_BUFFER_MULTIPLIER: u128 = 2;
const MIN_CANARY_VALUE: u64 = 1;
const MAX_CANARY_VALUE: u64 = 1000;
const LOOP_TICK_INTERVAL_SECS: u64 = 10;
const LATE_CONFIRMATION_TRACK_WINDOW_SECS: u64 = 900;
const STARTUP_DELAY_SECS: u64 = 60;

pub trait NetworkOps: Peers + NetworkInfo {
    type Primitives: NetworkPrimitives;

    fn send_transactions(
        &self,
        peer_id: PeerId,
        msg: Vec<Arc<<Self::Primitives as NetworkPrimitives>::BroadcastedTransaction>>,
    );
}

impl<N: NetworkPrimitives> NetworkOps for NetworkHandle<N> {
    type Primitives = N;

    fn send_transactions(
        &self,
        peer_id: PeerId,
        msg: Vec<Arc<<Self::Primitives as NetworkPrimitives>::BroadcastedTransaction>>,
    ) {
        NetworkHandle::send_transactions(self, peer_id, msg)
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

pub struct ProofOfGossipService<Network, P> {
    network: Network,
    provider: P,
    signer: PrivateKeySigner,
    chain_id: u64,
    db: Arc<Mutex<Connection>>,
    confirmed_peers: HashSet<PeerId>,
    failure_counts: HashMap<PeerId, u32>,
    reputation_penalty: i32,
    active: Option<ActiveCanary>,
    timed_out_canaries: HashMap<TxHash, TimedOutCanary>,
    nonce: u64,
    timeout: Duration,
    warned_syncing: bool,
    started_at: Instant,
}

impl<Network, P> ProofOfGossipService<Network, P>
where
    Network: NetworkOps<
            Primitives: NetworkPrimitives<
                BroadcastedTransaction = crate::transaction::BerachainTxEnvelope,
            >,
        > + Clone
        + Send
        + Sync
        + 'static,
    P: StateProviderFactory
        + BlockReaderIdExt<Header = crate::primitives::BerachainHeader>
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub async fn new_with_provider(
        network: Network,
        provider: P,
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

        db.execute("CREATE INDEX IF NOT EXISTS idx_peer_tests_peer_id ON peer_tests(peer_id)", [])?;

        db.pragma_update(None, "journal_mode", "WAL")?;

        let confirmed_peers: HashSet<PeerId> = {
            let mut stmt = db
                .prepare("SELECT DISTINCT peer_id FROM peer_tests WHERE result IN ('confirmed', 'late_confirmed')")?;
            stmt.query_map([], |row| {
                let peer_id_str: String = row.get(0)?;
                Ok(peer_id_str.parse::<PeerId>().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?)
            })?
            .collect::<Result<_, _>>()?
        };

        let failure_counts: HashMap<PeerId, u32> = {
            let mut stmt = db.prepare("SELECT peer_id, COUNT(*) FROM peer_tests WHERE result = 'timeout' GROUP BY peer_id")?;
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

        let db = Arc::new(Mutex::new(db));

        warn!(
            target: "bera_reth::pog",
            address = %address,
            confirmed_peers = confirmed_peers.len(),
            failed_peers = failure_counts.len(),
            "Proof of Gossip service initialized"
        );

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
            started_at: Instant::now(),
        }))
    }

    pub async fn run(mut self) {
        warn!(target: "bera_reth::pog", "Starting Proof of Gossip service loop");

        loop {
            if let Err(e) = self.tick().await {
                warn!(target: "bera_reth::pog", error = %e, "Error in PoG service tick");
            }

            sleep(Duration::from_secs(LOOP_TICK_INTERVAL_SECS)).await;
        }
    }

    async fn tick(&mut self) -> Result<()> {
        if self.started_at.elapsed() < Duration::from_secs(STARTUP_DELAY_SECS) {
            return Ok(());
        }

        self.reconcile_late_confirmations()?;

        if self.network.is_syncing() {
            if !self.warned_syncing {
                warn!(target: "bera_reth::pog", "PoG paused while node is syncing");
                self.warned_syncing = true;
            }

            if let Some(active) = self.active.as_mut() {
                active.sent_at = Instant::now();
            }

            return Ok(());
        }

        if self.warned_syncing {
            warn!(target: "bera_reth::pog", "PoG resumed after sync");
            self.warned_syncing = false;
        }

        if let Some(canary) = &self.active {
            if let Some(_receipt) = self.provider.receipt_by_hash(canary.tx_hash)? {
                warn!(
                    target: "bera_reth::pog",
                    peer_id = %canary.peer_id,
                    tx_hash = %canary.tx_hash,
                    "Canary transaction confirmed"
                );

                self.persist_result(&canary.peer_id, canary.tx_hash, "confirmed")?;
                self.confirmed_peers.insert(canary.peer_id);
                self.active = None;
                self.refresh_nonce()?;
            } else if canary.sent_at.elapsed() > self.timeout {
                warn!(
                    target: "bera_reth::pog",
                    peer_id = %canary.peer_id,
                    tx_hash = %canary.tx_hash,
                    elapsed_secs = canary.sent_at.elapsed().as_secs(),
                    "Canary transaction timed out"
                );

                self.persist_result(&canary.peer_id, canary.tx_hash, "timeout")?;

                let failure_count =
                    self.failure_counts.entry(canary.peer_id).and_modify(|c| *c += 1).or_insert(1);
                let failure_count = *failure_count;

                self.network.reputation_change(
                    canary.peer_id,
                    ReputationChangeKind::Other(self.reputation_penalty),
                );
                self.network.disconnect_peer(canary.peer_id);

                self.timed_out_canaries.insert(
                    canary.tx_hash,
                    TimedOutCanary { peer_id: canary.peer_id, timed_out_at: Instant::now() },
                );
                self.active = None;

                self.refresh_nonce()?;

                warn!(
                    target: "bera_reth::pog",
                    nonce = self.nonce,
                    failure_count = failure_count,
                    "Re-queried on-chain nonce after timeout"
                );
            }
        } else {
            let all_peers = self.network.get_all_peers().await?;

            let eligible: Vec<_> =
                all_peers.iter().filter(|p| !self.confirmed_peers.contains(&p.remote_id)).collect();

            let chosen_peer = eligible.choose(&mut rand::thread_rng()).map(|p| p.remote_id);

            if let Some(peer_id) = chosen_peer {
                self.refresh_nonce()?;
                let canary_tx = self.create_canary_tx().await?;
                let tx_hash = *canary_tx.hash();

                self.network.send_transactions(peer_id, vec![Arc::new(canary_tx)]);

                warn!(
                    target: "bera_reth::pog",
                    peer_id = %peer_id,
                    tx_hash = %tx_hash,
                    nonce = self.nonce,
                    "Sent canary transaction to peer"
                );

                self.active = Some(ActiveCanary { tx_hash, peer_id, sent_at: Instant::now() });
            } else {
                warn!(
                    target: "bera_reth::pog",
                    connected_peers = all_peers.len(),
                    confirmed_peers = self.confirmed_peers.len(),
                    "No untested peers available"
                );
            }
        }

        Ok(())
    }

    fn refresh_nonce(&mut self) -> Result<()> {
        let address = self.signer.address();
        self.nonce = self
            .provider
            .latest()?
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
            if self.provider.receipt_by_hash(tx_hash)?.is_some() {
                confirmed_late.push((tx_hash, timed_out.peer_id));
            }
        }

        for (tx_hash, peer_id) in confirmed_late {
            self.persist_result(&peer_id, tx_hash, "late_confirmed")?;
            self.confirmed_peers.insert(peer_id);
            self.timed_out_canaries.remove(&tx_hash);
            warn!(
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

    async fn create_canary_tx(&self) -> Result<crate::transaction::BerachainTxEnvelope> {
        let to = self.signer.address();
        let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);

        let latest_block = self
            .provider
            .latest_header()?
            .ok_or_else(|| eyre::eyre!("Failed to fetch latest block header"))?
            .into_header();

        let base_fee = latest_block
            .base_fee_per_gas
            .ok_or_else(|| eyre::eyre!("Latest block has no base fee - pre-EIP-1559 chain?"))?;
        let max_fee_per_gas = (base_fee as u128) * MAX_FEE_BUFFER_MULTIPLIER;

        let tx = TxEip1559 {
            chain_id: self.chain_id,
            nonce: self.nonce,
            gas_limit: CANARY_GAS_LIMIT,
            max_fee_per_gas,
            max_priority_fee_per_gas: 1_000_000_000,
            to: alloy_primitives::TxKind::Call(to),
            value: U256::from(value),
            access_list: Default::default(),
            input: Bytes::default(),
        };

        let signature = self.signer.sign_hash_sync(&tx.signature_hash())?;
        let signed = tx.into_signed(signature);
        let eth_envelope = EthereumTxEnvelope::Eip1559(signed);

        Ok(crate::transaction::BerachainTxEnvelope::Ethereum(eth_envelope))
    }

    fn persist_result(&self, peer_id: &PeerId, tx_hash: TxHash, result: &str) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let db = self.db.lock().map_err(|e| eyre::eyre!("PoG database lock poisoned: {e}"))?;
        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), result, timestamp],
        )?;

        Ok(())
    }
}

pub async fn new_pog_service<Network, P>(
    network: Network,
    provider: P,
    chain_id: u64,
    datadir: PathBuf,
    args: &BerachainArgs,
) -> Result<Option<ProofOfGossipService<Network, P>>>
where
    Network: NetworkOps<
            Primitives: NetworkPrimitives<
                BroadcastedTransaction = crate::transaction::BerachainTxEnvelope,
            >,
        > + Clone
        + Send
        + Sync
        + 'static,
    P: StateProviderFactory
        + BlockReaderIdExt<Header = crate::primitives::BerachainHeader>
        + Clone
        + Send
        + Sync
        + 'static,
{
    ProofOfGossipService::new_with_provider(network, provider, chain_id, datadir, args).await
}

pub fn create_canary_tx(
    signer: &PrivateKeySigner,
    nonce: u64,
    chain_id: u64,
    base_fee: u128,
) -> Result<crate::transaction::BerachainTxEnvelope> {
    let to = signer.address();
    let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);
    let max_fee_per_gas = base_fee * MAX_FEE_BUFFER_MULTIPLIER;

    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit: CANARY_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas: 1_000_000_000,
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
    use tempfile::NamedTempFile;

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
                .prepare("SELECT DISTINCT peer_id FROM peer_tests WHERE result = 'confirmed'")
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
            ).unwrap();
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
            let mut stmt = db.prepare("SELECT peer_id, COUNT(*) FROM peer_tests WHERE result = 'timeout' GROUP BY peer_id").unwrap();
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
                .prepare("SELECT DISTINCT peer_id FROM peer_tests WHERE result = 'confirmed'")
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
}
