use crate::{
    chainspec::BerachainChainSpec,
    primitives::{BerachainBlock, BerachainPrimitives, header::BlsPublicKey},
};
use alloy_eips::{
    eip4895::{Withdrawal, Withdrawals},
    eip7685::Requests,
};
use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::engine::{
    BlobsBundleV1, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV4, ExecutionPayloadEnvelopeV5, ExecutionPayloadV1, ExecutionPayloadV3,
    PayloadId,
};
use reth::{
    api::PayloadAttributes,
    builder::{PayloadAttributesBuilder, PayloadBuilderAttributes},
    chainspec::EthereumHardforks,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_ethereum_engine_primitives::{BlobSidecars, BuiltPayloadConversionError, payload_id};
use reth_node_ethereum::engine::EthPayloadAttributes;
use reth_payload_primitives::BuiltPayload;
use reth_primitives_traits::{NodePrimitives, SealedBlock};
use std::{convert::Infallible, sync::Arc};

/// Berachain-specific payload attributes
///
/// This structure wraps Ethereum payload attributes and provides extension
/// points for Berachain-specific functionality. Currently it delegates to
/// Ethereum attributes but can be extended with additional fields as needed.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    #[serde(rename = "parentProposerPubKey")]
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
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

impl BerachainPayloadAttributes {
    pub fn prev_proposer_pubkey(&self) -> Option<BlsPublicKey> {
        self.prev_proposer_pubkey
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
    pub prev_proposer_pubkey: Option<BlsPublicKey>,
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
            prev_proposer_pubkey: attributes.prev_proposer_pubkey,
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
    pub fn prev_proposer_pubkey(&self) -> Option<BlsPublicKey> {
        self.prev_proposer_pubkey
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
            prev_proposer_pubkey: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BerachainBuiltPayload {
    /// Identifier of the payload
    pub id: PayloadId,
    /// The built block
    pub block: Arc<SealedBlock<BerachainBlock>>,
    /// The fees of the block
    pub fees: U256,
    /// The blobs, proofs, and commitments in the block. If the block is pre-cancun, this will be
    /// empty.
    pub sidecars: BlobSidecars,
    /// The requests of the payload
    pub requests: Option<Requests>,
}

impl BerachainBuiltPayload {
    /// Initializes the payload with the given initial block
    ///
    /// Caution: This does not set any [`BlobSidecars`].
    pub const fn new(
        id: PayloadId,
        block: Arc<SealedBlock<BerachainBlock>>,
        fees: U256,
        requests: Option<Requests>,
    ) -> Self {
        Self { id, block, fees, requests, sidecars: BlobSidecars::Empty }
    }

    /// Sets blob transactions sidecars on the payload.
    pub fn with_sidecars(mut self, sidecars: impl Into<BlobSidecars>) -> Self {
        self.sidecars = sidecars.into();
        self
    }

    /// Try converting built payload into [`ExecutionPayloadEnvelopeV3`].
    ///
    /// Returns an error if the payload contains non EIP-4844 sidecar.
    pub fn try_into_v3(self) -> Result<ExecutionPayloadEnvelopeV3, BuiltPayloadConversionError> {
        let Self { block, fees, sidecars, .. } = self;

        let blobs_bundle = match sidecars {
            BlobSidecars::Empty => BlobsBundleV1::empty(),
            BlobSidecars::Eip4844(sidecars) => BlobsBundleV1::from(sidecars),
            BlobSidecars::Eip7594(_) => {
                return Err(BuiltPayloadConversionError::UnexpectedEip7594Sidecars)
            }
        };

        Ok(ExecutionPayloadEnvelopeV3 {
            execution_payload: ExecutionPayloadV3::from_block_unchecked(
                block.hash(),
                &Arc::unwrap_or_clone(block).into_block(),
            ),
            block_value: fees,
            // From the engine API spec:
            //
            // > Client software **MAY** use any heuristics to decide whether to set
            // `shouldOverrideBuilder` flag or not. If client software does not implement any
            // heuristic this flag **SHOULD** be set to `false`.
            //
            // Spec:
            // <https://github.com/ethereum/execution-apis/blob/fe8e13c288c592ec154ce25c534e26cb7ce0530d/src/engine/cancun.md#specification-2>
            should_override_builder: false,
            blobs_bundle,
        })
    }

    pub fn try_into_v4(self) -> Result<ExecutionPayloadEnvelopeV4, BuiltPayloadConversionError> {
        Ok(ExecutionPayloadEnvelopeV4 {
            execution_requests: self.requests.clone().unwrap_or_default(),
            envelope_inner: self.try_into()?,
        })
    }
}

impl From<BerachainBuiltPayload> for ExecutionPayloadV1 {
    fn from(_value: BerachainBuiltPayload) -> Self {
        panic!("ExecutionPayloadV1 conversion not supported for Berachain - use V3+ for Prague1")
    }
}

impl From<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV2 {
    fn from(_value: BerachainBuiltPayload) -> Self {
        panic!("ExecutionPayloadV2 conversion not supported for Berachain - use V3+ for Prague1")
    }
}

impl TryFrom<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV3 {
    type Error = BuiltPayloadConversionError;

    fn try_from(value: BerachainBuiltPayload) -> Result<Self, Self::Error> {
        value.try_into_v3()
    }
}

impl TryFrom<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV4 {
    type Error = BuiltPayloadConversionError;

    fn try_from(value: BerachainBuiltPayload) -> Result<Self, Self::Error> {
        value.try_into_v4()
    }
}

impl From<BerachainBuiltPayload> for ExecutionPayloadEnvelopeV5 {
    fn from(_value: BerachainBuiltPayload) -> Self {
        panic!("ExecutionPayloadV5 conversion not yet supported for Berachain")
    }
}

impl BuiltPayload for BerachainBuiltPayload {
    type Primitives = BerachainPrimitives;

    fn block(&self) -> &SealedBlock<<Self::Primitives as NodePrimitives>::Block> {
        &self.block
    }

    fn fees(&self) -> U256 {
        self.fees
    }

    fn requests(&self) -> Option<Requests> {
        self.requests.clone()
    }
}
