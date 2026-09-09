//! `pog` JSON-RPC: peer inventory, targeted raw send, RAM send window, node status.

mod network;
mod rpc_impl;
mod types;

#[cfg(test)]
mod mock_network;
#[cfg(test)]
mod rpc_tests;

pub use network::PogNet;
pub use rpc_impl::PogApiImpl;
pub use types::*;

use jsonrpsee::{core::RpcResult, proc_macros::rpc};

#[rpc(server, namespace = "pog")]
pub trait PogApi {
    #[method(name = "peers")]
    async fn peers(&self) -> RpcResult<Vec<PogPeer>>;

    #[method(name = "sendRawTransaction")]
    async fn send_raw_transaction(
        &self,
        peer_id: String,
        raw_tx: String,
    ) -> RpcResult<SendRawTransactionResponse>;

    #[method(name = "sends")]
    fn sends(&self) -> RpcResult<Vec<PogSend>>;

    #[method(name = "nodeStatus")]
    async fn node_status(&self) -> RpcResult<PogNodeStatus>;
}
