pub mod builder;

use jsonrpsee_core::Serialize;
use serde::Deserialize;

pub const BLS_PUBKEY_LENGTH: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlsPubkey(#[serde(with = "hex")] pub [u8; BLS_PUBKEY_LENGTH]);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub payload_attributes: alloy_rpc_types_engine::PayloadAttributes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_validator_pubkey: Option<BlsPubkey>,
}
