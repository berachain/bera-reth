//! known-peers.json curation using durable PoG evidence.

use reth_network_peers::NodeRecord;
use rusqlite::Connection;
use std::{collections::HashSet, fs, io, path::Path};
use tracing::{info, warn};

const CURATION_LOG_TARGET: &str = "bera_reth::pog_peer_curation";

/// Outcome of a shutdown curation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerCurationOutcome {
    Curated { retained: usize, removed: usize },
    NoOp(NoOpReason),
}

/// Why curation was a no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoOpReason {
    KnownPeersFileMissing,
    PogDbMissing,
    NoProbeHistory,
    Failed,
}

/// Curate the reth persisted peers file using durable PoG evidence.
pub fn curate_known_peers_file(known_peers_path: &Path, pog_db_path: &Path) -> PeerCurationOutcome {
    match try_curate_known_peers_file(known_peers_path, pog_db_path) {
        Ok(outcome) => {
            log_outcome(&outcome, known_peers_path, pog_db_path);
            outcome
        }
        Err(err) => {
            warn!(
                target: CURATION_LOG_TARGET,
                peers_file = ?known_peers_path,
                db_file = ?pog_db_path,
                error = %err,
                "Failed to curate known-peers.json from PoG evidence; leaving file unchanged"
            );
            PeerCurationOutcome::NoOp(NoOpReason::Failed)
        }
    }
}

fn try_curate_known_peers_file(
    known_peers_path: &Path,
    pog_db_path: &Path,
) -> Result<PeerCurationOutcome, PeerCurationError> {
    if !known_peers_path.is_file() {
        return Ok(PeerCurationOutcome::NoOp(NoOpReason::KnownPeersFileMissing));
    }
    if !pog_db_path.is_file() {
        return Ok(PeerCurationOutcome::NoOp(NoOpReason::PogDbMissing));
    }

    let conn = Connection::open(pog_db_path)?;
    crate::pog::ensure_peer_tests_schema(&conn)?;

    let probe_history_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM peer_pog_status", [], |row| row.get(0))?;
    if probe_history_rows == 0 {
        return Ok(PeerCurationOutcome::NoOp(NoOpReason::NoProbeHistory));
    }

    let able_to_relay = load_able_to_relay_peer_ids(&conn)?;

    let original = fs::read(known_peers_path)?;
    let mut known_peers: Vec<NodeRecord> = serde_json::from_slice(&original)?;
    let total = known_peers.len();
    known_peers.retain(|record| able_to_relay.contains(&record.id.to_string()));
    let retained = known_peers.len();
    let removed = total.saturating_sub(retained);

    write_curated_known_peers(known_peers_path, &known_peers)?;

    Ok(PeerCurationOutcome::Curated { retained, removed })
}

fn load_able_to_relay_peer_ids(conn: &Connection) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT peer_id FROM peer_pog_status WHERE success_count > 0
         UNION
         SELECT DISTINCT peer_id FROM peer_pog_log WHERE result = 'seen'",
    )?;

    let mut rows = stmt.query([])?;
    let mut peer_ids = HashSet::new();
    while let Some(row) = rows.next()? {
        let peer_id: String = row.get(0)?;
        peer_ids.insert(peer_id);
    }

    Ok(peer_ids)
}

fn write_curated_known_peers(path: &Path, peers: &[NodeRecord]) -> Result<(), PeerCurationError> {
    let encoded = serde_json::to_vec(peers)?;
    let mut tmp_path = path.to_path_buf();
    let tmp_extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{ext}.pog-curation-tmp"))
        .unwrap_or_else(|| "pog-curation-tmp".to_string());
    tmp_path.set_extension(tmp_extension);

    fs::write(&tmp_path, encoded)?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(PeerCurationError::Io(err));
    }
    Ok(())
}

