//! Proof-of-Gossip node-side state: SQLite, unsigned canary construction, watcher task.
//!
//! Autonomous signing/ticking was removed; sentinel drives prepare/sign/submit.

use std::sync::atomic::{AtomicBool, Ordering};

static POG_CLI_ENABLED: AtomicBool = AtomicBool::new(false);

/// Set from `main` after parsing CLI. When false (default), PoG RPC modules and watcher are off.
pub fn set_pog_cli_enabled(enabled: bool) {
    POG_CLI_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn pog_cli_enabled() -> bool {
    POG_CLI_ENABLED.load(Ordering::SeqCst)
}

use crate::primitives::BerachainHeader;
use alloy_consensus::{SignableTransaction, TxEip1559};
use alloy_primitives::{Address, Bytes, TxHash, TxKind, U256, hex};
use rand::Rng;
use reth::providers::{BlockReaderIdExt, StateProviderFactory};
use reth_network_peers::PeerId;
use rusqlite::{Connection, params};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::info;

pub const CANARY_GAS_LIMIT: u64 = 21000;
pub const MAX_FEE_BUFFER_MULTIPLIER: u128 = 2;
pub const CANARY_PRIORITY_FEE_WEI: u128 = 1_000_000_000;
pub const MIN_CANARY_VALUE: u64 = 1;
pub const MAX_CANARY_VALUE: u64 = 1000;
pub const DEFAULT_POG_TIMEOUT_SECS: u64 = 25;
pub const WATCHER_TICK_SECS: u64 = 2;
pub const MIN_FUNDING_BACKOFF_SECS: u64 = 30;
pub const MAX_FUNDING_BACKOFF_SECS: u64 = 86400;
pub const LATE_CONFIRMATION_TRACK_WINDOW_SECS: u64 = 900;

pub(crate) fn ensure_peer_tests_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS peer_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                peer_id TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_peer_tests_peer_id ON peer_tests(peer_id)", [])?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PogPeerStatus {
    pub last_result: String,
    pub failure_count: u32,
    pub last_tested_at: u64,
}

/// Persistent SQLite connection to the PoG peer_tests database.
/// Opened once, reused across RPC calls.
pub struct PogDb {
    conn: Mutex<Connection>,
}

