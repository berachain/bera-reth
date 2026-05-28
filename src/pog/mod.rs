//! Proof-of-Gossip node-side state: SQLite (`PogSqliteStore`), probe coordinator, watcher task,
//! and the durable sealed-tx-fact attribution store consumed by sentinel via the
//! `beradmin_exportSealedTxFacts` RPC.
//!
//! Autonomous signing/ticking was removed; sentinel drives prepare/sign/submit.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub mod peer_curation;

static POG_CLI_ENABLED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_PEER_CURATION: OnceLock<Mutex<Option<ShutdownPeerCurationConfig>>> =
    OnceLock::new();
static POG_SEALED_FACT_CONFIG: OnceLock<PogSealedFactConfig> = OnceLock::new();
static ATTRIBUTION_STORE: OnceLock<std::sync::Arc<PogAttributionStore>> = OnceLock::new();

#[derive(Debug, Clone)]
struct ShutdownPeerCurationConfig {
    known_peers_path: PathBuf,
    pog_db_path: PathBuf,
}

/// Operator-facing tuning for the durable sealed-tx-fact pipeline.
///
/// All three knobs are validated at CLI parse time (see `crate::cli_ext`); this struct is a
/// pure value carrier and does **not** repeat that validation.
#[derive(Debug, Clone, Copy)]
pub struct PogSealedFactConfig {
    /// Sealed-tx-fact retention window in hours. Used both for inline retention DELETE
    /// and the `pog_sealed_fact_retention_hours` gauge.
    pub retention_hours: u64,
    /// Hard cap on `InflightTransactions` entry count. When reached, an inline TTL sweep
    /// runs; if still at cap, new first-hear inserts are refused.
    pub max_inflight_entries: usize,
    /// Server-side upper bound accepted on the RPC `limit` parameter. Values above this
    /// cap produce a JSON-RPC error; values at or below are accepted as-is.
    pub export_max_limit: u32,
}

/// Default sealed-tx-fact retention window in hours (24h).
pub const DEFAULT_SEALED_FACT_RETENTION_HOURS: u64 = 24;
/// Default safety-belt cap on `InflightTransactions`.
pub const DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES: usize = 500_000;
/// Default server-side maximum accepted `limit` parameter on `beradmin_exportSealedTxFacts`.
pub const DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT: u32 = 10_000;

impl Default for PogSealedFactConfig {
    fn default() -> Self {
        Self {
            retention_hours: DEFAULT_SEALED_FACT_RETENTION_HOURS,
            max_inflight_entries: DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES,
            export_max_limit: DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT,
        }
    }
}

/// Set from `main` after parsing CLI. When false (default), PoG RPC modules and watcher are off.
pub fn set_pog_cli_enabled(enabled: bool) {
    POG_CLI_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn pog_cli_enabled() -> bool {
    POG_CLI_ENABLED.load(Ordering::SeqCst)
}

/// Install the process-wide `PogSealedFactConfig`. Must be called before the attribution
/// store is first accessed; subsequent calls are no-ops (first write wins).
pub fn set_sealed_fact_config(cfg: PogSealedFactConfig) {
    let _ = POG_SEALED_FACT_CONFIG.set(cfg);
}

/// Returns the active `PogSealedFactConfig`, falling back to defaults if unset.
pub fn sealed_fact_config() -> PogSealedFactConfig {
    POG_SEALED_FACT_CONFIG.get().copied().unwrap_or_default()
}

fn shutdown_peer_curation_slot() -> &'static Mutex<Option<ShutdownPeerCurationConfig>> {
    SHUTDOWN_PEER_CURATION.get_or_init(|| Mutex::new(None))
}

/// Capture the paths required for post-shutdown known-peers handling (PoG evidence →
/// `known-peers.json`).
///
/// When a persistent peers file is configured, registers
/// [`reth_node_builder::set_post_known_peers_write_hook`] so
/// [`run_shutdown_peer_curation_if_enabled`] runs **after** Reth’s graceful `known-peers.json`
/// flush (see `CliRunner::run_command_until_exit` / `post_known_peers_write`).
pub fn configure_shutdown_peer_curation(datadir: PathBuf, known_peers_path: Option<PathBuf>) {
    let mut guard = shutdown_peer_curation_slot().lock().unwrap_or_else(|e| e.into_inner());
    *guard = known_peers_path.map(|known_peers_path| ShutdownPeerCurationConfig {
        known_peers_path,
        pog_db_path: datadir.join("proof_of_gossip.db"),
    });

    if guard.is_some() {
        reth_node_builder::set_post_known_peers_write_hook(Some(Box::new(|_path: &Path| {
            run_shutdown_peer_curation_if_enabled();
        })));
    } else {
        reth_node_builder::set_post_known_peers_write_hook(None);
    }
}

/// Run known-peers filtering using the paths from [`configure_shutdown_peer_curation`].
///
/// Normally invoked from the `post_known_peers_write` hook registered in
/// [`configure_shutdown_peer_curation`]. Do **not** call this after `node_exit_future.await` in
/// `main.rs`: that runs before Reth’s graceful peer-file write and will be overwritten.
pub fn run_shutdown_peer_curation_if_enabled() {
    if !pog_cli_enabled() {
        return;
    }

    let config = shutdown_peer_curation_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(config) = config else {
        info!(
            target: "bera_reth::pog_peer_curation",
            "Skipping known-peers.json curation; persistent peers file is disabled"
        );
        return;
    };

    let _ = peer_curation::curate_known_peers_file(&config.known_peers_path, &config.pog_db_path);
}

use crate::primitives::BerachainHeader;
use crate::transaction::BerachainTxEnvelope;
use alloy_consensus::{SignableTransaction, TxEip1559};
use alloy_primitives::{Address, Bytes, TxHash, TxKind, U256, hex};
use rand::Rng;
use reth::providers::{BlockReaderIdExt, StateProviderFactory};
use reth_network_peers::{NodeRecord, PeerId};
use rusqlite::{Connection, params};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{error, info, warn};

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

/// TTL on in-RAM first-hear and locally-built-block tracking. 600s is long enough to cover
/// any realistic mempool residence time for a landable-then-landed tx plus the build→commit
/// latency for a locally-proposed block under healthy CL cadence.
pub const DEFAULT_INFLIGHT_TTL_SECS: u64 = 600;

