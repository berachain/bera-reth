pub mod builder;

use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_primitives::B256;
use alloy_rpc_types_engine::PayloadId;
use reth_payload_builder::EthPayloadBuilderAttributes;
use reth_payload_primitives::{PayloadAttributes, PayloadBuilderAttributes};
use serde::{Deserialize, Serialize};

pub use builder::BerachainPayloadBuilder;

pub const BLS_PUBKEY_LENGTH: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlsPubkey(#[serde(with = "hex")] pub [u8; BLS_PUBKEY_LENGTH]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub payload_attributes: alloy_rpc_types_engine::PayloadAttributes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validator_pubkey: Option<BlsPubkey>,
}

impl BerachainPayloadAttributes {
    pub fn new(payload_attributes: alloy_rpc_types_engine::PayloadAttributes) -> Self {
        Self { payload_attributes, validator_pubkey: None }
    }

    pub fn with_validator_pubkey(mut self, validator_pubkey: BlsPubkey) -> Self {
        self.validator_pubkey = Some(validator_pubkey);
        self
    }
}

impl From<alloy_rpc_types_engine::PayloadAttributes> for BerachainPayloadAttributes {
    fn from(payload_attributes: alloy_rpc_types_engine::PayloadAttributes) -> Self {
        Self::new(payload_attributes)
    }
}

impl PayloadAttributes for BerachainPayloadAttributes {
    fn timestamp(&self) -> u64 {
        self.payload_attributes.timestamp
    }

    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> {
        self.payload_attributes.withdrawals.as_ref()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.payload_attributes.parent_beacon_block_root
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BerachainPayloadBuilderAttributes {
    pub payload_attributes: EthPayloadBuilderAttributes,
    pub validator_pubkey: Option<BlsPubkey>,
}

impl BerachainPayloadBuilderAttributes {
    pub fn new(payload_attributes: EthPayloadBuilderAttributes) -> Self {
        Self { payload_attributes, validator_pubkey: None }
    }

    pub fn with_validator_pubkey(mut self, validator_pubkey: BlsPubkey) -> Self {
        self.validator_pubkey = Some(validator_pubkey);
        self
    }
}

impl PayloadBuilderAttributes for BerachainPayloadBuilderAttributes {
    type RpcPayloadAttributes = BerachainPayloadAttributes;
    type Error = <EthPayloadBuilderAttributes as PayloadBuilderAttributes>::Error;

    fn try_new(
        parent: B256,
        rpc_payload_attributes: Self::RpcPayloadAttributes,
        version: u8,
    ) -> Result<Self, Self::Error> {
        let eth_attributes = EthPayloadBuilderAttributes::try_new(
            parent,
            rpc_payload_attributes.payload_attributes,
            version,
        )?;

        Ok(Self {
            payload_attributes: eth_attributes,
            validator_pubkey: rpc_payload_attributes.validator_pubkey,
        })
    }

    fn payload_id(&self) -> PayloadId {
        self.payload_attributes.payload_id()
    }

    fn parent(&self) -> B256 {
        self.payload_attributes.parent()
    }

    fn timestamp(&self) -> u64 {
        self.payload_attributes.timestamp()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.payload_attributes.parent_beacon_block_root()
    }

    fn suggested_fee_recipient(&self) -> alloy_primitives::Address {
        self.payload_attributes.suggested_fee_recipient()
    }

    fn prev_randao(&self) -> B256 {
        self.payload_attributes.prev_randao()
    }

    fn withdrawals(&self) -> &Withdrawals {
        self.payload_attributes.withdrawals()
    }
}