impl PogDb {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        ensure_peer_tests_schema(&conn)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn insert_peer_test(
        &self,
        peer_id: &PeerId,
        tx_hash: TxHash,
        result: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        conn.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), tx_hash.to_string(), result, ts],
        )?;
        Ok(())
    }

    /// Load PoG status for all peers in a single query. Returns a map keyed by peer_id string.
    pub fn all_peer_statuses(&self) -> rusqlite::Result<HashMap<String, PogPeerStatus>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT peer_id,
                    (SELECT result FROM peer_tests p2 WHERE p2.peer_id = p1.peer_id ORDER BY tested_at DESC LIMIT 1) AS last_result,
                    (SELECT tested_at FROM peer_tests p2 WHERE p2.peer_id = p1.peer_id ORDER BY tested_at DESC LIMIT 1) AS last_tested_at,
                    SUM(CASE WHEN result = 'timeout' THEN 1 ELSE 0 END) AS failure_count
             FROM peer_tests p1
             GROUP BY peer_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let peer_id: String = row.get(0)?;
            let last_result: String = row.get(1)?;
            let last_tested_at: i64 = row.get(2)?;
            let failure_count: u32 = row.get(3)?;
            Ok((
                peer_id,
                PogPeerStatus { last_result, failure_count, last_tested_at: last_tested_at as u64 },
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

pub fn build_unsigned_canary(
    to: Address,
    nonce: u64,
    chain_id: u64,
    base_fee: u128,
) -> (TxEip1559, u64) {
    let value = rand::thread_rng().gen_range(MIN_CANARY_VALUE..=MAX_CANARY_VALUE);
    let max_priority_fee_per_gas = CANARY_PRIORITY_FEE_WEI;
    let max_fee_per_gas = (base_fee * MAX_FEE_BUFFER_MULTIPLIER).max(max_priority_fee_per_gas + 1);
    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit: CANARY_GAS_LIMIT,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        to: TxKind::Call(to),
        value: U256::from(value),
        access_list: Default::default(),
        input: Bytes::default(),
    };
    (tx, value)
}

/// Hex `0x` + signing preimage (type byte 0x02 + RLP fields) for sentinel to hash and sign.
pub fn unsigned_tx_hex(tx: &TxEip1559) -> String {
    let mut buf = Vec::with_capacity(tx.payload_len_for_signature());
    tx.encode_for_signing(&mut buf);
    format!("0x{}", hex::encode(&buf))
}

pub fn min_balance_for_canary(base_fee: u128) -> U256 {
    let max_fee = (base_fee * MAX_FEE_BUFFER_MULTIPLIER).max(CANARY_PRIORITY_FEE_WEI + 1);
    U256::from(CANARY_GAS_LIMIT) * U256::from(max_fee) + U256::from(MAX_CANARY_VALUE)
}

#[derive(Debug, Clone)]
pub struct PendingPrepare {
    pub peer_id: PeerId,
    pub enode: String,
    pub nonce: u64,
    pub signer: Address,
    pub value_wei: u64,
}

#[derive(Debug, Clone)]
pub struct InflightProbe {
    pub peer_id: PeerId,
    pub enode: String,
    pub tx_hash: TxHash,
    pub nonce: u64,
    pub value_wei: u64,
    pub sent_at: Instant,
}

#[derive(Debug, Clone)]
struct TimedOutTrack {
    peer_id: PeerId,
    timed_out_at: Instant,
}

/// Shared PoG coordinator state (prepare/submit correlation + watcher).
pub struct PogCoordinator {
    db_path: PathBuf,
    db: Arc<PogDb>,
    pub chain_id: u64,
    pub pog_timeout: Duration,
    inner: Mutex<PogInner>,
}

struct PogInner {
    pending: Option<PendingPrepare>,
    inflight: Option<InflightProbe>,
    timed_out: HashMap<TxHash, TimedOutTrack>,
    funding_backoff_until: Option<Instant>,
    funding_backoff_secs: u64,
}

impl PogCoordinator {
    pub fn new(datadir: PathBuf, chain_id: u64) -> rusqlite::Result<Self> {
        let db_path = datadir.join("proof_of_gossip.db");
        let db = Arc::new(PogDb::open(&db_path)?);
        Ok(Self {
            db_path,
            db,
            chain_id,
            pog_timeout: Duration::from_secs(DEFAULT_POG_TIMEOUT_SECS),
            inner: Mutex::new(PogInner {
                pending: None,
                inflight: None,
                timed_out: HashMap::new(),
                funding_backoff_until: None,
                funding_backoff_secs: 0,
            }),
        })
    }

    pub fn pog_db(&self) -> Arc<PogDb> {
        Arc::clone(&self.db)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, PogInner> {
        self.inner.lock().expect("PogCoordinator: mutex poisoned (audit confirms no panic paths)")
    }

    pub fn take_pending(&self) -> Option<PendingPrepare> {
        self.lock_inner().pending.take()
    }

    pub fn set_pending(&self, p: PendingPrepare) {
        self.lock_inner().pending = Some(p);
    }

    pub fn set_inflight(&self, probe: InflightProbe) {
        self.lock_inner().inflight = Some(probe);
    }

    pub fn clear_inflight(&self) {
        self.lock_inner().inflight = None;
    }

    pub fn inflight_snapshot(&self) -> Option<InflightProbe> {
        self.lock_inner().inflight.clone()
    }

    pub fn has_inflight(&self) -> bool {
        self.lock_inner().inflight.is_some()
    }

    pub fn funding_backoff_active(&self) -> Option<Duration> {
        let g = self.lock_inner();
        let until = g.funding_backoff_until?;
        let now = Instant::now();
        if now < until { Some(until - now) } else { None }
    }

    pub fn record_underfunded(&self) {
        let mut g = self.lock_inner();
        g.funding_backoff_secs = if g.funding_backoff_secs == 0 {
            MIN_FUNDING_BACKOFF_SECS
        } else {
            (g.funding_backoff_secs * 2).min(MAX_FUNDING_BACKOFF_SECS)
        };
        g.funding_backoff_until =
            Some(Instant::now() + Duration::from_secs(g.funding_backoff_secs));
    }

    pub fn clear_funding_backoff(&self) {
        let mut g = self.lock_inner();
        g.funding_backoff_until = None;
        g.funding_backoff_secs = 0;
    }

    pub fn insert_timed_out(&self, tx_hash: TxHash, peer_id: PeerId) {
        self.lock_inner()
            .timed_out
            .insert(tx_hash, TimedOutTrack { peer_id, timed_out_at: Instant::now() });
    }

    pub fn remove_timed_out(&self, tx_hash: &TxHash) {
        self.lock_inner().timed_out.remove(tx_hash);
    }

    pub fn timed_out_peer(&self, tx_hash: &TxHash) -> Option<PeerId> {
        self.lock_inner().timed_out.get(tx_hash).map(|t| t.peer_id)
    }

    pub fn timed_out_tx_hashes(&self) -> Vec<TxHash> {
        self.lock_inner().timed_out.keys().copied().collect()
    }

    fn prune_timed_out_window(&self) {
        let window = Duration::from_secs(LATE_CONFIRMATION_TRACK_WINDOW_SECS);
        self.lock_inner().timed_out.retain(|_, t| t.timed_out_at.elapsed() <= window);
    }
}

pub trait PogProvider: Send + Sync {
    fn account_nonce(&self, address: &Address) -> eyre::Result<Option<u64>>;
    fn account_balance(&self, address: &Address) -> eyre::Result<Option<U256>>;
    fn latest_base_fee(&self) -> eyre::Result<u128>;
}

impl<P> PogProvider for P
where
    P: StateProviderFactory + BlockReaderIdExt<Header = BerachainHeader> + Send + Sync,
{
    fn account_nonce(&self, address: &Address) -> eyre::Result<Option<u64>> {
        Ok(self.latest()?.account_nonce(address)?)
    }

    fn account_balance(&self, address: &Address) -> eyre::Result<Option<U256>> {
        Ok(self.latest()?.account_balance(address)?)
    }

    fn latest_base_fee(&self) -> eyre::Result<u128> {
        let header =
            self.latest_header()?.ok_or_else(|| eyre::eyre!("no best block header"))?.into_header();
        let base_fee =
            header.base_fee_per_gas.ok_or_else(|| eyre::eyre!("latest block has no base fee"))?;
        Ok(base_fee as u128)
    }
}

pub const DEFAULT_PROVENANCE_TTL_SECS: u64 = 600;

/// In-memory tx→peer provenance window (first-seen-wins, TTL eviction).
pub struct ProvenanceWindow {
    entries: HashMap<TxHash, (PeerId, Instant)>,
    ttl: Duration,
}

impl ProvenanceWindow {
    pub fn new(ttl: Duration) -> Self {
        Self { entries: HashMap::new(), ttl }
    }

    pub fn insert(&mut self, tx_hash: TxHash, peer_id: PeerId) {
        self.entries.entry(tx_hash).or_insert((peer_id, Instant::now()));
    }

    pub fn get(&self, tx_hash: &TxHash) -> Option<PeerId> {
        self.entries.get(tx_hash).and_then(
            |(p, t)| {
                if t.elapsed() < self.ttl { Some(*p) } else { None }
            },
        )
    }

    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        self.entries.retain(|_, (_, t)| t.elapsed() < ttl);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Tracks block numbers sealed by this node (in-memory, TTL eviction).
pub struct SealedBlockRegistry {
    entries: HashMap<u64, Instant>,
    ttl: Duration,
    latest: Option<u64>,
}

impl SealedBlockRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self { entries: HashMap::new(), ttl, latest: None }
    }

    pub fn insert(&mut self, block_number: u64) {
        self.entries.insert(block_number, Instant::now());
        self.latest = Some(match self.latest {
            Some(prev) => prev.max(block_number),
            None => block_number,
        });
    }

    pub fn contains(&self, block_number: u64) -> bool {
        self.entries.get(&block_number).is_some_and(|t| t.elapsed() < self.ttl)
    }

    pub fn latest(&self) -> Option<u64> {
        self.latest
    }

    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        self.entries.retain(|_, t| t.elapsed() < ttl);
        self.latest = self.entries.keys().max().copied();
    }
}

