pub mod builder;

use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_primitives::B256;
use alloy_rpc_types_engine::PayloadId;
use reth_ethereum_engine_primitives::EthPayloadTypes;
use reth_payload_builder::EthPayloadBuilderAttributes;
use reth_payload_primitives::{PayloadAttributes, PayloadBuilderAttributes, PayloadTypes};
use reth_primitives::EthPrimitives;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

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

    fn payload_id(&self) -> PayloadId {
        self.payload_attributes.payload_id()
    }

    fn parent(&self) -> B256 {
        self.payload_attributes.parent()
    }

    fn timestamp(&self) -> u64 {
        self.payload_attributes.timestamp()
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

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.payload_attributes.parent_beacon_block_root()
    }

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
}

pub fn berachain_payload_id(parent: &B256, attributes: &BerachainPayloadAttributes) -> PayloadId {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(parent.as_slice());
    hasher.update(&attributes.payload_attributes.timestamp.to_be_bytes()[..]);
    hasher.update(attributes.payload_attributes.prev_randao.as_slice());
    hasher.update(attributes.payload_attributes.suggested_fee_recipient.as_slice());

    if let Some(withdrawals) = &attributes.payload_attributes.withdrawals {
        for withdrawal in withdrawals {
            hasher.update(&withdrawal.index.to_be_bytes()[..]);
            hasher.update(&withdrawal.validator_index.to_be_bytes()[..]);
            hasher.update(withdrawal.address.as_slice());
            hasher.update(&withdrawal.amount.to_be_bytes()[..]);
        }
    }

    if let Some(parent_beacon_block_root) = &attributes.payload_attributes.parent_beacon_block_root
    {
        hasher.update(parent_beacon_block_root.as_slice());
    }

    if let Some(validator_pubkey) = &attributes.validator_pubkey {
        hasher.update(&validator_pubkey.0);
    }

    let out = hasher.finalize();
    PayloadId::new(out[..8].try_into().expect("sufficient length"))
}

#[derive(Debug, Default, Clone)]
pub struct BerachainPayloadTypes<N = EthPrimitives>(PhantomData<N>);

impl<N: reth_primitives::NodePrimitives> PayloadTypes for BerachainPayloadTypes<N> {
    type ExecutionData = <EthPayloadTypes as PayloadTypes>::ExecutionData;
    type BuiltPayload = <EthPayloadTypes as PayloadTypes>::BuiltPayload;
    type PayloadAttributes = BerachainPayloadAttributes;
    type PayloadBuilderAttributes = BerachainPayloadBuilderAttributes;

    fn block_to_payload(
        block: reth_primitives::SealedBlock<
            <<Self::BuiltPayload as reth_payload_primitives::BuiltPayload>::Primitives as reth_primitives::NodePrimitives>::Block,
        >,
    ) -> Self::ExecutionData {
        EthPayloadTypes::block_to_payload(block)
    }
}
