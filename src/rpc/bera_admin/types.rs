use alloy_primitives::B256;
use serde::{Deserialize, Serialize};

pub use crate::pog::PogPeerStatus;

/// One peer entry from `beradmin_detailedPeers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedPeer {
    pub peer_id: String,
    pub enode: String,
    pub remote_addr: String,
    pub direction: String,
    pub client_version: String,
    pub chain_id: u64,
    pub genesis: B256,
    pub fork_id_hash: String,
    pub fork_id_next: u64,
    pub blockhash: B256,
    pub total_difficulty: Option<alloy_primitives::U256>,
    pub latest_block: Option<u64>,
    pub earliest_block: Option<u64>,
    pub reputation: i32,
    pub session_duration_mins: u64,
    pub backed_off: bool,
    pub severe_backoff_counter: u8,
    pub connection_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_fork_id_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_fork_id_next: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pog: Option<PogPeerStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatusResponse {
    pub chain_id: u64,
    pub genesis_hash: B256,
    pub fork_id_hash: String,
    pub fork_id_next: u64,
    pub head_number: u64,
    pub head_hash: B256,
    pub peer_count_inbound: usize,
    pub peer_count_outbound: usize,
    pub peer_count_total: usize,
    pub syncing: bool,
    pub client_version: String,
    pub network_id: u64,
    pub local_enode: String,
    pub local_peer_id: String,
    pub ban_threshold: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxAttribution {
    pub tx_hash: B256,
    pub from_peer_id: Option<String>,
    /// `(effective_gas_price − base_fee) × gas_used`
    pub effective_tip_wei: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedBlockAttributionResponse {
    pub block_number: u64,
    pub transactions: Vec<TxAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCanaryResponse {
    pub peer_id: String,
    pub enode: String,
    /// Hex-encoded EIP-1559 signing preimage (`0x02` + RLP) for sentinel to sign.
    pub unsigned_tx: String,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCanaryResponse {
    pub tx_hash: B256,
    pub peer_id: String,
    pub enode: String,
}
