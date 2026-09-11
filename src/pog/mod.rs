//! Sentry-free Proof of Gossip: RAM send map, land/timeout watcher, `/24` gauges.
//!
//! No SQLite, no node-held signer, no first-hear provenance.

use alloy_primitives::{Address, TxHash};
use reth_network_peers::PeerId;
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static POG_CLI_ENABLED: AtomicBool = AtomicBool::new(false);

/// Set from `main` after parsing CLI.
pub fn set_pog_cli_enabled(enabled: bool) {
    POG_CLI_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn pog_cli_enabled() -> bool {
    POG_CLI_ENABLED.load(Ordering::SeqCst)
}

pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(25);
pub const SEND_ROW_TTL: Duration = Duration::from_secs(900);
pub const WATCHER_TICK: Duration = Duration::from_secs(2);

/// Session `enode://<peerId>@<session-ip>:<session-port>` from the live TCP endpoint.
pub fn session_enode(peer_id: PeerId, remote_addr: SocketAddr) -> String {
    format!("enode://{}@{remote_addr}", alloy_primitives::hex::encode(peer_id.as_slice()))
}

pub fn direction_label(incoming: bool) -> &'static str {
    if incoming { "inbound" } else { "outbound" }
}

pub fn subnet_24(addr: SocketAddr) -> String {
    match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            let o = ip.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        std::net::IpAddr::V6(ip) => {
            let s = ip.segments();
            format!("{:x}:{:x}:{:x}::/48", s[0], s[1], s[2])
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendStatus {
    Sent,
    Landed,
    Timeout,
}

impl SendStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Landed => "landed",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SendRecord {
    pub tx_hash: TxHash,
    pub peer_id: PeerId,
    pub enode: String,
    pub subnet: String,
    pub from: Address,
    pub nonce: u64,
    pub sent_at: Instant,
    pub sent_at_unix: u64,
    pub status: SendStatus,
    pub block_number: Option<u64>,
}

#[derive(Debug, Default)]
struct SendMapInner {
    by_hash: HashMap<TxHash, SendRecord>,
}

pub struct SendMap {
    inner: Mutex<SendMapInner>,
    timeout: Duration,
    ttl: Duration,
}

impl Default for SendMap {
    fn default() -> Self {
        Self::new(DEFAULT_SEND_TIMEOUT, SEND_ROW_TTL)
    }
}

impl SendMap {
    pub fn new(timeout: Duration, ttl: Duration) -> Self {
        Self { inner: Mutex::new(SendMapInner::default()), timeout, ttl }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SendMapInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn inflight_count(&self) -> usize {
        self.lock().by_hash.values().filter(|r| r.status == SendStatus::Sent).count()
    }

    pub fn signer_inflight(&self, from: Address) -> bool {
        self.lock().by_hash.values().any(|r| r.status == SendStatus::Sent && r.from == from)
    }

    pub fn insert(&self, rec: SendRecord) {
        self.lock().by_hash.insert(rec.tx_hash, rec);
    }

    pub fn snapshot(&self) -> Vec<SendRecord> {
        self.lock().by_hash.values().cloned().collect()
    }

    /// Mark matching hashes landed. Returns subnets of newly landed rows.
    pub fn note_block(&self, block_number: u64, hashes: &[TxHash]) -> Vec<String> {
        let mut inner = self.lock();
        let mut landed_subnets = Vec::new();
        for hash in hashes {
            if let Some(rec) = inner.by_hash.get_mut(hash) &&
                rec.status != SendStatus::Landed
            {
                rec.status = SendStatus::Landed;
                rec.block_number = Some(block_number);
                landed_subnets.push(rec.subnet.clone());
            }
        }
        landed_subnets
    }

    /// Expire `sent` → `timeout`. Returns subnets of newly timed-out rows.
    pub fn expire_timeouts(&self) -> Vec<String> {
        let mut inner = self.lock();
        let timeout = self.timeout;
        let mut out = Vec::new();
        for rec in inner.by_hash.values_mut() {
            if rec.status == SendStatus::Sent && rec.sent_at.elapsed() > timeout {
                rec.status = SendStatus::Timeout;
                out.push(rec.subnet.clone());
            }
        }
        out
    }

    pub fn prune(&self) {
        let ttl = self.ttl;
        self.lock().by_hash.retain(|_, rec| rec.sent_at.elapsed() < ttl);
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn record_send_result(subnet: &str, result: &str) {
    metrics::counter!("pog_sends_total", "subnet" => subnet.to_string(), "result" => result.to_string())
        .increment(1);
}

pub fn record_penalize(subnet: &str) {
    metrics::counter!("pog_penalized_total", "subnet" => subnet.to_string()).increment(1);
}

pub fn refresh_inflight_gauge(count: usize) {
    metrics::gauge!("pog_sends_inflight").set(count as f64);
}

/// Replace `/24` occupancy gauges. Zeroes subnets that dropped off.
pub fn refresh_peer_subnet_gauges(occupancy: &[(String, String)]) {
    static LAST: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);
    let mut counts: HashMap<(String, String), u64> = HashMap::new();
    for (subnet, direction) in occupancy {
        *counts.entry((subnet.clone(), direction.clone())).or_default() += 1;
    }
    let mut last_guard = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let last = last_guard.get_or_insert_with(HashSet::new);
    for key in last.iter() {
        if !counts.contains_key(key) {
            metrics::gauge!("pog_peers", "subnet" => key.0.clone(), "direction" => key.1.clone())
                .set(0.0);
        }
    }
    for (key, n) in &counts {
        metrics::gauge!("pog_peers", "subnet" => key.0.clone(), "direction" => key.1.clone())
            .set(*n as f64);
    }
    *last = counts.keys().cloned().collect();
}

pub async fn run_send_watcher(
    shutdown: reth::tasks::shutdown::GracefulShutdown,
    map: std::sync::Arc<SendMap>,
    mut canon_events: reth::providers::CanonStateNotifications<
        crate::primitives::BerachainPrimitives,
    >,
) {
    use alloy_consensus::BlockHeader as _;
    use reth_primitives_traits::{BlockBody as _, transaction::TxHashRef as _};

    let mut shutdown = shutdown;
    let mut tick = tokio::time::interval(WATCHER_TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tracing::info!(target: "bera_reth::pog", "PoG send watcher started");

    loop {
        tokio::select! {
            guard = &mut shutdown => {
                drop(guard);
                tracing::info!(target: "bera_reth::pog", "PoG send watcher stopped");
                return;
            }
            event = canon_events.recv() => {
                let chain = match event {
                    Ok(notification) => notification.committed(),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::info!(target: "bera_reth::pog", skipped = n, "canon state stream lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                };
                for block in chain.blocks_iter() {
                    let block_num = block.header().number();
                    let hashes: Vec<TxHash> = block
                        .body()
                        .transactions_iter()
                        .map(|tx| *tx.tx_hash())
                        .collect();
                    for subnet in map.note_block(block_num, &hashes) {
                        record_send_result(&subnet, "landed");
                    }
                }
                refresh_inflight_gauge(map.inflight_count());
            }
            _ = tick.tick() => {
                for subnet in map.expire_timeouts() {
                    record_send_result(&subnet, "timeout");
                }
                map.prune();
                refresh_inflight_gauge(map.inflight_count());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn session_enode_uses_tcp_endpoint() {
        let id = PeerId::repeat_byte(0xab);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(51, 68, 187, 101)), 30304);
        let enode = session_enode(id, addr);
        assert!(enode.starts_with("enode://"));
        assert!(enode.ends_with("@51.68.187.101:30304"));
        assert!(!enode.contains("0x"));
    }

    #[test]
    fn subnet_v4_is_slash_24() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(51, 68, 187, 101)), 1);
        assert_eq!(subnet_24(addr), "51.68.187.0/24");
    }

    #[test]
    fn signer_serialization_one_inflight() {
        let map = SendMap::new(Duration::from_secs(60), Duration::from_secs(120));
        let from = Address::repeat_byte(1);
        map.insert(SendRecord {
            tx_hash: TxHash::repeat_byte(1),
            peer_id: PeerId::repeat_byte(2),
            enode: "enode://x@1.2.3.4:5".into(),
            subnet: "1.2.3.0/24".into(),
            from,
            nonce: 0,
            sent_at: Instant::now(),
            sent_at_unix: 0,
            status: SendStatus::Sent,
            block_number: None,
        });
        assert!(map.signer_inflight(from));
        assert_eq!(map.inflight_count(), 1);
        map.note_block(10, &[TxHash::repeat_byte(1)]);
        assert!(!map.signer_inflight(from));
        assert_eq!(map.snapshot()[0].status, SendStatus::Landed);
        assert_eq!(map.snapshot()[0].block_number, Some(10));
    }

    #[test]
    fn timeout_then_late_land() {
        let map = SendMap::new(Duration::from_millis(1), Duration::from_secs(60));
        let hash = TxHash::repeat_byte(9);
        map.insert(SendRecord {
            tx_hash: hash,
            peer_id: PeerId::repeat_byte(2),
            enode: "enode://x@1.2.3.4:5".into(),
            subnet: "1.2.3.0/24".into(),
            from: Address::repeat_byte(1),
            nonce: 3,
            sent_at: Instant::now() - Duration::from_secs(1),
            sent_at_unix: 0,
            status: SendStatus::Sent,
            block_number: None,
        });
        let timed = map.expire_timeouts();
        assert_eq!(timed, vec!["1.2.3.0/24".to_string()]);
        map.note_block(11, &[hash]);
        assert_eq!(map.snapshot()[0].status, SendStatus::Landed);
    }
}