fn log_outcome(outcome: &PeerCurationOutcome, known_peers_path: &Path, pog_db_path: &Path) {
    match outcome {
        PeerCurationOutcome::Curated { retained, removed } => info!(
            target: CURATION_LOG_TARGET,
            peers_file = ?known_peers_path,
            retained = *retained,
            removed = *removed,
            "Curated known-peers.json from PoG evidence"
        ),
        PeerCurationOutcome::NoOp(NoOpReason::NoProbeHistory | NoOpReason::PogDbMissing) => info!(
            target: CURATION_LOG_TARGET,
            peers_file = ?known_peers_path,
            db_file = ?pog_db_path,
            "Skipping known-peers.json curation; no PoG probe history"
        ),
        PeerCurationOutcome::NoOp(NoOpReason::KnownPeersFileMissing) => info!(
            target: CURATION_LOG_TARGET,
            peers_file = ?known_peers_path,
            "Skipping known-peers.json curation; file not found"
        ),
        PeerCurationOutcome::NoOp(NoOpReason::Failed) => {}
    }
}

#[derive(Debug, thiserror::Error)]
enum PeerCurationError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pog::ensure_peer_tests_schema;
    use alloy_primitives::B256;
    use reth_network_peers::{NodeRecord, PeerId};
    use rusqlite::{Connection, params};
    use std::{
        collections::HashSet,
        fs,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
    };
    use tempfile::tempdir;

    fn make_peer(index: u8) -> NodeRecord {
        let id = PeerId::random();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, index));
        NodeRecord::new_with_ports(ip, 30300 + u16::from(index), Some(30300 + u16::from(index)), id)
    }

    fn write_known_peers(path: &Path, peers: &[NodeRecord]) {
        let encoded = serde_json::to_vec(peers).expect("serialize peers");
        fs::write(path, encoded).expect("write known-peers");
    }

    fn read_known_peers(path: &Path) -> Vec<NodeRecord> {
        let bytes = fs::read(path).expect("read known-peers");
        serde_json::from_slice(&bytes).expect("parse known-peers")
    }

    fn seed_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).expect("open sqlite");
        ensure_peer_tests_schema(&conn).expect("ensure schema");
        conn
    }

    fn insert_status_row(conn: &Connection, peer_id: &PeerId, success_count: i64, failure_count: i64) {
        conn.execute(
            "INSERT INTO peer_pog_status (peer_id, last_result, last_tx_hash, last_tested_at, failure_count, success_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                peer_id.to_string(),
                if success_count > 0 { "seen" } else { "timeout" },
                B256::random().to_string(),
                1_i64,
                failure_count,
                success_count
            ],
        )
        .expect("insert status row");
    }

    fn insert_log_row(conn: &Connection, peer_id: &PeerId, result: &str) {
        conn.execute(
            "INSERT INTO peer_pog_log (peer_id, tx_hash, result, tested_at) VALUES (?1, ?2, ?3, ?4)",
            params![peer_id.to_string(), B256::random().to_string(), result, 1_i64],
        )
        .expect("insert log row");
    }

    fn ids(peers: &[NodeRecord]) -> HashSet<PeerId> {
        peers.iter().map(|peer| peer.id).collect()
    }

    fn fixture_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let known_peers_path = dir.path().join("known-peers.json");
        let db_path = dir.path().join("proof_of_gossip.db");
        (dir, known_peers_path, db_path)
    }

    #[test]
    fn tp1_allowlist_intersection_keeps_only_seen_subset() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peers: Vec<NodeRecord> = (1..=10).map(make_peer).collect();
        write_known_peers(&known_peers_path, &peers);

        let conn = seed_db(&db_path);
        for peer in peers.iter().take(4) {
            insert_status_row(&conn, &peer.id, 1, 0);
            insert_log_row(&conn, &peer.id, "seen");
        }
        for peer in peers.iter().skip(4) {
            insert_status_row(&conn, &peer.id, 0, 1);
            insert_log_row(&conn, &peer.id, "timeout");
        }

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 4, removed: 6 });
        assert_eq!(read_known_peers(&known_peers_path).len(), 4);
    }

    #[test]
    fn tp2_seen_evidence_wins_over_timeout_history() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer = make_peer(1);
        write_known_peers(&known_peers_path, &[peer]);
        let conn = seed_db(&db_path);
        insert_status_row(&conn, &peer.id, 1, 1);
        insert_log_row(&conn, &peer.id, "timeout");
        insert_log_row(&conn, &peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 1, removed: 0 });
        assert_eq!(read_known_peers(&known_peers_path), vec![peer]);
    }

    #[test]
    fn tp3_timeout_only_peer_is_removed() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer = make_peer(1);
        write_known_peers(&known_peers_path, &[peer]);
        let conn = seed_db(&db_path);
        insert_status_row(&conn, &peer.id, 0, 3);
        insert_log_row(&conn, &peer.id, "timeout");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 0, removed: 1 });
        assert_eq!(read_known_peers(&known_peers_path), Vec::<NodeRecord>::new());
    }

    #[test]
    fn tp4_unprobed_peer_in_file_is_removed() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer_in_file = make_peer(1);
        let peer_in_db = make_peer(2);
        write_known_peers(&known_peers_path, &[peer_in_file]);
        let conn = seed_db(&db_path);
        insert_status_row(&conn, &peer_in_db.id, 1, 0);
        insert_log_row(&conn, &peer_in_db.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 0, removed: 1 });
        assert_eq!(read_known_peers(&known_peers_path), Vec::<NodeRecord>::new());
    }

    #[test]
    fn tp5_non_empty_status_with_no_match_yields_empty_file() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer_in_file = make_peer(1);
        write_known_peers(&known_peers_path, &[peer_in_file]);
        let conn = seed_db(&db_path);
        let timeout_only = make_peer(2);
        insert_status_row(&conn, &timeout_only.id, 0, 1);
        insert_log_row(&conn, &timeout_only.id, "timeout");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 0, removed: 1 });
        assert_eq!(read_known_peers(&known_peers_path), Vec::<NodeRecord>::new());
    }

    #[test]
    fn tp6_seen_for_non_overlapping_peers_yields_empty_intersection() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer_in_file = make_peer(1);
        write_known_peers(&known_peers_path, &[peer_in_file]);
        let conn = seed_db(&db_path);
        let seen_elsewhere = make_peer(2);
        insert_status_row(&conn, &seen_elsewhere.id, 1, 0);
        insert_log_row(&conn, &seen_elsewhere.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 0, removed: 1 });
        assert_eq!(read_known_peers(&known_peers_path), Vec::<NodeRecord>::new());
    }

    #[test]
    fn tp7_empty_status_table_is_no_op_and_file_bytes_are_identical() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peers = vec![make_peer(1), make_peer(2)];
        write_known_peers(&known_peers_path, &peers);
        let before = fs::read(&known_peers_path).expect("read before");
        let _conn = seed_db(&db_path);

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::NoProbeHistory));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }

    #[test]
    fn tp8_missing_db_is_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peers = vec![make_peer(1), make_peer(2)];
        write_known_peers(&known_peers_path, &peers);
        let before = fs::read(&known_peers_path).expect("read before");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::PogDbMissing));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }

    #[test]
    fn tp9_no_trusted_or_static_carve_out_in_file_filtering() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let trusted_like_peer = make_peer(1);
        write_known_peers(&known_peers_path, &[trusted_like_peer]);
        let conn = seed_db(&db_path);
        let unrelated_seen_peer = make_peer(2);
        insert_status_row(&conn, &unrelated_seen_peer.id, 1, 0);
        insert_log_row(&conn, &unrelated_seen_peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 0, removed: 1 });
        assert_eq!(read_known_peers(&known_peers_path), Vec::<NodeRecord>::new());
    }

    #[test]
    fn tp10_mixed_seen_and_timeout_retains_seen_only() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let a = make_peer(1);
        let b = make_peer(2);
        let c = make_peer(3);
        write_known_peers(&known_peers_path, &[a, b, c]);
        let conn = seed_db(&db_path);
        insert_status_row(&conn, &a.id, 1, 0);
        insert_status_row(&conn, &b.id, 1, 0);
        insert_status_row(&conn, &c.id, 0, 2);
        insert_log_row(&conn, &a.id, "seen");
        insert_log_row(&conn, &b.id, "seen");
        insert_log_row(&conn, &c.id, "timeout");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 2, removed: 1 });
        assert_eq!(ids(&read_known_peers(&known_peers_path)), HashSet::from([a.id, b.id]));
    }

    #[test]
    fn tp11_subset_filtering_does_not_require_stable_order() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peers: Vec<NodeRecord> = (1..=6).map(make_peer).collect();
        write_known_peers(&known_peers_path, &peers);
        let conn = seed_db(&db_path);
        for peer in [&peers[1], &peers[4], &peers[5]] {
            insert_status_row(&conn, &peer.id, 1, 0);
            insert_log_row(&conn, &peer.id, "seen");
        }
        insert_status_row(&conn, &peers[0].id, 0, 1);
        insert_status_row(&conn, &peers[2].id, 0, 1);
        insert_status_row(&conn, &peers[3].id, 0, 1);

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 3, removed: 3 });
        assert_eq!(
            ids(&read_known_peers(&known_peers_path)),
            HashSet::from([peers[1].id, peers[4].id, peers[5].id])
        );
    }

    #[test]
    fn tp12_historical_seen_rows_are_sufficient() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peer = make_peer(1);
        write_known_peers(&known_peers_path, &[peer]);
        let conn = seed_db(&db_path);
        insert_status_row(&conn, &peer.id, 1, 4);
        insert_log_row(&conn, &peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::Curated { retained: 1, removed: 0 });
        assert_eq!(read_known_peers(&known_peers_path), vec![peer]);
    }

    #[test]
    fn tp13_no_probes_ever_is_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        write_known_peers(&known_peers_path, &[make_peer(1), make_peer(2)]);
        let _conn = seed_db(&db_path);
        let before = fs::read(&known_peers_path).expect("read before");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::NoProbeHistory));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }

    #[test]
    fn tp14_missing_known_peers_file_is_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let conn = seed_db(&db_path);
        let peer = make_peer(1);
        insert_status_row(&conn, &peer.id, 1, 0);
        insert_log_row(&conn, &peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::KnownPeersFileMissing));
    }

    #[test]
    fn tp15_unexpected_known_peers_schema_is_fail_safe_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        fs::write(&known_peers_path, br#"{"not":"a-list"}"#).expect("write malformed schema");
        let before = fs::read(&known_peers_path).expect("read before");
        let conn = seed_db(&db_path);
        let peer = make_peer(1);
        insert_status_row(&conn, &peer.id, 1, 0);
        insert_log_row(&conn, &peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::Failed));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }

    #[test]
    fn tp16_corrupt_known_peers_json_is_fail_safe_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        fs::write(&known_peers_path, b"[not valid json").expect("write corrupt json");
        let before = fs::read(&known_peers_path).expect("read before");
        let conn = seed_db(&db_path);
        let peer = make_peer(1);
        insert_status_row(&conn, &peer.id, 1, 0);
        insert_log_row(&conn, &peer.id, "seen");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::Failed));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }

    #[test]
    fn tp17_corrupt_db_query_failure_is_fail_safe_no_op() {
        let (_dir, known_peers_path, db_path) = fixture_paths();
        let peers = vec![make_peer(1), make_peer(2)];
        write_known_peers(&known_peers_path, &peers);
        let before = fs::read(&known_peers_path).expect("read before");
        fs::write(&db_path, b"not a sqlite database").expect("write fake db");

        let outcome = curate_known_peers_file(&known_peers_path, &db_path);
        assert_eq!(outcome, PeerCurationOutcome::NoOp(NoOpReason::Failed));
        let after = fs::read(&known_peers_path).expect("read after");
        assert_eq!(before, after);
    }
}