pub(crate) fn ensure_peer_tests_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS peer_pog_status (
            peer_id        TEXT PRIMARY KEY,
            last_result    TEXT NOT NULL,
            last_tx_hash   TEXT NOT NULL,
            last_tested_at INTEGER NOT NULL,
            failure_count  INTEGER NOT NULL DEFAULT 0,
            success_count  INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS peer_pog_log (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            peer_id   TEXT NOT NULL,
            tx_hash   TEXT NOT NULL,
            result    TEXT NOT NULL,
            tested_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_pog_log_peer ON peer_pog_log(peer_id, tested_at);",
    )?;

    // Migrate from legacy `peer_tests` table if it exists.
    let legacy_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='peer_tests'",
        [],
        |row| row.get(0),
    )?;
    if legacy_exists {
        conn.execute_batch(
            "INSERT OR IGNORE INTO peer_pog_status (peer_id, last_result, last_tx_hash, last_tested_at, failure_count, success_count)
             SELECT peer_id,
                    (SELECT result   FROM peer_tests p2 WHERE p2.peer_id = p1.peer_id ORDER BY tested_at DESC LIMIT 1),
                    (SELECT tx_hash  FROM peer_tests p2 WHERE p2.peer_id = p1.peer_id ORDER BY tested_at DESC LIMIT 1),
                    (SELECT tested_at FROM peer_tests p2 WHERE p2.peer_id = p1.peer_id ORDER BY tested_at DESC LIMIT 1),
                    SUM(CASE WHEN result = 'timeout' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN result = 'seen' THEN 1 ELSE 0 END)
             FROM peer_tests p1
             GROUP BY peer_id;

             INSERT INTO peer_pog_log (peer_id, tx_hash, result, tested_at)
             SELECT peer_id, tx_hash, result, tested_at FROM peer_tests;

             DROP TABLE peer_tests;",
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_sealed_tx_fact_schema(conn: &Connection) -> rusqlite::Result<()> {
    // `effective_tip_wei` is stored as the exact wire-serialized hex-`u128` string so no
    // re-encoding happens at export time; see brief §5.5.
    //
    // `first_enode` (BERA-305) is the canonical `NodeRecord::Display` enode URL captured at
    // the peer's first-hear session, NULL for sessions where the peer signalled
    // `Hello.port == 0` and for pre-migration rows.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sealed_tx_fact (
            id                   INTEGER PRIMARY KEY AUTOINCREMENT,
            sealed_block_number  INTEGER NOT NULL,
            tx_hash              TEXT    NOT NULL,
            first_peer_id        TEXT    NULL,
            first_heard_ms       INTEGER NOT NULL,
            effective_tip_wei    TEXT    NOT NULL,
            tip_formula_version  INTEGER NOT NULL DEFAULT 1,
            first_enode          TEXT    NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sealed_tx_fact_first_heard_ms
            ON sealed_tx_fact(first_heard_ms);",
    )?;

    // BERA-305: probe-and-ALTER to backfill the `first_enode` column on databases that
    // were created by pre-migration bera-reth (where the column is absent). Idempotent:
    // re-running on an already-migrated DB is a no-op because the column already exists.
    let has_first_enode: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('sealed_tx_fact') WHERE name = 'first_enode'",
        [],
        |row| row.get(0),
    )?;
    if !has_first_enode {
        conn.execute_batch("ALTER TABLE sealed_tx_fact ADD COLUMN first_enode TEXT NULL")?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PogPeerStatus {
    pub last_result: String,
    pub failure_count: u32,
    pub last_tested_at: u64,
    pub last_tx_hash: String,
}

/// Durable SQLite store backing the Proof-of-Gossip peer-probe history and the
/// sealed-tx-fact attribution table (BERA-265). Opens two connections against the same
/// file: `write_conn` for probes / seal-flush / retention, `read_conn` for RPC export
/// handlers. WAL permits concurrent read+write so a large export pull never blocks a
/// seal-flush writer.
///
/// Instrumentation: every lock acquisition on either connection is counted via
/// `write_conn_lock_count` / `read_conn_lock_count`, enabling AC-R6 and TP-R8/TP-R9 to
/// assert the export-only-reads-read_conn invariant without plumbing custom mutex
/// wrappers through the RPC handler.
pub struct PogSqliteStore {
    write_conn: Mutex<Connection>,
    read_conn: Mutex<Connection>,
    write_conn_lock_count: AtomicU64,
    read_conn_lock_count: AtomicU64,
    sealed_tx_fact_row_count: AtomicU64,
    sealed_tx_fact_high_water_id: AtomicU64,
    sealed_tx_fact_min_retained_id: AtomicU64,
    sealed_facts_flushed_total: AtomicU64,
    sealed_facts_flushed_with_peer_total: AtomicU64,
    sealed_facts_retention_deleted_total: AtomicU64,
    sealed_facts_export_rows_total: AtomicU64,
}

/// Owned snapshot of a single `sealed_tx_fact` row used for export wire serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedTxFactRecord {
    pub id: u64,
    pub sealed_block_number: u64,
    pub tx_hash: String,
    pub first_peer_id: Option<String>,
    pub first_heard_ms: u64,
    /// Hex `u128` (ethereum-spec "Quantity" lowercase 0x-prefixed minimal encoding) as
    /// stored in SQLite; see brief §5.5.
    pub effective_tip_wei: String,
    pub tip_formula_version: u32,
    /// Canonical `enode://hex@ip:port` URL captured from the peer's first-hear session
    /// (`NodeRecord::new(listening_addr, peer_id).to_string()`), `None` for pre-migration
    /// rows or sessions whose `Hello.port == 0`. See BERA-305 brief.
    pub first_enode: Option<String>,
}

/// One row ready to be inserted into `sealed_tx_fact` during a seal-flush transaction.
#[derive(Debug, Clone)]
pub struct SealedTxFactInsert {
    pub sealed_block_number: u64,
    pub tx_hash: String,
    pub first_peer_id: Option<String>,
    pub first_heard_ms: u64,
    pub effective_tip_wei_hex: String,
    pub tip_formula_version: u32,
    /// Pre-rendered enode URL (per `NodeRecord::Display`) for the peer's first-hear session,
    /// `None` when no listening_addr was captured. BERA-305.
    pub first_enode: Option<String>,
}

impl PogSqliteStore {
    /// Open (or rename-and-recreate on `PRAGMA integrity_check` failure) the PoG store.
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        let write_conn = open_checked_conn(db_path)?;
        let read_conn = open_checked_conn(db_path)?;
        ensure_peer_tests_schema(&write_conn)?;
        ensure_sealed_tx_fact_schema(&write_conn)?;

        let store = Self {
            write_conn: Mutex::new(write_conn),
            read_conn: Mutex::new(read_conn),
            write_conn_lock_count: AtomicU64::new(0),
            read_conn_lock_count: AtomicU64::new(0),
            sealed_tx_fact_row_count: AtomicU64::new(0),
            sealed_tx_fact_high_water_id: AtomicU64::new(0),
            sealed_tx_fact_min_retained_id: AtomicU64::new(0),
            sealed_facts_flushed_total: AtomicU64::new(0),
            sealed_facts_flushed_with_peer_total: AtomicU64::new(0),
            sealed_facts_retention_deleted_total: AtomicU64::new(0),
            sealed_facts_export_rows_total: AtomicU64::new(0),
        };
        store.prime_startup_gauges()?;
        Ok(store)
    }

    fn lock_write(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.write_conn_lock_count.fetch_add(1, Ordering::Relaxed);
        self.write_conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_read(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.read_conn_lock_count.fetch_add(1, Ordering::Relaxed);
        self.read_conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn write_conn_lock_count(&self) -> u64 {
        self.write_conn_lock_count.load(Ordering::Relaxed)
    }

    pub fn read_conn_lock_count(&self) -> u64 {
        self.read_conn_lock_count.load(Ordering::Relaxed)
    }

    pub fn sealed_tx_fact_row_count(&self) -> u64 {
        self.sealed_tx_fact_row_count.load(Ordering::Relaxed)
    }

    pub fn sealed_tx_fact_high_water_id(&self) -> u64 {
        self.sealed_tx_fact_high_water_id.load(Ordering::Relaxed)
    }

    pub fn sealed_tx_fact_min_retained_id(&self) -> u64 {
        self.sealed_tx_fact_min_retained_id.load(Ordering::Relaxed)
    }

    pub fn sealed_facts_flushed_total(&self) -> u64 {
        self.sealed_facts_flushed_total.load(Ordering::Relaxed)
    }

    pub fn sealed_facts_flushed_with_peer_total(&self) -> u64 {
        self.sealed_facts_flushed_with_peer_total.load(Ordering::Relaxed)
    }

    pub fn sealed_facts_retention_deleted_total(&self) -> u64 {
        self.sealed_facts_retention_deleted_total.load(Ordering::Relaxed)
    }

    pub fn sealed_facts_export_rows_total(&self) -> u64 {
        self.sealed_facts_export_rows_total.load(Ordering::Relaxed)
    }

    /// Probe-test write (existing peer_pog_* semantics). Uses `write_conn`.
    pub fn insert_peer_test(
        &self,
        peer_id: &PeerId,
        tx_hash: TxHash,
        result: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.lock_write();
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let pid = peer_id.to_string();
        let txh = tx_hash.to_string();
        conn.execute(
            "INSERT INTO peer_pog_status (peer_id, last_result, last_tx_hash, last_tested_at, failure_count, success_count)
             VALUES (?1, ?2, ?3, ?4,
                     CASE WHEN ?2 = 'timeout' THEN 1 ELSE 0 END,
                     CASE WHEN ?2 = 'seen' THEN 1 ELSE 0 END)
             ON CONFLICT(peer_id) DO UPDATE SET
                 last_result    = excluded.last_result,
                 last_tx_hash   = excluded.last_tx_hash,
                 last_tested_at = excluded.last_tested_at,
                 failure_count  = failure_count + CASE WHEN excluded.last_result = 'timeout' THEN 1 ELSE 0 END,
                 success_count  = success_count + CASE WHEN excluded.last_result = 'seen' THEN 1 ELSE 0 END",
            params![pid, result, txh, ts],
        )?;
        conn.execute(
            "INSERT INTO peer_pog_log (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![pid, txh, result, ts],
        )?;
        Ok(())
    }

    /// Peer-probe status read. Uses `read_conn` (unrelated to the export path, but shares
    /// the same reader split so it doesn't contend with seal-flush writes).
    pub fn all_peer_statuses(&self) -> rusqlite::Result<HashMap<String, PogPeerStatus>> {
        let conn = self.lock_read();
        let mut stmt = conn.prepare(
            "SELECT peer_id, last_result, last_tested_at, last_tx_hash, failure_count
             FROM peer_pog_status",
        )?;
        let rows = stmt.query_map([], |row| {
            let peer_id: String = row.get(0)?;
            let last_result: String = row.get(1)?;
            let last_tested_at: i64 = row.get(2)?;
            let last_tx_hash: String = row.get(3)?;
            let failure_count: u32 = row.get(4)?;
            Ok((
                peer_id,
                PogPeerStatus {
                    last_result,
                    failure_count,
                    last_tested_at: last_tested_at as u64,
                    last_tx_hash,
                },
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Insert sealed-tx-fact rows and apply inline retention in a single WAL transaction.
    /// Invoked from the seal-flush task for canonical blocks whose number is present in
    /// `LocallyBuiltBlocks`; see brief §5.3.
    ///
    /// Returns `(inserted_high_water_id, rows_deleted_by_retention)`.
    pub fn flush_sealed_tx_facts(
        &self,
        rows: &[SealedTxFactInsert],
        retention_cutoff_ms: u64,
    ) -> rusqlite::Result<(Option<u64>, u64)> {
        let mut conn = self.lock_write();
        let tx = conn.transaction()?;
        let mut new_high_water: Option<u64> = None;
        let mut inserted_with_peer: u64 = 0;
        // BERA-305 / VC-1: bucket sealed_tx_fact writes by `first_enode` outcome so VC-1
        // (Hello.port hit-rate spike) can be graphed at insert time, not just by paging
        // the SQLite table. Buckets:
        //   - present:              first_peer_id IS NOT NULL && first_enode IS NOT NULL
        //   - null_hello_port_zero: first_peer_id IS NOT NULL && first_enode IS NULL
        //                            (peer attributed but Hello.port=0 — the VC-1 case)
        //   - null_no_peer:         first_peer_id IS NULL (locally-built / RPC-only;
        //                            first_enode is structurally NULL)
        let mut sealed_first_enode_present: u64 = 0;
        let mut sealed_first_enode_null_hello_port_zero: u64 = 0;
        let mut sealed_first_enode_null_no_peer: u64 = 0;
        for row in rows {
            tx.execute(
                "INSERT INTO sealed_tx_fact \
                    (sealed_block_number, tx_hash, first_peer_id, first_heard_ms, \
                     effective_tip_wei, tip_formula_version, first_enode) \
                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.sealed_block_number as i64,
                    row.tx_hash,
                    row.first_peer_id,
                    row.first_heard_ms as i64,
                    row.effective_tip_wei_hex,
                    row.tip_formula_version as i64,
                    row.first_enode,
                ],
            )?;
            let id = tx.last_insert_rowid() as u64;
            new_high_water = Some(new_high_water.map(|h| h.max(id)).unwrap_or(id));
            match (row.first_peer_id.is_some(), row.first_enode.is_some()) {
                (true, true) => {
                    inserted_with_peer += 1;
                    sealed_first_enode_present += 1;
                }
                (true, false) => {
                    inserted_with_peer += 1;
                    sealed_first_enode_null_hello_port_zero += 1;
                }
                (false, _) => {
                    sealed_first_enode_null_no_peer += 1;
                }
            }
        }

        let deleted: usize = tx.execute(
            "DELETE FROM sealed_tx_fact WHERE first_heard_ms < ?1",
            params![retention_cutoff_ms as i64],
        )?;
        tx.commit()?;
        drop(conn);

        let inserted = rows.len() as u64;
        self.sealed_tx_fact_row_count.fetch_add(inserted, Ordering::Relaxed);
        self.sealed_tx_fact_row_count.fetch_sub(deleted as u64, Ordering::Relaxed);
        self.sealed_facts_flushed_total.fetch_add(inserted, Ordering::Relaxed);
        self.sealed_facts_flushed_with_peer_total.fetch_add(inserted_with_peer, Ordering::Relaxed);
        self.sealed_facts_retention_deleted_total.fetch_add(deleted as u64, Ordering::Relaxed);
        if let Some(h) = new_high_water {
            self.sealed_tx_fact_high_water_id.fetch_max(h, Ordering::Relaxed);
        }
        self.refresh_min_retained_id();

        metrics::counter!("pog_sealed_tx_facts_flushed_total").increment(inserted);
        metrics::counter!("pog_sealed_tx_facts_flushed_with_peer_total")
            .increment(inserted_with_peer);
        // BERA-305 / VC-1 buckets — see comment above.
        metrics::counter!(
            "pog_sealed_tx_facts_flushed_first_enode_total",
            "outcome" => "present",
        )
        .increment(sealed_first_enode_present);
        metrics::counter!(
            "pog_sealed_tx_facts_flushed_first_enode_total",
            "outcome" => "null_hello_port_zero",
        )
        .increment(sealed_first_enode_null_hello_port_zero);
        metrics::counter!(
            "pog_sealed_tx_facts_flushed_first_enode_total",
            "outcome" => "null_no_peer",
        )
        .increment(sealed_first_enode_null_no_peer);
        metrics::counter!("pog_sealed_tx_facts_retention_deleted_total").increment(deleted as u64);
        metrics::gauge!("pog_sealed_tx_fact_row_count").set(self.sealed_tx_fact_row_count() as f64);
        metrics::gauge!("pog_sealed_tx_fact_high_water_id")
            .set(self.sealed_tx_fact_high_water_id() as f64);
        metrics::gauge!("pog_sealed_tx_fact_min_retained_id")
            .set(self.sealed_tx_fact_min_retained_id() as f64);

        Ok((new_high_water, deleted as u64))
    }

    /// One-shot retention sweep run at startup before the first `CanonStateNotification`
    /// lands, so a node restarted after a long offline window doesn't pay the catch-up
    /// DELETE cost inside the first seal-flush transaction (see brief §5.3).
    pub fn startup_retention_sweep(&self, retention_cutoff_ms: u64) -> rusqlite::Result<u64> {
        let conn = self.lock_write();
        let deleted: usize = conn.execute(
            "DELETE FROM sealed_tx_fact WHERE first_heard_ms < ?1",
            params![retention_cutoff_ms as i64],
        )?;
        drop(conn);
        self.sealed_tx_fact_row_count.fetch_sub(deleted as u64, Ordering::Relaxed);
        self.sealed_facts_retention_deleted_total.fetch_add(deleted as u64, Ordering::Relaxed);
        self.refresh_min_retained_id();
        metrics::counter!("pog_sealed_tx_facts_retention_deleted_total").increment(deleted as u64);
        metrics::gauge!("pog_sealed_tx_fact_row_count").set(self.sealed_tx_fact_row_count() as f64);
        metrics::gauge!("pog_sealed_tx_fact_min_retained_id")
            .set(self.sealed_tx_fact_min_retained_id() as f64);
        Ok(deleted as u64)
    }

    /// Cursor-paginated export for `beradmin_exportSealedTxFacts`. Uses `read_conn`.
    ///
    /// Returns up to `limit` rows with `id > after_id` plus the current
    /// `(high_water_id, min_retained_id, truncated)` trio per the brief's wire shape.
    pub fn export_sealed_tx_facts(
        &self,
        after_id: u64,
        limit: u32,
    ) -> rusqlite::Result<ExportSealedTxFactsOutcome> {
        let conn = self.lock_read();

        let high_water: i64 =
            conn.query_row("SELECT COALESCE(MAX(id), 0) FROM sealed_tx_fact", [], |r| r.get(0))?;
        let min_retained: i64 =
            conn.query_row("SELECT COALESCE(MIN(id), 0) FROM sealed_tx_fact", [], |r| r.get(0))?;

        let mut stmt = conn.prepare(
            "SELECT id, sealed_block_number, tx_hash, first_peer_id, \
                    first_heard_ms, effective_tip_wei, tip_formula_version, first_enode \
             FROM sealed_tx_fact \
             WHERE id > ?1 \
             ORDER BY id ASC \
             LIMIT ?2",
        )?;
        let rows_iter = stmt.query_map(params![after_id as i64, limit as i64], |row| {
            let id: i64 = row.get(0)?;
            let sealed_block_number: i64 = row.get(1)?;
            let tx_hash: String = row.get(2)?;
            let first_peer_id: Option<String> = row.get(3)?;
            let first_heard_ms: i64 = row.get(4)?;
            let effective_tip_wei: String = row.get(5)?;
            let tip_formula_version: i64 = row.get(6)?;
            let first_enode: Option<String> = row.get(7)?;
            Ok(SealedTxFactRecord {
                id: id as u64,
                sealed_block_number: sealed_block_number as u64,
                tx_hash,
                first_peer_id,
                first_heard_ms: first_heard_ms as u64,
                effective_tip_wei,
                tip_formula_version: tip_formula_version as u32,
                first_enode,
            })
        })?;

        let mut rows = Vec::new();
        for r in rows_iter {
            rows.push(r?);
        }

        let next_after_id = rows.last().map(|r| r.id).unwrap_or(after_id);
        let truncated = (next_after_id as i64) < high_water;

        drop(stmt);
        drop(conn);

        let served = rows.len() as u64;
        self.sealed_facts_export_rows_total.fetch_add(served, Ordering::Relaxed);
        metrics::counter!("pog_sealed_tx_facts_export_rows_total").increment(served);

        Ok(ExportSealedTxFactsOutcome {
            rows,
            next_after_id,
            high_water_id: high_water as u64,
            min_retained_id: min_retained as u64,
            truncated,
        })
    }

    fn prime_startup_gauges(&self) -> rusqlite::Result<()> {
        let conn = self.lock_read();
        let row_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM sealed_tx_fact", [], |r| r.get(0))?;
        let max_id: i64 =
            conn.query_row("SELECT COALESCE(MAX(id), 0) FROM sealed_tx_fact", [], |r| r.get(0))?;
        let min_id: i64 =
            conn.query_row("SELECT COALESCE(MIN(id), 0) FROM sealed_tx_fact", [], |r| r.get(0))?;
        drop(conn);
        self.sealed_tx_fact_row_count.store(row_count as u64, Ordering::Relaxed);
        self.sealed_tx_fact_high_water_id.store(max_id as u64, Ordering::Relaxed);
        self.sealed_tx_fact_min_retained_id.store(min_id as u64, Ordering::Relaxed);
        Ok(())
    }

    fn refresh_min_retained_id(&self) {
        if let Ok(conn) = self.read_conn.lock() {
            // Intentionally bypass `lock_read` counter: this is an internal post-write
            // fix-up, not an RPC read; counting it would confuse TP-R8/TP-R9 assertions.
            if let Ok(min_id) = conn.query_row::<i64, _, _>(
                "SELECT COALESCE(MIN(id), 0) FROM sealed_tx_fact",
                [],
                |r| r.get(0),
            ) {
                self.sealed_tx_fact_min_retained_id.store(min_id as u64, Ordering::Relaxed);
            }
        }
    }
}

/// Outcome of a single `export_sealed_tx_facts` query, ready to be mapped onto the
/// wire type defined in `crate::rpc::bera_admin::types::ExportSealedTxFactsResponse`.
#[derive(Debug, Clone)]
pub struct ExportSealedTxFactsOutcome {
    pub rows: Vec<SealedTxFactRecord>,
    pub next_after_id: u64,
    pub high_water_id: u64,
    pub min_retained_id: u64,
    pub truncated: bool,
}

/// Opens a connection with the fixed PoG pragma set. On `PRAGMA integrity_check` failure,
/// renames the existing DB to `proof_of_gossip.db.corrupt.<unix_ts>` and creates a fresh
/// one per brief §5.4. Returns the prepared connection.
fn open_checked_conn(db_path: &Path) -> rusqlite::Result<Connection> {
    // Open first; attempt integrity_check *before* applying PRAGMAs, because
    // `pragma_update` issues a statement that itself errors on a non-SQLite header
    // and would mask the recoverable "file is corrupt" case with a hard error.
    let conn = Connection::open(db_path)?;
    let integrity: String = match conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)) {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(
                target: "bera_reth::pog_store",
                db_file = ?db_path,
                error = %err,
                "integrity_check returned an error; treating as corruption"
            );
            "fail".to_string()
        }
    };
    if integrity != "ok" {
        drop(conn);
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let corrupt_path = {
            let mut p = db_path.to_path_buf();
            let current = p.extension().and_then(|s| s.to_str()).unwrap_or("db").to_string();
            p.set_extension(format!("{current}.corrupt.{ts}"));
            p
        };
        error!(
            target: "bera_reth::pog_store",
            db_file = ?db_path,
            corrupt_file = ?corrupt_path,
            integrity = %integrity,
            "PoG SQLite integrity_check failed; renaming corrupt file and recreating empty store"
        );
        // Best-effort rename; if rename fails fall back to unlinking so we can still boot.
        if let Err(err) = std::fs::rename(db_path, &corrupt_path) {
            warn!(
                target: "bera_reth::pog_store",
                db_file = ?db_path,
                error = %err,
                "rename to corrupt-path failed; deleting so node can boot clean"
            );
            let _ = std::fs::remove_file(db_path);
        }
        // Also remove WAL/SHM sidecars to avoid reviving the corrupt state.
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = db_path.as_os_str().to_owned();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(sidecar));
        }
        let fresh = Connection::open(db_path)?;
        apply_pragmas(&fresh)?;
        return Ok(fresh);
    }
    apply_pragmas(&conn)?;
    Ok(conn)
}

fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    // `auto_vacuum=FULL` must be set before any CREATE TABLE on a newly-initialized file
    // (brief §5.4); setting it on an existing populated file is a no-op unless followed
    // by an explicit VACUUM, which matches SQLite's documented behavior.
    conn.pragma_update(None, "auto_vacuum", "FULL")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
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
    db: Arc<PogSqliteStore>,
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
        let db = Arc::new(PogSqliteStore::open(&db_path)?);
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

    pub fn store(&self) -> Arc<PogSqliteStore> {
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

/// First-hear-wins attribution of tx hashes to the p2p peer that first successfully added
/// them to the pool. Replaces the RAM-only `ProvenanceWindow` with a cap-aware variant
/// used by the seal-flush path (brief §5.1). Subsequent-peer relay tracking is deferred
/// to BERA-261 (blocked by upstream reth BERA-260).
pub struct InflightTransactions {
    entries: HashMap<TxHash, InflightTx>,
    ttl: Duration,
    max_entries: usize,
    cap_rejections: AtomicU64,
    first_hears: AtomicU64,
    ttl_evictions: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
pub struct InflightTx {
    pub first_peer_id: PeerId,
    pub first_heard_ms: u64,
    pub first_heard_at: Instant,
    /// The peer's first-hear advertised listening socket — see BERA-305 brief and
    /// `TransactionProvenanceSink::record_accepted_from_peer`. `None` when the peer
    /// signalled `Hello.port == 0`.
    pub first_listening_addr: Option<SocketAddr>,
}

impl InflightTransactions {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            max_entries,
            cap_rejections: AtomicU64::new(0),
            first_hears: AtomicU64::new(0),
            ttl_evictions: AtomicU64::new(0),
        }
    }

    /// First-seen-wins insert. Returns `true` if the entry was accepted (new or already
    /// present under a different first hear), `false` if refused by the safety belt.
    ///
    /// On cap: runs an inline TTL sweep; if still at cap, refuses the insert and bumps
    /// `pog_inflight_tx_cap_rejections_total`.
    ///
    /// `first_listening_addr` (BERA-305) is the peer's advertised devp2p Hello socket,
    /// preserved through the seal-flush path so we can persist a re-dialable
    /// `first_enode`. When it is `None` (`Hello.port == 0`), we **do not** insert an
    /// inflight row: downstream cannot build an enode URL or join the sentinel fleet
    /// registry from attribution alone, so attributing the tx to that peer is dropped.
    pub fn record_first_hear(
        &mut self,
        tx_hash: TxHash,
        peer_id: PeerId,
        first_listening_addr: Option<SocketAddr>,
        now_ms: u64,
    ) -> bool {
        // BERA-305 / VC-1: split `first_hears_total` by whether the upstream provenance
        // sink supplied a listening_addr (Hello.port != 0). Operators graph
        // sum(rate{listening_addr_present="true"}) / sum(rate(...)) for the Hello.port
        // hit-rate the brief calls out, without table-scanning sealed_tx_fact.
        let listening_addr_present = if first_listening_addr.is_some() { "true" } else { "false" };
        if self.entries.contains_key(&tx_hash) {
            self.first_hears.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(
                "pog_inflight_tx_first_hears_total",
                "listening_addr_present" => listening_addr_present,
            )
            .increment(1);
            return true;
        }
        if first_listening_addr.is_none() {
            metrics::counter!("pog_inflight_tx_first_hears_skipped_no_listening_addr_total")
                .increment(1);
            return true;
        }
        if self.entries.len() >= self.max_entries {
            self.evict_expired();
            if self.entries.len() >= self.max_entries {
                self.cap_rejections.fetch_add(1, Ordering::Relaxed);
                metrics::counter!("pog_inflight_tx_cap_rejections_total").increment(1);
                warn!(
                    target: "bera_reth::pog_inflight",
                    tx_hash = %tx_hash,
                    peer_id = %peer_id,
                    cap = self.max_entries,
                    "InflightTransactions cap reached after TTL sweep; refusing first-hear insert"
                );
                return false;
            }
        }
        self.entries.insert(
            tx_hash,
            InflightTx {
                first_peer_id: peer_id,
                first_heard_ms: now_ms,
                first_heard_at: Instant::now(),
                first_listening_addr,
            },
        );
        self.first_hears.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "pog_inflight_tx_first_hears_total",
            "listening_addr_present" => listening_addr_present,
        )
        .increment(1);
        metrics::gauge!("pog_inflight_tx_count").set(self.entries.len() as f64);
        true
    }

    /// Extract-and-remove entries for each hash in `hashes`. Rows with no RAM state
    /// (never seen via p2p, or evicted) map to `None`. Used by the seal-flush path.
    pub fn drain_for_seal(&mut self, hashes: &[TxHash]) -> Vec<(TxHash, Option<InflightTx>)> {
        let drained: Vec<(TxHash, Option<InflightTx>)> =
            hashes.iter().map(|h| (*h, self.entries.remove(h))).collect();
        metrics::gauge!("pog_inflight_tx_count").set(self.entries.len() as f64);
        drained
    }

    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        let before = self.entries.len();
        self.entries.retain(|_, v| v.first_heard_at.elapsed() < ttl);
        let removed = before.saturating_sub(self.entries.len()) as u64;
        if removed > 0 {
            self.ttl_evictions.fetch_add(removed, Ordering::Relaxed);
            metrics::counter!("pog_inflight_tx_ttl_evictions_total").increment(removed);
        }
        metrics::gauge!("pog_inflight_tx_count").set(self.entries.len() as f64);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn cap_rejections(&self) -> u64 {
        self.cap_rejections.load(Ordering::Relaxed)
    }

    pub fn first_hears(&self) -> u64 {
        self.first_hears.load(Ordering::Relaxed)
    }

    pub fn ttl_evictions(&self) -> u64 {
        self.ttl_evictions.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn contains(&self, tx_hash: &TxHash) -> bool {
        self.entries.contains_key(tx_hash)
    }
}

/// Shared store for in-RAM first-hear attribution.
pub struct PogAttributionStore {
    pub inflight: Mutex<InflightTransactions>,
}

impl PogAttributionStore {
    pub fn new(cfg: PogSealedFactConfig) -> Self {
        let ttl = Duration::from_secs(DEFAULT_INFLIGHT_TTL_SECS);
        Self {
            inflight: Mutex::new(InflightTransactions::new(ttl, cfg.max_inflight_entries)),
        }
    }
}

impl Default for PogAttributionStore {
    fn default() -> Self {
        Self::new(PogSealedFactConfig::default())
    }
}

/// Returns the process-wide `PogAttributionStore`, initializing it with the installed
/// `PogSealedFactConfig` (or defaults) on first call.
pub fn attribution_store() -> std::sync::Arc<PogAttributionStore> {
    ATTRIBUTION_STORE
        .get_or_init(|| std::sync::Arc::new(PogAttributionStore::new(sealed_fact_config())))
        .clone()
}

/// Background watcher: consumes `CanonStateNotifications::Commit` for probe
/// reconciliation **and** funnels locally-built-block commits through the seal-flush
/// path. Also ticks every `WATCHER_TICK_SECS` to evict expired inflight and
/// locally-built-block entries.
pub async fn run_pog_watcher<Provider>(
    shutdown: reth::tasks::shutdown::GracefulShutdown,
    coord: std::sync::Arc<PogCoordinator>,
    store: std::sync::Arc<PogAttributionStore>,
    provider: Provider,
    mut canon_events: reth::providers::CanonStateNotifications<
        crate::primitives::BerachainPrimitives,
    >,
    cfg: PogSealedFactConfig,
) where
    Provider: reth_storage_api::BlockReader<Block = crate::primitives::BerachainBlock>
        + reth_storage_api::ReceiptProvider<
            Receipt = reth_ethereum_primitives::Receipt<crate::transaction::BerachainTxType>,
        > + Send
        + Sync
        + 'static,
{
    use alloy_consensus::BlockHeader as _;
    use reth_primitives_traits::{BlockBody as _, transaction::TxHashRef as _};

    let mut shutdown = shutdown;
    let mut timeout_interval = tokio::time::interval(Duration::from_secs(WATCHER_TICK_SECS));
    timeout_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    info!(target: "bera_reth::pog_probe", "PoG probe watcher started (block-scan mode)");
    // Startup retention sweep per brief §5.3: drop stale rows before first notification
    // so a post-offline restart doesn't burn its first seal-flush on catch-up DELETEs.
    match coord.store().startup_retention_sweep(retention_cutoff_ms(cfg.retention_hours)) {
        Ok(deleted) => info!(
            target: "bera_reth::pog_store",
            deleted,
            retention_hours = cfg.retention_hours,
            "startup sealed-tx-fact retention sweep complete"
        ),
        Err(err) => warn!(
            target: "bera_reth::pog_store",
            error = %err,
            "startup sealed-tx-fact retention sweep failed (continuing)"
        ),
    }
    metrics::gauge!("pog_sealed_fact_retention_hours").set(cfg.retention_hours as f64);
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

                    // Seal-flush: persist sealed_tx_fact rows unconditionally.
                    // See brief §Approach / BERA-268.
                    let flush_start = Instant::now();
                    if let Err(err) =
                        run_seal_flush_from_canon(&coord, &store, &provider, block_num, block, cfg)
                    {
                        warn!(
                            target: "bera_reth::pog_store",
                            block = block_num,
                            error = %err,
                            "seal-flush failed (continuing; rows will not land for this block)"
                        );
                    } else {
                        let histogram =
                            metrics::histogram!("pog_sealed_flush_duration_seconds");
                        histogram.record(flush_start.elapsed().as_secs_f64());
                    }

                    // Probe-reconciliation path (existing behavior).
                    if let Some(inflight) = coord.inflight_snapshot()
                        && tx_hashes.contains(&inflight.tx_hash)
                    {
                        let _ = coord.store().insert_peer_test(
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

                    for tx_hash in coord.timed_out_tx_hashes() {
                        if tx_hashes.contains(&tx_hash) {
                            let Some(peer_id) = coord.timed_out_peer(&tx_hash) else {
                                continue;
                            };
                            let _ = coord.store().insert_peer_test(&peer_id, tx_hash, "seen");
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
                if let Ok(mut inflight) = store.inflight.lock() {
                    inflight.evict_expired();
                }

                if let Some(inflight) = coord.inflight_snapshot()
                    && inflight.sent_at.elapsed() > coord.pog_timeout
                {
                    let _ = coord.store().insert_peer_test(
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

/// Current seal-flush retention cutoff in milliseconds-since-unix-epoch.
pub fn retention_cutoff_ms(retention_hours: u64) -> u64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    now_ms.saturating_sub(retention_hours.saturating_mul(3_600_000))
}

/// BERA-325: walks every `(tx, receipt)` in block order. `BerachainTxEnvelope::Berachain`
/// (PoL / 0x7e) txs are omitted from the returned `tx_hashes` / `tips` (they never become
/// `sealed_tx_fact` rows), but each receipt still advances the cumulative-gas running
/// total so effective-tip arithmetic matches execution order.
///
/// Returns `(filtered_tx_hashes, tips_per_filtered_tx, system_tx_skipped_count)`.
pub fn collect_seal_flush_tx_hashes_and_tips(
    base_fee: u128,
    transactions: &[&BerachainTxEnvelope],
    receipts: &[reth_ethereum_primitives::Receipt<crate::transaction::BerachainTxType>],
) -> eyre::Result<(Vec<TxHash>, Vec<u128>, u64)> {
    use alloy_consensus::Transaction as _;
    use reth_primitives_traits::transaction::TxHashRef as _;

    if transactions.len() != receipts.len() {
        return Err(eyre::eyre!(
            "seal-flush tx/receipt length mismatch: {} txs vs {} receipts",
            transactions.len(),
            receipts.len()
        ));
    }
    let mut tx_hashes = Vec::new();
    let mut tips = Vec::new();
    let mut system_skipped: u64 = 0;
    let mut prev_cumulative: u64 = 0;

    for (&tx, receipt) in transactions.iter().zip(receipts.iter()) {
        let gas_used = receipt.cumulative_gas_used.saturating_sub(prev_cumulative);
        prev_cumulative = receipt.cumulative_gas_used;

        match tx {
            BerachainTxEnvelope::Berachain(_) => {
                system_skipped += 1;
            }
            BerachainTxEnvelope::Ethereum(_) => {
                let eff_price = tx.effective_gas_price(Some(base_fee as u64));
                let tip = eff_price.saturating_sub(base_fee) * gas_used as u128;
                tips.push(tip);
                tx_hashes.push(*tx.tx_hash());
            }
        }
    }

    eyre::ensure!(
        tx_hashes.len() == tips.len(),
        "internal: filtered tx_hashes / tips length mismatch"
    );
    Ok((tx_hashes, tips, system_skipped))
}

/// Run the seal-flush path for a single locally-built committed block arriving on the
/// canonical-state stream. Extracted so the `canon_events.recv()` arm stays readable.
fn run_seal_flush_from_canon<Provider>(
    coord: &PogCoordinator,
    store: &PogAttributionStore,
    provider: &Provider,
    block_num: u64,
    block: &reth_primitives_traits::RecoveredBlock<crate::primitives::BerachainBlock>,
    cfg: PogSealedFactConfig,
) -> eyre::Result<()>
where
    Provider: reth_storage_api::ReceiptProvider<
            Receipt = reth_ethereum_primitives::Receipt<crate::transaction::BerachainTxType>,
        >,
{
    use alloy_consensus::BlockHeader as _;
    use reth_primitives_traits::BlockBody as _;

    let header = block.header();
    let base_fee = header.base_fee_per_gas().unwrap_or(0) as u128;
    let body = block.body();
    let txs: Vec<&BerachainTxEnvelope> = body.transactions_iter().collect();

    if txs.is_empty() {
        // Empty block — still run inline retention DELETE so file doesn't grow.
        let cutoff = retention_cutoff_ms(cfg.retention_hours);
        let _ = coord.store().flush_sealed_tx_facts(&[], cutoff)?;
        return Ok(());
    }

    let receipts = provider
        .receipts_by_block(alloy_eips::BlockHashOrNumber::Number(block_num))?
        .unwrap_or_default();
    if txs.len() != receipts.len() {
        return Err(eyre::eyre!(
            "seal-flush: block {block_num} has {} txs but {} receipts",
            txs.len(),
            receipts.len()
        ));
    }

    let (tx_hashes, tips, system_skipped) =
        collect_seal_flush_tx_hashes_and_tips(base_fee, &txs, &receipts)?;
    if system_skipped > 0 {
        metrics::counter!("pog_sealed_flush_tx_skipped_total", "reason" => "system_tx")
            .increment(system_skipped);
    }

    if tx_hashes.is_empty() {
        // All txs filtered (e.g. PoL-only block) — still run inline retention DELETE.
        let cutoff = retention_cutoff_ms(cfg.retention_hours);
        let _ = coord.store().flush_sealed_tx_facts(&[], cutoff)?;
        return Ok(());
    }

    let drained = match store.inflight.lock() {
        Ok(mut g) => g.drain_for_seal(&tx_hashes),
        Err(poison) => poison.into_inner().drain_for_seal(&tx_hashes),
    };

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    let first_heard_histogram = metrics::histogram!("pog_sealed_first_heard_to_sealed_ms");
    for (_h, entry) in drained.iter() {
        if let Some(e) = entry {
            first_heard_histogram.record(now_ms.saturating_sub(e.first_heard_ms) as f64);
        }
    }

    let rows = build_sealed_tx_fact_inserts(block_num, &drained, &tips, now_ms);
    let cutoff = retention_cutoff_ms(cfg.retention_hours);
    coord.store().flush_sealed_tx_facts(&rows, cutoff)?;
    Ok(())
}

/// Returns the effective-tip (`(effective_gas_price - base_fee) * gas_used`) per tx in
/// the block, preserving the receipt order. Mirrors the tip-formula previously in the
/// `sealed_block_attribution` RPC (brief §Context Payload).
///
/// Seal-flush uses [`collect_seal_flush_tx_hashes_and_tips`] instead, which applies the
/// BERA-325 system-tx filter while still walking every receipt for cumulative gas.
pub fn compute_effective_tips_from_receipts(
    base_fee: u128,
    effective_gas_prices: &[u128],
    receipts: &[reth_ethereum_primitives::Receipt<crate::transaction::BerachainTxType>],
) -> Vec<u128> {
    let mut prev_cumulative: u64 = 0;
    let mut tips = Vec::with_capacity(receipts.len());
    for (eff_price, receipt) in effective_gas_prices.iter().zip(receipts.iter()) {
        let gas_used = receipt.cumulative_gas_used.saturating_sub(prev_cumulative);
        prev_cumulative = receipt.cumulative_gas_used;
        let tip = eff_price.saturating_sub(base_fee) * gas_used as u128;
        tips.push(tip);
    }
    tips
}

/// Build seal-flush insert rows for a committed, locally-built block. Extracted for
/// testability; callers (the watcher) compute `tips` from the block body + receipts via
/// [`collect_seal_flush_tx_hashes_and_tips`] (BERA-325: excludes PoL system txs while
/// preserving cumulative-gas alignment).
pub fn build_sealed_tx_fact_inserts(
    block_num: u64,
    drained: &[(TxHash, Option<InflightTx>)],
    tips: &[u128],
    now_ms: u64,
) -> Vec<SealedTxFactInsert> {
    let mut rows = Vec::with_capacity(drained.len());
    for ((hash, ram), tip) in drained.iter().zip(tips.iter()) {
        let (first_peer_id, first_heard_ms, first_enode) = match ram {
            Some(entry) => {
                // BERA-305: render the canonical `enode://hex@ip:port` form via
                // `NodeRecord::Display` when both peer_id and listening_addr are
                // present. Hello.port=0 sessions yield None → first_enode = NULL.
                let enode = entry
                    .first_listening_addr
                    .map(|addr| NodeRecord::new(addr, entry.first_peer_id).to_string());
                (Some(entry.first_peer_id.to_string()), entry.first_heard_ms, enode)
            }
            None => (None, now_ms, None),
        };
        rows.push(SealedTxFactInsert {
            sealed_block_number: block_num,
            tx_hash: format!("{hash:#x}"),
            first_peer_id,
            first_heard_ms,
            effective_tip_wei_hex: encode_u128_hex_quantity(*tip),
            tip_formula_version: 1,
            first_enode,
        });
    }
    rows
}

/// Encode a `u128` as the ethereum-spec "Quantity" JSON wire representation: lowercase,
/// `0x`-prefixed, minimal hex, `"0x0"` for zero. Stored directly in SQLite so export
/// does zero re-encoding (brief §5.5).
pub fn encode_u128_hex_quantity(value: u128) -> String {
    if value == 0 {
        return "0x0".to_string();
    }
    format!("0x{value:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::{EthereumTxEnvelope, Signed, Transaction as _, TxEip1559, TxType};
    use alloy_primitives::{B256, Bytes, ChainId, Sealed, Signature, TxKind, U256};
    use alloy_rlp::Decodable;
    use reth_primitives_traits::transaction::TxHashRef as _;
    use tempfile::NamedTempFile;

    use crate::transaction::{BerachainTxEnvelope, BerachainTxType, PoLTx};
    use reth_ethereum_primitives::Receipt;

    // ---- TP-4 (BERA-305): schema migration adds `first_enode` column ----
    /// Confirms `ensure_sealed_tx_fact_schema` upgrades a pre-migration
    /// `sealed_tx_fact` table (without the `first_enode` column) by adding the column,
    /// preserving existing rows with `first_enode = NULL`, and that fresh INSERTs with a
    /// populated `first_enode` round-trip through `export_sealed_tx_facts`.
    #[test]
    fn sealed_tx_fact_schema_migration_adds_first_enode() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let tmp = NamedTempFile::new().unwrap();

        // Phase 1: create a DB with the *pre-migration* schema (no first_enode column).
        let conn = Connection::open(tmp.path()).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute_batch(
            "CREATE TABLE sealed_tx_fact (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                sealed_block_number  INTEGER NOT NULL,
                tx_hash              TEXT    NOT NULL,
                first_peer_id        TEXT    NULL,
                first_heard_ms       INTEGER NOT NULL,
                effective_tip_wei    TEXT    NOT NULL,
                tip_formula_version  INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX idx_sealed_tx_fact_first_heard_ms
                ON sealed_tx_fact(first_heard_ms);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sealed_tx_fact \
                (sealed_block_number, tx_hash, first_peer_id, first_heard_ms, \
                 effective_tip_wei, tip_formula_version) \
              VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![1_i64, "0xfeed", Option::<String>::None, 100_i64, "0x0", 1_i64,],
        )
        .unwrap();

        // Sanity: pre-migration column set has no first_enode.
        let pre_has: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('sealed_tx_fact') WHERE name = 'first_enode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!pre_has, "test setup expects pre-migration schema");
        drop(conn);

        // Phase 2: open the DB through PogSqliteStore::open, which runs the migration.
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        // (a) migration succeeds: open returned Ok above.
        // (b) first_enode column exists.
        let post_has: bool = store
            .lock_read()
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('sealed_tx_fact') WHERE name = 'first_enode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(post_has, "migration must add the first_enode column");

        // (c) pre-existing row preserved with first_enode = NULL.
        let exported = store.export_sealed_tx_facts(0, 100).unwrap();
        assert_eq!(exported.rows.len(), 1);
        assert!(exported.rows[0].first_enode.is_none(), "legacy row must round-trip with NULL");

        // (d) fresh INSERT with a populated first_enode round-trips.
        let peer = PeerId::random();
        let listening_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 30303);
        let expected_enode = NodeRecord::new(listening_addr, peer).to_string();
        let insert = SealedTxFactInsert {
            sealed_block_number: 99,
            tx_hash: "0xbeef".to_string(),
            first_peer_id: Some(peer.to_string()),
            first_heard_ms: 200,
            effective_tip_wei_hex: "0x0".to_string(),
            tip_formula_version: 1,
            first_enode: Some(expected_enode.clone()),
        };
        store.flush_sealed_tx_facts(&[insert], 0).unwrap();
        let exported = store.export_sealed_tx_facts(0, 100).unwrap();
        let new_row =
            exported.rows.iter().find(|r| r.tx_hash == "0xbeef").expect("fresh row exported");
        assert_eq!(
            new_row.first_enode.as_deref(),
            Some(expected_enode.as_str()),
            "fresh INSERT with populated first_enode must round-trip through export",
        );
    }

    // ---- TP-5 (BERA-305): PogTxProvenanceSink persists first_enode round trip ----
    /// Drives the in-RAM `InflightTransactions` → `build_sealed_tx_fact_inserts` pipeline
    /// directly (the same pipeline `PogTxProvenanceSink::record_accepted_from_peer` feeds)
    /// and asserts that:
    ///   - a `Some(addr)` listening_addr produces `first_enode == NodeRecord::new(addr,
    ///     peer_id).to_string()` on the persisted row;
    ///   - a `None` listening_addr (Hello.port=0) produces no inflight row → seal flush is
    ///     non-p2p (`first_peer_id` and `first_enode` both unset).
    /// Routing through `flush_sealed_tx_facts` plus `export_sealed_tx_facts` exercises the
    /// SQLite round-trip end-to-end.
    #[test]
    fn pog_sink_persists_first_enode_round_trip() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();

        // Two parallel scenarios in one DB: peer A with listening_addr; peer B without.
        let mut inflight = InflightTransactions::new(Duration::from_secs(60), 1024);
        let peer_a = PeerId::random();
        let listening = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 30303);
        let tx_a = B256::random();
        inflight.record_first_hear(tx_a, peer_a, Some(listening), 1_000);

        let peer_b = PeerId::random();
        let tx_b = B256::random();
        inflight.record_first_hear(tx_b, peer_b, None, 2_000);

        let drained = inflight.drain_for_seal(&[tx_a, tx_b]);
        let rows = build_sealed_tx_fact_inserts(99, &drained, &[100, 200], 9_999);
        assert_eq!(rows.len(), 2);
        let row_a = rows.iter().find(|r| r.tx_hash == format!("{tx_a:#x}")).unwrap();
        let row_b = rows.iter().find(|r| r.tx_hash == format!("{tx_b:#x}")).unwrap();
        let expected_enode = NodeRecord::new(listening, peer_a).to_string();
        assert_eq!(row_a.first_enode.as_deref(), Some(expected_enode.as_str()));
        assert!(
            row_b.first_peer_id.is_none() && row_b.first_enode.is_none(),
            "Hello.port=0 first-hear must not create a p2p attribution row",
        );

        store.flush_sealed_tx_facts(&rows, 0).unwrap();
        let exported = store.export_sealed_tx_facts(0, 100).unwrap();
        assert_eq!(exported.rows.len(), 2);
        let exported_a = exported
            .rows
            .iter()
            .find(|r| r.tx_hash == format!("{tx_a:#x}"))
            .expect("populated row exported");
        let exported_b = exported
            .rows
            .iter()
            .find(|r| r.tx_hash == format!("{tx_b:#x}"))
            .expect("non-p2p row exported");
        assert_eq!(
            exported_a.first_enode.as_deref(),
            Some(expected_enode.as_str()),
            "Some(listening_addr) must round-trip as canonical enode URL",
        );
        assert!(
            exported_b.first_peer_id.is_none() && exported_b.first_enode.is_none(),
            "skipped first-hear must export as non-p2p (NULL peer + NULL enode)",
        );
    }

    // ---- BERA-325: seal-flush excludes PoL / system txs ----
    fn ber325_test_sig() -> Signature {
        Signature::new(U256::from(1u64), U256::from(2u64), false)
    }

    fn ber325_eip1559(
        distinct_nonce: u64,
        gas_limit: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
    ) -> BerachainTxEnvelope {
        let tx = TxEip1559 {
            chain_id: ChainId::from(1u64),
            nonce: distinct_nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            to: TxKind::Call(Address::from([0xcd; 20])),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Bytes::default(),
        };
        let signed = Signed::new_unhashed(tx, ber325_test_sig());
        BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Eip1559(signed))
    }

    fn ber325_pol(nonce: u64) -> BerachainTxEnvelope {
        BerachainTxEnvelope::Berachain(Sealed::new(PoLTx {
            chain_id: ChainId::from(80084u64),
            from: Address::ZERO,
            to: Address::from([0x11; 20]),
            nonce,
            gas_limit: 30_000_000,
            gas_price: 1u128,
            input: Bytes::copy_from_slice(&nonce.to_le_bytes()),
        }))
    }

    fn ber325_receipt(cum: u64) -> Receipt<BerachainTxType> {
        Receipt {
            tx_type: BerachainTxType::Ethereum(TxType::Eip1559),
            success: true,
            cumulative_gas_used: cum,
            logs: vec![],
        }
    }

    /// TP-1 (BERA-325): one EIP-1559 tx + one PoL → exactly one `sealed_tx_fact` row.
    #[test]
    fn seal_flush_skips_pol_system_tx() {
        let base_fee = 1_000u128;
        let eth = ber325_eip1559(0, 21_000, 10_000, 2_000);
        let pol = ber325_pol(7);
        let h_eth = *eth.tx_hash();

        let txs = vec![eth, pol];
        let tx_refs: Vec<&BerachainTxEnvelope> = txs.iter().collect();
        let receipts = vec![ber325_receipt(21_000), ber325_receipt(21_000 + 500_000)];

        let (hashes, tips, skipped) =
            collect_seal_flush_tx_hashes_and_tips(base_fee, &tx_refs, &receipts).unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(hashes, vec![h_eth]);
        assert_eq!(tips.len(), 1);

        let drained = vec![(h_eth, None)];
        let rows = build_sealed_tx_fact_inserts(1, &drained, &tips, 99);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tx_hash, format!("{h_eth:#x}"));
    }

    /// TP-2 (BERA-325): receipt alignment for surviving txs when PoL rows sit between them.
    #[test]
    fn seal_flush_filter_preserves_receipt_alignment() {
        let base_fee = 1_000u128;
        let tx_a = ber325_eip1559(1, 100, 10_000, 2_000);
        let tx_b = ber325_pol(2);
        let tx_c = ber325_eip1559(3, 50, 50_000, 10_000);
        let tx_d = ber325_pol(4);

        let h_a = *tx_a.tx_hash();
        let h_c = *tx_c.tx_hash();

        let txs = vec![tx_a, tx_b, tx_c, tx_d];
        let tx_refs: Vec<&BerachainTxEnvelope> = txs.iter().collect();
        let receipts = vec![
            ber325_receipt(100),
            ber325_receipt(300),
            ber325_receipt(350),
            ber325_receipt(650),
        ];

        let (hashes, tips, skipped) =
            collect_seal_flush_tx_hashes_and_tips(base_fee, &tx_refs, &receipts).unwrap();
        assert_eq!(skipped, 2);
        assert_eq!(hashes, vec![h_a, h_c]);

        let eff_a = 3_000u128;
        assert_eq!(tips[0], (eff_a - base_fee) * 100u128);

        let eff_c = 11_000u128;
        assert_eq!(tips[1], (eff_c - base_fee) * 50u128);
    }

    /// TP-3 (BERA-325): two PoL txs in the fixture → `system_tx` skip count is 2 (drives
    /// `pog_sealed_flush_tx_skipped_total{reason="system_tx"}` in production).
    #[test]
    fn seal_flush_skip_counter_increments_per_filtered_tx() {
        let base_fee = 1_000u128;
        let txs = vec![
            ber325_eip1559(1, 100, 10_000, 2_000),
            ber325_pol(2),
            ber325_eip1559(3, 50, 50_000, 10_000),
            ber325_pol(4),
        ];
        let tx_refs: Vec<&BerachainTxEnvelope> = txs.iter().collect();
        let receipts = vec![
            ber325_receipt(100),
            ber325_receipt(300),
            ber325_receipt(350),
            ber325_receipt(650),
        ];
        let (_, _, skipped) =
            collect_seal_flush_tx_hashes_and_tips(base_fee, &tx_refs, &receipts).unwrap();
        assert_eq!(skipped, 2);
    }

    /// TP-4 (BERA-325): Ethereum-only blocks match the legacy tip vector.
    #[test]
    fn seal_flush_eth_only_matches_unfiltered_tip_vector() {
        let base_fee = 1_000u128;
        let t1 = ber325_eip1559(0, 100, 5_000, 2_000);
        let t2 = ber325_eip1559(1, 50, 8_000, 5_000);
        let txs = vec![t1, t2];
        let tx_refs: Vec<&BerachainTxEnvelope> = txs.iter().collect();
        let receipts = vec![ber325_receipt(100), ber325_receipt(150)];

        let eff: Vec<u128> =
            txs.iter().map(|t| t.effective_gas_price(Some(base_fee as u64))).collect();
        let legacy = compute_effective_tips_from_receipts(base_fee, &eff, &receipts);
        let (_, tips, skipped) =
            collect_seal_flush_tx_hashes_and_tips(base_fee, &tx_refs, &receipts).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(tips, legacy);
    }

    /// TP-5 (BERA-325): PoL-only block → no rows; empty `flush_sealed_tx_facts` still runs retention.
    #[test]
    fn seal_flush_all_pol_block_writes_zero_rows_and_runs_retention() {
        let base_fee = 1u128;
        let txs = vec![ber325_pol(10), ber325_pol(11), ber325_pol(12)];
        let tx_refs: Vec<&BerachainTxEnvelope> = txs.iter().collect();
        let receipts = vec![ber325_receipt(5), ber325_receipt(8), ber325_receipt(13)];

        let (hashes, tips, skipped) =
            collect_seal_flush_tx_hashes_and_tips(base_fee, &tx_refs, &receipts).unwrap();
        assert!(hashes.is_empty() && tips.is_empty());
        assert_eq!(skipped, 3);

        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let stale = SealedTxFactInsert {
            sealed_block_number: 1,
            tx_hash: format!("{:#x}", B256::random()),
            first_peer_id: None,
            first_heard_ms: 100,
            effective_tip_wei_hex: "0x0".into(),
            tip_formula_version: 1,
            first_enode: None,
        };
        store.flush_sealed_tx_facts(&[stale], u64::MAX).unwrap();
        assert_eq!(store.sealed_tx_fact_row_count(), 1);

        let (_hw, deleted) = store.flush_sealed_tx_facts(&[], 5_000).unwrap();
        assert_eq!(deleted, 1, "retention DELETE must run on empty insert batch");
        assert_eq!(store.sealed_tx_fact_row_count(), 0);
    }

    // ---- TP-R1 ----
    #[test]
    fn tp_r1_record_first_hear_first_seen_wins() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut w = InflightTransactions::new(Duration::from_secs(60), 1024);
        let tx = B256::random();
        let p1 = PeerId::random();
        let p2 = PeerId::random();
        let a1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), 30303);
        let a2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2)), 30303);
        assert!(w.record_first_hear(tx, p1, Some(a1), 1_000));
        assert!(w.record_first_hear(tx, p2, Some(a2), 2_000));
        let drained = w.drain_for_seal(&[tx]);
        let (_, entry) = drained.into_iter().next().unwrap();
        let e = entry.expect("entry present");
        assert_eq!(e.first_peer_id, p1);
        assert_eq!(e.first_heard_ms, 1_000);
    }

    // ---- TP-R2 ----
    #[test]
    fn tp_r2_drain_for_seal_extracts_and_removes() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut w = InflightTransactions::new(Duration::from_secs(60), 1024);
        let a = B256::random();
        let b = B256::random();
        let c = B256::random();
        let peer = PeerId::random();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 30303);
        w.record_first_hear(a, peer, Some(addr), 10);
        w.record_first_hear(b, peer, Some(addr), 20);
        let drained = w.drain_for_seal(&[a, b, c]);
        assert_eq!(drained.len(), 3);
        assert!(drained.iter().find(|(h, _)| *h == a).unwrap().1.is_some());
        assert!(drained.iter().find(|(h, _)| *h == b).unwrap().1.is_some());
        assert!(drained.iter().find(|(h, _)| *h == c).unwrap().1.is_none());
        // After drain, the map is empty (drain removes)
        assert_eq!(w.len(), 0);
    }

    // ---- TP-R3 ----
    #[test]
    fn tp_r3_ttl_eviction_removes_stale_entries() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut w = InflightTransactions::new(Duration::from_millis(1), 1024);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)), 30303);
        for _ in 0..5 {
            w.record_first_hear(B256::random(), PeerId::random(), Some(addr), 0);
        }
        assert_eq!(w.len(), 5);
        std::thread::sleep(Duration::from_millis(5));
        w.evict_expired();
        assert_eq!(w.len(), 0);
    }

    // ---- TP-R4 ----
    #[test]
    fn tp_r4_cap_rejection_and_inline_sweep() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 3)), 30303);
        let mut w = InflightTransactions::new(Duration::from_millis(1), 2);
        let peer = PeerId::random();
        assert!(w.record_first_hear(B256::random(), peer, Some(addr), 0));
        assert!(w.record_first_hear(B256::random(), peer, Some(addr), 0));
        // At cap; let the TTL expire so the inline sweep clears the map.
        std::thread::sleep(Duration::from_millis(5));
        // Next insert triggers inline sweep and succeeds.
        assert!(w.record_first_hear(B256::random(), peer, Some(addr), 0));
        assert_eq!(w.cap_rejections(), 0);

        // Now fill cap with FRESH entries (TTL=1h so sweep can't clear them).
        let mut w = InflightTransactions::new(Duration::from_secs(3600), 2);
        assert!(w.record_first_hear(B256::random(), peer, Some(addr), 0));
        assert!(w.record_first_hear(B256::random(), peer, Some(addr), 0));
        assert!(!w.record_first_hear(B256::random(), peer, Some(addr), 0));
        assert_eq!(w.cap_rejections(), 1);
    }

    // ---- TP-R5 ----
    #[test]
    fn tp_r5_encode_u128_hex_quantity_roundtrip() {
        assert_eq!(encode_u128_hex_quantity(0), "0x0");
        assert_eq!(encode_u128_hex_quantity(1), "0x1");
        assert_eq!(encode_u128_hex_quantity(0x10), "0x10");
        let big: u128 = (u64::MAX as u128) * 20;
        assert_eq!(encode_u128_hex_quantity(big), format!("0x{big:x}"));
    }

    // ---- TP-R6 ----
    #[test]
    fn tp_r6_retention_delete_via_first_heard_ms_index() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let rows = vec![
            SealedTxFactInsert {
                sealed_block_number: 1,
                tx_hash: format!("{:#x}", B256::random()),
                first_peer_id: None,
                first_heard_ms: 100,
                effective_tip_wei_hex: "0x0".to_string(),
                tip_formula_version: 1,
                first_enode: None,
            },
            SealedTxFactInsert {
                sealed_block_number: 2,
                tx_hash: format!("{:#x}", B256::random()),
                first_peer_id: None,
                first_heard_ms: 10_000,
                effective_tip_wei_hex: "0x0".to_string(),
                tip_formula_version: 1,
                first_enode: None,
            },
        ];
        let (_hw, _del) = store.flush_sealed_tx_facts(&rows, 0).unwrap();
        assert_eq!(store.sealed_tx_fact_row_count(), 2);

        // Retention DELETE uses the `idx_sealed_tx_fact_first_heard_ms` index.
        let (_hw, deleted) = store.flush_sealed_tx_facts(&[], 5_000).unwrap();
        assert_eq!(deleted, 1, "only the row with first_heard_ms < 5000 should be deleted");
        assert_eq!(store.sealed_tx_fact_row_count(), 1);
    }

    // ---- TP-R7 ----
    #[test]
    fn tp_r7_export_wire_shape_and_cursor_semantics() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let peer = PeerId::random();
        let peer_hex = peer.to_string();
        let h1 = B256::random();
        let rows = vec![SealedTxFactInsert {
            sealed_block_number: 42,
            tx_hash: format!("{h1:#x}"),
            first_peer_id: Some(peer_hex.clone()),
            first_heard_ms: 1_713_876_543_000,
            effective_tip_wei_hex: "0x1bc16d674ec80000".to_string(),
            tip_formula_version: 1,
            first_enode: None,
        }];
        store.flush_sealed_tx_facts(&rows, 0).unwrap();

        let out = store.export_sealed_tx_facts(0, 10).unwrap();
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].sealed_block_number, 42);
        assert_eq!(out.rows[0].first_peer_id.as_deref(), Some(peer_hex.as_str()));
        assert_eq!(out.rows[0].effective_tip_wei, "0x1bc16d674ec80000");
        assert_eq!(out.high_water_id, 1);
        assert_eq!(out.min_retained_id, 1);
        assert!(!out.truncated);

        // Cursor past high_water returns zero rows but preserves after_id.
        let after = store.export_sealed_tx_facts(out.high_water_id, 10).unwrap();
        assert!(after.rows.is_empty());
        assert_eq!(after.next_after_id, out.high_water_id);
        assert!(!after.truncated);
    }

    // ---- TP-R8 ----
    #[test]
    fn tp_r8_export_never_acquires_write_conn() {
        let tmp = NamedTempFile::new().unwrap();
        let store = std::sync::Arc::new(PogSqliteStore::open(tmp.path()).unwrap());
        let rows: Vec<_> = (0..20)
            .map(|i| SealedTxFactInsert {
                sealed_block_number: i,
                tx_hash: format!("0x{:064x}", i),
                first_peer_id: None,
                first_heard_ms: 1_000 + i,
                effective_tip_wei_hex: "0x0".to_string(),
                tip_formula_version: 1,
                first_enode: None,
            })
            .collect();
        store.flush_sealed_tx_facts(&rows, 0).unwrap();

        // Baseline the write-conn counter before the concurrent window.
        let writes_before = store.write_conn_lock_count();
        // Spawn a parallel writer + reader; run several iterations each.
        let s1 = store.clone();
        let s2 = store.clone();
        let writer = std::thread::spawn(move || {
            for i in 1000..1050 {
                let rs = vec![SealedTxFactInsert {
                    sealed_block_number: i,
                    tx_hash: format!("0x{:064x}", i),
                    first_peer_id: None,
                    first_heard_ms: 1_000_000 + i,
                    effective_tip_wei_hex: "0x0".to_string(),
                    tip_formula_version: 1,
                    first_enode: None,
                }];
                s1.flush_sealed_tx_facts(&rs, 0).unwrap();
            }
        });
        let reader = std::thread::spawn(move || {
            for _ in 0..100 {
                let _ = s2.export_sealed_tx_facts(0, 10).unwrap();
            }
        });
        writer.join().unwrap();
        reader.join().unwrap();

        let writes_after = store.write_conn_lock_count();
        let reads_after = store.read_conn_lock_count();
        // Exporter never touches write_conn, only read_conn:
        // write_conn_lock_count should advance only from the writer thread's flushes.
        let writes_delta = writes_after - writes_before;
        assert_eq!(writes_delta, 50, "only seal-flush acquires write_conn");
        assert!(reads_after >= 100, "reader acquired read_conn at least once per export");
    }

    // ---- TP-R9 ----
    #[test]
    fn tp_r9_metrics_path_zero_sql_on_scrape() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let rows = vec![SealedTxFactInsert {
            sealed_block_number: 7,
            tx_hash: "0xdeadbeef".into(),
            first_peer_id: None,
            first_heard_ms: 500,
            effective_tip_wei_hex: "0x1".into(),
            tip_formula_version: 1,
            first_enode: None,
        }];
        let before_flushed = store.sealed_facts_flushed_total();
        store.flush_sealed_tx_facts(&rows, 0).unwrap();
        assert_eq!(store.sealed_facts_flushed_total(), before_flushed + 1);

        // Baseline connection-lock counts, then invoke the "scrape" path: reading every
        // metric atomic. Assert no SQL (i.e., no mutex acquisitions on either connection).
        let w_before = store.write_conn_lock_count();
        let r_before = store.read_conn_lock_count();
        let _snapshot = (
            store.sealed_tx_fact_row_count(),
            store.sealed_tx_fact_high_water_id(),
            store.sealed_tx_fact_min_retained_id(),
            store.sealed_facts_flushed_total(),
            store.sealed_facts_flushed_with_peer_total(),
            store.sealed_facts_retention_deleted_total(),
            store.sealed_facts_export_rows_total(),
        );
        assert_eq!(store.write_conn_lock_count(), w_before, "scrape must not touch write_conn");
        assert_eq!(store.read_conn_lock_count(), r_before, "scrape must not touch read_conn");
    }

    // ---- TP-1 / AC-3 ----
    // Verify that since the LocallyBuiltBlocks gate is removed, any block commit
    // (regardless of local proposal) invokes the seal-flush path.
    #[test]
    fn tp_r10_unconditional_seal_flush_on_canon_commit() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut inflight = InflightTransactions::new(Duration::from_secs(60), 1024);
        let peer = PeerId::random();
        let tx_a = B256::random();
        let tx_b = B256::random();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4)), 30303);
        inflight.record_first_hear(tx_a, peer, Some(addr), 100);
        inflight.record_first_hear(tx_b, peer, Some(addr), 200);
        
        assert_eq!(inflight.len(), 2);
        assert!(inflight.contains(&tx_a));
        assert!(inflight.contains(&tx_b));

        // When we drain for seal, both transactions are successfully processed,
        // regardless of which block proposed them, because we execute seal-flush unconditionally.
        let drained = inflight.drain_for_seal(&[tx_a, tx_b]);
        assert_eq!(drained.len(), 2);
        assert_eq!(inflight.len(), 0);
    }

    // ---- InflightTransactions: retained value structure ----
    #[test]
    fn inflight_reads_the_correct_peer_after_first_hear() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut w = InflightTransactions::new(Duration::from_secs(60), 1024);
        let tx = B256::random();
        let peer = PeerId::random();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)), 30303);
        w.record_first_hear(tx, peer, Some(addr), 42);
        let drained = w.drain_for_seal(&[tx]);
        let entry = drained[0].1.unwrap();
        assert_eq!(entry.first_peer_id, peer);
        assert_eq!(entry.first_heard_ms, 42);
    }

    #[test]
    fn tp_r1b_record_first_hear_skips_none_listening_then_some_inserts() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        let mut w = InflightTransactions::new(Duration::from_secs(60), 1024);
        let tx = B256::random();
        let p_skip = PeerId::random();
        let p_win = PeerId::random();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)), 30303);
        assert!(w.record_first_hear(tx, p_skip, None, 1));
        assert!(w.record_first_hear(tx, p_win, Some(addr), 2));
        let drained = w.drain_for_seal(&[tx]);
        let e = drained[0].1.expect("second hear with listening addr must insert");
        assert_eq!(e.first_peer_id, p_win);
        assert_eq!(e.first_heard_ms, 2);
    }


    // ---- PogSqliteStore probe API (inherited from PogDb) ----
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
        PogSqliteStore::open(tmp.path()).unwrap();
        PogSqliteStore::open(tmp.path()).unwrap();
    }

    #[test]
    fn pog_store_all_peer_statuses_batch_query() {
        let tmp = NamedTempFile::new().unwrap();
        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let p1 = PeerId::random();
        let p2 = PeerId::random();
        let tx1 = B256::random();
        let tx2 = B256::random();
        let tx3 = B256::random();
        let tx4 = B256::random();
        store.insert_peer_test(&p1, tx1, "timeout").unwrap();
        store.insert_peer_test(&p1, tx2, "timeout").unwrap();
        store.insert_peer_test(&p1, tx3, "seen").unwrap();
        store.insert_peer_test(&p2, tx4, "timeout").unwrap();

        let statuses = store.all_peer_statuses().unwrap();
        assert_eq!(statuses.len(), 2);
        let s1 = &statuses[&p1.to_string()];
        assert_eq!(s1.last_result, "seen");
        assert_eq!(s1.failure_count, 2);
        assert_eq!(s1.last_tx_hash, tx3.to_string());
        let s2 = &statuses[&p2.to_string()];
        assert_eq!(s2.last_result, "timeout");
        assert_eq!(s2.failure_count, 1);
        assert_eq!(s2.last_tx_hash, tx4.to_string());
    }

    #[test]
    fn pog_store_migrates_legacy_peer_tests() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute_batch(
            "CREATE TABLE peer_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                peer_id TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                result TEXT NOT NULL,
                tested_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        let p1 = PeerId::random();
        let tx = B256::random();
        conn.execute(
            "INSERT INTO peer_tests (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![p1.to_string(), tx.to_string(), "seen", 100_i64],
        )
        .unwrap();
        drop(conn);

        let store = PogSqliteStore::open(tmp.path()).unwrap();
        let statuses = store.all_peer_statuses().unwrap();
        assert_eq!(statuses.len(), 1);
        let s = &statuses[&p1.to_string()];
        assert_eq!(s.last_result, "seen");
        assert_eq!(s.last_tx_hash, tx.to_string());
    }

    #[test]
    fn integrity_check_corrupt_renames_and_boots_clean() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp); // release file so we can control its contents
        std::fs::write(&path, b"this is not a sqlite database at all").unwrap();

        // Open should succeed by renaming the corrupt file and creating a fresh DB.
        let store = PogSqliteStore::open(&path).unwrap();
        assert_eq!(store.sealed_tx_fact_row_count(), 0);

        // A sibling file with ".corrupt." in its name must exist next to the DB.
        let parent = path.parent().unwrap();
        let file_stem = path.file_name().unwrap().to_string_lossy().to_string();
        let corrupt = std::fs::read_dir(parent).unwrap().filter_map(|e| e.ok()).any(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with(&file_stem) && name.contains(".corrupt.")
        });
        assert!(corrupt, "expected a renamed .corrupt. sibling file");

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
