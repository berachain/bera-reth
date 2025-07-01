use crate::chainspec::BerachainChainSpec;
use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_primitives::{Address, B256};
use alloy_rpc_types::engine::PayloadId;
use reth::{
    api::PayloadAttributes,
    builder::{PayloadAttributesBuilder, PayloadBuilderAttributes},
    chainspec::EthereumHardforks,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_ethereum_engine_primitives::payload_id;
use reth_node_ethereum::engine::EthPayloadAttributes;
use std::convert::Infallible;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BeraPayloadAttributes {
    pub inner: EthPayloadAttributes,
}

impl PayloadAttributes for BeraPayloadAttributes {
    fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }
    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> {
        self.inner.withdrawals.as_ref()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.inner.parent_beacon_block_root
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeraPayloadBuilderAttributes {
    /// Id of the payload
    pub id: PayloadId,
    /// Parent block to build the payload on top
    pub parent: B256,
    /// Unix timestamp for the generated payload
    ///
    /// Number of seconds since the Unix epoch.
    pub timestamp: u64,
    /// Address of the recipient for collecting transaction fee
    pub suggested_fee_recipient: Address,
    /// Randomness value for the generated payload
    pub prev_randao: B256,
    /// Withdrawals for the generated payload
    pub withdrawals: Withdrawals,
    /// Root of the parent beacon block
    pub parent_beacon_block_root: Option<B256>,
}

impl PayloadBuilderAttributes for BeraPayloadBuilderAttributes {
    type RpcPayloadAttributes = BeraPayloadAttributes;
    type Error = Infallible;

    fn try_new(
        parent: B256,
        attributes: Self::RpcPayloadAttributes,
        _version: u8,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let payload_id = payload_id(&parent, &attributes.inner);
        Ok(Self {
            id: payload_id,
            parent,
            timestamp: attributes.inner.timestamp,
            suggested_fee_recipient: attributes.inner.suggested_fee_recipient,
            prev_randao: attributes.inner.prev_randao,
            withdrawals: attributes.inner.withdrawals.unwrap_or_default().into(),
            parent_beacon_block_root: attributes.inner.parent_beacon_block_root,
        })
    }

    fn payload_id(&self) -> PayloadId {
        self.id
    }

    fn parent(&self) -> B256 {
        self.parent
    }

    fn timestamp(&self) -> u64 {
        self.timestamp
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.parent_beacon_block_root
    }

    fn suggested_fee_recipient(&self) -> Address {
        self.suggested_fee_recipient
    }

    fn prev_randao(&self) -> B256 {
        self.prev_randao
    }

    fn withdrawals(&self) -> &Withdrawals {
        &self.withdrawals
    }
}

impl BeraPayloadBuilderAttributes {
    pub fn to_eth_payload_attributes(&self) -> EthPayloadAttributes {
        EthPayloadAttributes {
            timestamp: self.timestamp,
            prev_randao: self.prev_randao,
            suggested_fee_recipient: self.suggested_fee_recipient,
            withdrawals: Some(self.withdrawals.to_vec()),
            parent_beacon_block_root: self.parent_beacon_block_root,
        }
    }
}

/// Implementation for LocalPayloadAttributesBuilder to build BeraPayloadAttributes
impl PayloadAttributesBuilder<BeraPayloadAttributes>
    for LocalPayloadAttributesBuilder<BerachainChainSpec>
{
    fn build(&self, timestamp: u64) -> BeraPayloadAttributes {
        BeraPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp,
                prev_randao: B256::random(),
                suggested_fee_recipient: Address::random(),
                withdrawals: self
                    .chain_spec
                    .is_shanghai_active_at_timestamp(timestamp)
                    .then(Default::default),
                parent_beacon_block_root: self
                    .chain_spec
                    .is_cancun_active_at_timestamp(timestamp)
                    .then(B256::random),
            },
        }
    }
}
