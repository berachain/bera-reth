use crate::{chainspec::BerachainChainSpec, primitives::BerachainPrimitives};
use alloy_eips::{
    eip4895::{Withdrawal, Withdrawals},
    eip7685::Requests,
};
use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::engine::{ExecutionData, PayloadId};
use reth::{
    api::PayloadAttributes,
    builder::{PayloadAttributesBuilder, PayloadBuilderAttributes},
    chainspec::EthereumHardforks,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_ethereum_engine_primitives::{EthBuiltPayload, payload_id};
use reth_node_ethereum::engine::EthPayloadAttributes;
use reth_payload_primitives::{BuiltPayload, PayloadTypes};
use reth_primitives_traits::{NodePrimitives, SealedBlock};
use std::convert::Infallible;

/// Berachain-specific payload attributes
///
/// This structure wraps Ethereum payload attributes and provides extension
/// points for Berachain-specific functionality. Currently it delegates to
/// Ethereum attributes but can be extended with additional fields as needed.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    // TODO: Add Berachain-specific fields here as needed
    // Example: pub system_transactions: Option<Vec<SystemTransaction>>,
}

impl PayloadAttributes for BerachainPayloadAttributes {
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

/// Berachain payload builder attributes
///
/// Internal representation of payload attributes used during the payload building process.
/// This structure maintains compatibility with Ethereum while providing extension points
/// for Berachain-specific payload building logic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BerachainPayloadBuilderAttributes {
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

impl PayloadBuilderAttributes for BerachainPayloadBuilderAttributes {
    type RpcPayloadAttributes = BerachainPayloadAttributes;
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

impl BerachainPayloadBuilderAttributes {
    /// Convert to Ethereum payload attributes for compatibility
    ///
    /// This method provides the necessary conversion to interface with
    /// Ethereum payload building logic while maintaining all required data.
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

/// Implementation for LocalPayloadAttributesBuilder to build BerachainPayloadAttributes
impl PayloadAttributesBuilder<BerachainPayloadAttributes>
    for LocalPayloadAttributesBuilder<BerachainChainSpec>
{
    fn build(&self, timestamp: u64) -> BerachainPayloadAttributes {
        BerachainPayloadAttributes {
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

#[derive(Debug, Clone)]
pub struct BerachainBuiltPayload;

impl BuiltPayload for BerachainBuiltPayload {
    type Primitives = BerachainPrimitives;

    fn block(&self) -> &SealedBlock<<Self::Primitives as NodePrimitives>::Block> {
        todo!()
    }

    fn fees(&self) -> U256 {
        todo!()
    }

    fn requests(&self) -> Option<Requests> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256};
    use reth::api::PayloadAttributes;

    fn create_test_bera_payload_attributes() -> BerachainPayloadAttributes {
        BerachainPayloadAttributes {
            inner: EthPayloadAttributes {
                timestamp: 1234567890,
                prev_randao: B256::from([1u8; 32]),
                suggested_fee_recipient: Address::from([2u8; 20]),
                withdrawals: Some(vec![]),
                parent_beacon_block_root: Some(B256::from([3u8; 32])),
            },
        }
    }

    fn create_test_bera_payload_builder_attributes() -> BerachainPayloadBuilderAttributes {
        let parent = B256::from([4u8; 32]);
        let attributes = create_test_bera_payload_attributes();
        BerachainPayloadBuilderAttributes::try_new(parent, attributes, 1).unwrap()
    }

    #[test]
    fn test_bera_payload_attributes_payload_attributes_trait() {
        let attributes = create_test_bera_payload_attributes();

        assert_eq!(attributes.timestamp(), 1234567890);
        assert_eq!(attributes.withdrawals(), Some(&vec![]));
        assert_eq!(attributes.parent_beacon_block_root(), Some(B256::from([3u8; 32])));
    }

    #[test]
    fn test_bera_payload_builder_attributes_try_new() {
        let parent = B256::from([4u8; 32]);
        let rpc_attributes = create_test_bera_payload_attributes();

        let builder_attributes =
            BerachainPayloadBuilderAttributes::try_new(parent, rpc_attributes.clone(), 1).unwrap();

        assert_eq!(builder_attributes.parent(), parent);
        assert_eq!(builder_attributes.timestamp(), rpc_attributes.inner.timestamp);
        assert_eq!(
            builder_attributes.suggested_fee_recipient(),
            rpc_attributes.inner.suggested_fee_recipient
        );
        assert_eq!(builder_attributes.prev_randao(), rpc_attributes.inner.prev_randao);
        assert_eq!(
            builder_attributes.parent_beacon_block_root(),
            rpc_attributes.inner.parent_beacon_block_root
        );
    }

    #[test]
    fn test_bera_payload_builder_attributes_payload_id_deterministic() {
        let parent = B256::from([4u8; 32]);
        let rpc_attributes = create_test_bera_payload_attributes();

        let builder_attributes_1 =
            BerachainPayloadBuilderAttributes::try_new(parent, rpc_attributes.clone(), 1).unwrap();
        let builder_attributes_2 =
            BerachainPayloadBuilderAttributes::try_new(parent, rpc_attributes, 1).unwrap();

        assert_eq!(builder_attributes_1.payload_id(), builder_attributes_2.payload_id());
    }

    #[test]
    fn test_to_eth_payload_attributes_conversion() {
        let builder_attributes = create_test_bera_payload_builder_attributes();
        let eth_attributes = builder_attributes.to_eth_payload_attributes();

        assert_eq!(eth_attributes.timestamp, builder_attributes.timestamp);
        assert_eq!(eth_attributes.prev_randao, builder_attributes.prev_randao);
        assert_eq!(
            eth_attributes.suggested_fee_recipient,
            builder_attributes.suggested_fee_recipient
        );
        assert_eq!(eth_attributes.withdrawals, Some(builder_attributes.withdrawals.to_vec()));
        assert_eq!(
            eth_attributes.parent_beacon_block_root,
            builder_attributes.parent_beacon_block_root
        );
    }

    #[test]
    fn test_withdrawals_conversion() {
        let parent = B256::from([4u8; 32]);
        let mut rpc_attributes = create_test_bera_payload_attributes();

        // Test with empty withdrawals
        rpc_attributes.inner.withdrawals = Some(vec![]);
        let builder_attributes =
            BerachainPayloadBuilderAttributes::try_new(parent, rpc_attributes.clone(), 1).unwrap();
        assert!(builder_attributes.withdrawals().is_empty());

        // Test with None withdrawals (should default to empty)
        rpc_attributes.inner.withdrawals = None;
        let builder_attributes =
            BerachainPayloadBuilderAttributes::try_new(parent, rpc_attributes, 1).unwrap();
        assert!(builder_attributes.withdrawals().is_empty());
    }
}
