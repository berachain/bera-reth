//! Network surface for targeted PoG sends.

use crate::transaction::BerachainTxEnvelope;
use reth_eth_wire_types::NetworkPrimitives;
use reth_network::NetworkHandle;
use reth_network_api::{NetworkError, NetworkInfo, PeerInfo, Peers, ReputationChangeKind};
use reth_network_peers::PeerId;
use std::sync::Arc;

pub trait PogNet: Clone + Send + Sync + 'static {
    fn is_syncing(&self) -> bool;

    fn get_all_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerInfo>, NetworkError>> + Send;

    fn send_raw(&self, peer_id: PeerId, tx: Arc<BerachainTxEnvelope>);

    /// Drop the peer below the ban threshold. Trusted peers are exempt.
    fn penalize(&self, peer_id: PeerId);
}

impl<N> PogNet for NetworkHandle<N>
where
    N: NetworkPrimitives<BroadcastedTransaction = BerachainTxEnvelope>,
{
    fn is_syncing(&self) -> bool {
        NetworkInfo::is_syncing(self)
    }

    fn get_all_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerInfo>, NetworkError>> + Send {
        Peers::get_all_peers(self)
    }

    fn send_raw(&self, peer_id: PeerId, tx: Arc<BerachainTxEnvelope>) {
        self.send_transactions(peer_id, vec![tx]);
    }

    fn penalize(&self, peer_id: PeerId) {
        Peers::reputation_change(self, peer_id, ReputationChangeKind::BadProtocol);
    }
}