/// Shared store for provenance window and sealed block registry.
pub struct PogAttributionStore {
    pub provenance: Mutex<ProvenanceWindow>,
    pub sealed: Mutex<SealedBlockRegistry>,
}

impl PogAttributionStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            provenance: Mutex::new(ProvenanceWindow::new(ttl)),
            sealed: Mutex::new(SealedBlockRegistry::new(ttl)),
        }
    }
}

impl Default for PogAttributionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(DEFAULT_PROVENANCE_TTL_SECS))
    }
}

/// Process-wide attribution store, initialized once (on first PoG-enabled startup).
static ATTRIBUTION_STORE: OnceLock<std::sync::Arc<PogAttributionStore>> = OnceLock::new();

/// Returns the global [`PogAttributionStore`], creating it on first call.
pub fn attribution_store() -> std::sync::Arc<PogAttributionStore> {
    ATTRIBUTION_STORE.get_or_init(|| std::sync::Arc::new(PogAttributionStore::default())).clone()
}

/// Background watcher that detects canary tx inclusion by scanning new canonical blocks.
///
/// Replaces the old polling `receipt_by_hash` approach, which failed when blocks were
/// flushed immediately to disk (`--engine.persistence-threshold 0`) because the MDBX
/// `TransactionHashNumbers` table is not populated during Engine API live sync.
pub async fn run_pog_watcher(
    shutdown: reth::tasks::shutdown::GracefulShutdown,
    coord: std::sync::Arc<PogCoordinator>,
    store: std::sync::Arc<PogAttributionStore>,
    mut canon_events: reth::providers::CanonStateNotifications<crate::primitives::BerachainPrimitives>,
) {
    use alloy_consensus::BlockHeader as _;
    use reth_primitives_traits::{BlockBody as _, transaction::TxHashRef as _};

    let mut shutdown = shutdown;
    let mut timeout_interval = tokio::time::interval(Duration::from_secs(WATCHER_TICK_SECS));
    timeout_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    info!(target: "bera_reth::pog_probe", "PoG probe watcher started (block-scan mode)");
    loop {
        tokio::select! {
            guard = &mut shutdown => {
                drop(guard);
                info!(target: "bera_reth::pog_probe", "PoG probe watcher stopped");
                return;
            }
            event = canon_events.recv() => {
                let chain = match event {
                    Ok(notification) => notification.committed(),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        info!(target: "bera_reth::pog_probe", skipped = n, "canon state stream lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!(target: "bera_reth::pog_probe", "canon state stream closed");
                        return;
                    }
                };
                for block in chain.blocks_iter() {
                    let block_num = block.header().number();
                    let tx_hashes: Vec<TxHash> = block
                        .body()
                        .transactions_iter()
                        .map(|tx| *tx.tx_hash())
                        .collect();

                    // Check inflight probe against this block's transactions
                    if let Some(inflight) = coord.inflight_snapshot()
                        && tx_hashes.contains(&inflight.tx_hash)
                    {
                        let _ = coord.pog_db().insert_peer_test(
                            &inflight.peer_id,
                            inflight.tx_hash,
                            "seen",
                        );
                        coord.clear_inflight();
                        coord.remove_timed_out(&inflight.tx_hash);
                        info!(
                            target: "bera_reth::pog_probe",
                            event = "probe.result",
                            outcome = "seen",
                            peer_id = %inflight.peer_id,
                            enode = %inflight.enode,
                            probe_id = %inflight.tx_hash,
                            nonce = inflight.nonce,
                            value_wei = inflight.value_wei,
                            block = block_num,
                            "canary receipt observed"
                        );
                    }

                    // Check timed-out probes (late reconciliation)
                    for tx_hash in coord.timed_out_tx_hashes() {
                        if tx_hashes.contains(&tx_hash) {
                            let Some(peer_id) = coord.timed_out_peer(&tx_hash) else {
                                continue;
                            };
                            let _ = coord.pog_db().insert_peer_test(&peer_id, tx_hash, "seen");
                            coord.remove_timed_out(&tx_hash);
                            info!(
                                target: "bera_reth::pog_probe",
                                event = "probe.result",
                                outcome = "seen",
                                peer_id = %peer_id,
                                probe_id = %tx_hash,
                                block = block_num,
                                late = true,
                                "timed-out canary appeared on-chain"
                            );
                        }
                    }
                }
            }
            _ = timeout_interval.tick() => {
                coord.prune_timed_out_window();
                store.provenance.lock().expect("provenance mutex poisoned").evict_expired();
                store.sealed.lock().expect("sealed mutex poisoned").evict_expired();

                // Check for timeout on inflight probe
                if let Some(inflight) = coord.inflight_snapshot()
                    && inflight.sent_at.elapsed() > coord.pog_timeout
                {
                    let _ = coord.pog_db().insert_peer_test(
                        &inflight.peer_id,
                        inflight.tx_hash,
                        "timeout",
                    );
                    coord.insert_timed_out(inflight.tx_hash, inflight.peer_id);
                    coord.clear_inflight();
                    info!(
                        target: "bera_reth::pog_probe",
                        event = "probe.result",
                        outcome = "timeout",
                        peer_id = %inflight.peer_id,
                        enode = %inflight.enode,
                        probe_id = %inflight.tx_hash,
                        nonce = inflight.nonce,
                        value_wei = inflight.value_wei,
                        elapsed_secs = inflight.sent_at.elapsed().as_secs(),
                        "canary probe timed out"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;
    use alloy_rlp::Decodable;
    use tempfile::NamedTempFile;

    // TP-2: provenance window retains mappings, evicts after TTL, first-seen-wins
    #[test]
    fn provenance_window_insert_and_get() {
        let mut w = ProvenanceWindow::new(Duration::from_secs(60));
        let tx = B256::random();
        let peer = PeerId::random();
        w.insert(tx, peer);
        assert_eq!(w.get(&tx), Some(peer));
    }

    #[test]
    fn provenance_window_first_seen_wins() {
        let mut w = ProvenanceWindow::new(Duration::from_secs(60));
        let tx = B256::random();
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        w.insert(tx, peer1);
        w.insert(tx, peer2);
        assert_eq!(w.get(&tx), Some(peer1), "first-seen-wins: peer1 should be retained");
    }

    #[test]
    fn provenance_window_evicts_after_ttl() {
        let mut w = ProvenanceWindow::new(Duration::from_millis(1));
        let tx = B256::random();
        let peer = PeerId::random();
        w.insert(tx, peer);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(w.get(&tx), None, "entry should be expired after TTL");
    }

    #[test]
    fn provenance_window_evict_expired_cleans_up() {
        let mut w = ProvenanceWindow::new(Duration::from_millis(1));
        for _ in 0..5 {
            w.insert(B256::random(), PeerId::random());
        }
        assert_eq!(w.len(), 5);
        std::thread::sleep(Duration::from_millis(5));
        w.evict_expired();
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn provenance_window_live_entries_not_evicted() {
        let mut w = ProvenanceWindow::new(Duration::from_secs(60));
        let tx = B256::random();
        let peer = PeerId::random();
        w.insert(tx, peer);
        w.evict_expired();
        assert_eq!(w.get(&tx), Some(peer));
    }

    // SealedBlockRegistry tests
    #[test]
    fn sealed_block_registry_insert_and_contains() {
        let mut r = SealedBlockRegistry::new(Duration::from_secs(60));
        r.insert(42);
        assert!(r.contains(42));
        assert!(!r.contains(43));
    }

    #[test]
    fn sealed_block_registry_latest_tracks_max() {
        let mut r = SealedBlockRegistry::new(Duration::from_secs(60));
        assert_eq!(r.latest(), None);
        r.insert(10);
        r.insert(5);
        r.insert(20);
        assert_eq!(r.latest(), Some(20));
    }

    #[test]
    fn sealed_block_registry_evicts_after_ttl() {
        let mut r = SealedBlockRegistry::new(Duration::from_millis(1));
        r.insert(1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(!r.contains(1));
    }

    #[test]
    fn sealed_block_registry_latest_updates_after_eviction() {
        let mut r = SealedBlockRegistry::new(Duration::from_millis(1));
        r.insert(10);
        r.insert(20);
        std::thread::sleep(Duration::from_millis(5));
        r.evict_expired();
        // After all entries evicted, latest() must reflect reality
        assert_eq!(r.latest(), None, "latest() must be None when all entries are evicted");

        // Insert a new block after eviction — latest should update
        r.insert(5);
        assert_eq!(r.latest(), Some(5));
    }

    #[test]
    fn sealed_block_registry_latest_partial_eviction() {
        // Insert blocks with different TTLs to test partial eviction.
        // Block 10 inserted first (expires first), block 20 later.
        let mut r = SealedBlockRegistry::new(Duration::from_millis(50));
        r.insert(10);
        std::thread::sleep(Duration::from_millis(30));
        r.insert(20);
        // Block 10 is ~30ms old, block 20 is ~0ms old. TTL is 50ms.
        // After another 25ms, block 10 will be expired but block 20 won't.
        std::thread::sleep(Duration::from_millis(25));
        r.evict_expired();
        assert!(!r.contains(10), "block 10 should be evicted");
        assert!(r.contains(20), "block 20 should survive");
        assert_eq!(
            r.latest(),
            Some(20),
            "latest() must reflect surviving entries, not evicted ones"
        );
    }

    #[test]
    fn unsigned_roundtrip_signing_hash_stable() {
        let to = Address::repeat_byte(0x42);
        let (tx, _) = build_unsigned_canary(to, 7, 80094, 1_000_000_000);
        let h1 = tx.signature_hash();
        let hex_str = unsigned_tx_hex(&tx);
        let bytes = hex::decode(hex_str.trim_start_matches("0x")).unwrap();
        assert_eq!(bytes[0], 0x02);
        let mut slice = &bytes[1..];
        let decoded = TxEip1559::decode(&mut slice).unwrap();
        assert_eq!(decoded.signature_hash(), h1);
    }

    #[test]
    fn sqlite_init_idempotent() {
        let tmp = NamedTempFile::new().unwrap();
        PogDb::open(tmp.path()).unwrap();
        PogDb::open(tmp.path()).unwrap();
    }

    #[test]
    fn pog_db_all_peer_statuses_batch_query() {
        let tmp = NamedTempFile::new().unwrap();
        let db = Connection::open(tmp.path()).unwrap();
        ensure_peer_tests_schema(&db).unwrap();
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        let p1 = PeerId::random();
        let p2 = PeerId::random();
        // p1: two timeouts then a seen
        for (result, ts) in [("timeout", 1), ("timeout", 2), ("seen", 3)] {
            db.execute(
                "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
                params![p1.to_string(), B256::random().to_string(), result, ts as i64],
            ).unwrap();
        }
        // p2: one timeout only
        db.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![p2.to_string(), B256::random().to_string(), "timeout", 10_i64],
        )
        .unwrap();
        drop(db);

        let pog_db = PogDb::open(tmp.path()).unwrap();
        let statuses = pog_db.all_peer_statuses().unwrap();
        assert_eq!(statuses.len(), 2);

        let s1 = &statuses[&p1.to_string()];
        assert_eq!(s1.last_result, "seen");
        assert_eq!(s1.failure_count, 2);
        assert_eq!(s1.last_tested_at, 3);

        let s2 = &statuses[&p2.to_string()];
        assert_eq!(s2.last_result, "timeout");
        assert_eq!(s2.failure_count, 1);
        assert_eq!(s2.last_tested_at, 10);
    }
}
