//! `beradmin` JSON-RPC namespace (WP1): detailed peers, node status, ban/penalize,
//! sentinel-driven PoG prepare/submit.

mod helpers;
mod rpc_impl;
mod types;

pub use helpers::PogCanarySend;
pub use rpc_impl::BerAdminImpl;
pub use types::*;

use jsonrpsee::{core::RpcResult, proc_macros::rpc};

#[rpc(server, namespace = "beradmin")]
pub trait BerAdminApi {
    #[method(name = "detailedPeers")]
    async fn detailed_peers(&self) -> RpcResult<Vec<DetailedPeer>>;

    #[method(name = "banPeer")]
    fn ban_peer(&self, peer_id: String) -> RpcResult<()>;

    #[method(name = "penalizePeer")]
    fn penalize_peer(&self, peer_id: String, value: i32) -> RpcResult<()>;

    #[method(name = "nodeStatus")]
    async fn node_status(&self) -> RpcResult<NodeStatusResponse>;

    /// Breaking change: `target_peer_id` is now required; the node no longer selects a target.
    #[method(name = "prepareCanary")]
    async fn prepare_canary(
        &self,
        signer_address: String,
        target_peer_id: String,
    ) -> RpcResult<PrepareCanaryResponse>;

    #[method(name = "submitCanary")]
    async fn submit_canary(&self, signed_tx: String) -> RpcResult<SubmitCanaryResponse>;

    #[method(name = "sealedBlockAttribution")]
    async fn sealed_block_attribution(
        &self,
        block_number: Option<u64>,
    ) -> RpcResult<SealedBlockAttributionResponse>;
}
