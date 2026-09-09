//! Mock [`PogNet`] for unit tests.

use crate::transaction::BerachainTxEnvelope;
use alloy_primitives::B256;
use reth_eth_wire_types::{Capabilities, Capability, EthVersion, UnifiedStatus};
use reth_network_api::{Direction, NetworkError, PeerInfo};
use reth_network_peers::PeerId;
use reth_primitives_traits::transaction::TxHashRef;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Debug, Default)]
struct State {
    syncing: bool,
    peers: Vec<PeerInfo>,
    sent: Vec<(PeerId, B256)>,
}

#[derive(Clone, Debug)]
pub struct MockPogNet {
    state: Arc<Mutex<State>>,
}

impl MockPogNet {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(State::default())) }
    }

    pub fn syncing(self, syncing: bool) -> Self {
        self.state.lock().expect("lock").syncing = syncing;
        self
    }

    pub fn with_peer(self, peer_id: PeerId, addr: SocketAddr, incoming: bool) -> Self {
        self.state.lock().expect("lock").peers.push(test_peer(peer_id, addr, incoming));
        self
    }

    pub fn sent(&self) -> Vec<(PeerId, B256)> {
        self.state.lock().expect("lock").sent.clone()
    }
}

fn test_peer(peer_id: PeerId, addr: SocketAddr, incoming: bool) -> PeerInfo {
    use alloy_hardforks::{ForkHash, ForkId};
    let status = Arc::new(
        UnifiedStatus::builder()
            .version(EthVersion::Eth68)
            .chain(alloy_chains::Chain::from_id(80094))
            .genesis(B256::ZERO)
            .forkid(ForkId { hash: ForkHash([0u8; 4]), next: 0 })
            .blockhash(B256::ZERO)
            .build(),
    );
    PeerInfo {
        capabilities: Arc::new(Capabilities::new(vec![Capability::new_static("eth", 68)])),
        remote_id: peer_id,
        client_version: Arc::from("mock-peer"),
        enode: "enode://advertised@9.9.9.9:1".into(),
        enr: None,
        remote_addr: addr,
        local_addr: None,
        direction: if incoming { Direction::Incoming } else { Direction::Outgoing(peer_id) },
        eth_version: EthVersion::Eth68,
        status,
        session_established: Instant::now(),
        kind: reth_network_api::PeerKind::Basic,
    }
}

impl super::PogNet for MockPogNet {
    fn is_syncing(&self) -> bool {
        self.state.lock().expect("lock").syncing
    }

    fn get_all_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerInfo>, NetworkError>> + Send {
        let peers = self.state.lock().expect("lock").peers.clone();
        async move { Ok(peers) }
    }

    fn send_raw(&self, peer_id: PeerId, tx: Arc<BerachainTxEnvelope>) {
        let hash = *TxHashRef::tx_hash(tx.as_ref());
        self.state.lock().expect("lock").sent.push((peer_id, hash));
    }
}
