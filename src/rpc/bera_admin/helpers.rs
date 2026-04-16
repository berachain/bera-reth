//! Small `beradmin` helpers and the PoG canary send abstraction.

use crate::transaction::BerachainTxEnvelope;
use alloy_primitives::hex;
use jsonrpsee::{core::RpcResult, types::ErrorObjectOwned};
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_peers::PeerId;

use super::types::{DetailedPeer, PogPeerStatus};

/// Targeted devp2p gossip for signed canary transactions (not broadcast RPC).
pub trait PogCanarySend: Send + Sync {
    fn pog_send_canary(&self, peer_id: PeerId, tx: std::sync::Arc<BerachainTxEnvelope>);
}

impl<N> PogCanarySend for NetworkHandle<N>
where
    N: NetworkPrimitives<BroadcastedTransaction = BerachainTxEnvelope> + Send + Sync + 'static,
{
    fn pog_send_canary(&self, peer_id: PeerId, tx: std::sync::Arc<BerachainTxEnvelope>) {
        NetworkHandle::send_transactions(self, peer_id, vec![tx]);
    }
}

pub fn parse_peer_id(s: &str) -> RpcResult<PeerId> {
    s.parse()
        .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid peer_id: {e}"), None::<()>))
}

pub fn fork_id_hash_hex(hash: [u8; 4]) -> String {
    format!("0x{}", hex::encode(hash))
}

pub fn peer_to_detailed(
    info: &reth_network_api::PeerInfo,
    peer: &reth_network_types::Peer,
    pog: Option<PogPeerStatus>,
) -> DetailedPeer {
    use reth_eth_wire_types::UnifiedStatus;
    let st: &UnifiedStatus = &info.status;
    let (disc_hash, disc_next) = match &peer.fork_id {
        Some(fid) => (Some(fork_id_hash_hex(fid.hash.0)), Some(fid.next)),
        None => (None, None),
    };
    DetailedPeer {
        peer_id: alloy_primitives::hex::encode(info.remote_id.as_slice()),
        enode: info.enode.clone(),
        remote_addr: info.remote_addr.to_string(),
        direction: info.direction.to_string(),
        client_version: info.client_version.to_string(),
        chain_id: st.chain.id(),
        genesis: st.genesis,
        fork_id_hash: fork_id_hash_hex(st.forkid.hash.0),
        fork_id_next: st.forkid.next,
        blockhash: st.blockhash,
        total_difficulty: st.total_difficulty,
        latest_block: st.latest_block,
        earliest_block: st.earliest_block,
        reputation: peer.reputation,
        session_duration_mins: info.session_established.elapsed().as_secs() / 60,
        backed_off: peer.backed_off,
        severe_backoff_counter: peer.severe_backoff_counter,
        connection_state: format!("{:?}", peer.state),
        discovery_fork_id_hash: disc_hash,
        discovery_fork_id_next: disc_next,
        pog,
    }
}
