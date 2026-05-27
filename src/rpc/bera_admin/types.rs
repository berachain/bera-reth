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

// ---------------------------------------------------------------------------
// Sealed-tx-fact export wire shape (BERA-265).
// ---------------------------------------------------------------------------

/// Reserved wire slot for subsequent-peer relay records. Always serialized as `[]` in v1
/// (see brief §5.7 / AC-R4); BERA-261 populates it once upstream reth lands BERA-260.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtraHear {
    pub peer_id: String,
    pub heard_at_ms: u64,
}

/// JSON-RPC params for `beradmin_exportSealedTxFacts`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSealedTxFactsRequest {
    /// Exclusive lower bound on `id`. `0` means "start from the beginning of retained history."
    pub after_id: u64,
    /// Requested page size. Server rejects values outside `[1, server_max_limit]`.
    pub limit: u32,
}

/// One `sealed_tx_fact` row as it appears on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SealedTxFactRow {
    pub id: u64,
    pub sealed_block_number: u64,
    pub tx_hash: String,
    pub first_peer_id: Option<String>,
    pub first_heard_ms: u64,
    /// Effective tip in wei, encoded as a `u128` "Quantity" (lowercase 0x-prefixed
    /// minimal hex, `"0x0"` for zero). See brief §5.5 and the `DRIFT-SENTINEL` resolution.
    #[serde(with = "alloy_serde::quantity")]
    pub effective_tip_wei: u128,
    pub tip_formula_version: u32,
    /// Reserved slot for BERA-261 extras-population. Always `[]` in v1.
    pub extra_hears: Vec<ExtraHear>,
    /// Canonical `enode://hex@ip:port` URL captured from the peer's first-hear devp2p Hello
    /// (BERA-305). The key is always present on export wire (`null` when `Hello.port == 0`
    /// or no listening address). Single supported JSON shape per BERA-465 — coordinate
    /// sentinel mirror bumps with producer releases.
    pub first_enode: Option<String>,
}

/// JSON-RPC response for `beradmin_exportSealedTxFacts`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportSealedTxFactsResponse {
    pub rows: Vec<SealedTxFactRow>,
    pub next_after_id: u64,
    pub high_water_id: u64,
    pub min_retained_id: u64,
    pub truncated: bool,
}
