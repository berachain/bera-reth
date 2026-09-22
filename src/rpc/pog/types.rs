//! Wire types for the `pog` namespace.

use alloy_primitives::{Address, B256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PogPeer {
    pub peer_id: String,
    pub enode: String,
    pub direction: String,
    pub client: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRawTransactionResponse {
    pub tx_hash: B256,
    pub peer_id: String,
    pub enode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PenalizeResponse {
    pub peer_id: String,
    /// Whether the peer held a session when the penalty was applied.
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PogSend {
    pub tx_hash: B256,
    pub peer_id: String,
    pub enode: String,
    pub from: Address,
    pub nonce: u64,
    pub sent_at: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PogNodeStatus {
    pub syncing: bool,
    pub head_number: u64,
    pub head_hash: B256,
    pub sends_inflight: usize,
}
