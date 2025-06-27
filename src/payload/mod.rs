pub mod builder;

use crate::{
    chainspec::BerachainChainSpec, hardforks::BerachainHardforks, node::evm::BerachainEvmConfig,
};
use alloy_eips::eip4895::{Withdrawal, Withdrawals};
use alloy_primitives::{Address, B256};
use alloy_rpc_types_engine::{
    ExecutionData, ExecutionPayload, ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3,
    ExecutionPayloadEnvelopeV4, ExecutionPayloadEnvelopeV5, ExecutionPayloadV1, PayloadId,
};
use jsonrpsee_core::Serialize;
use reth::{
    api,
    api::{EngineTypes, EngineValidator, PayloadValidator},
    core::{primitives, primitives::SealedBlock},
    primitives::{EthPrimitives, RecoveredBlock, TransactionSigned},
    providers::StateProviderFactory,
};
use reth_basic_payload_builder::{BuildArguments, BuildOutcome, PayloadBuilder, PayloadConfig};
use reth_chainspec::ChainSpecProvider;
use reth_ethereum_engine_primitives::{
    EthBuiltPayload, EthPayloadAttributes, EthPayloadBuilderAttributes,
};
use reth_ethereum_payload_builder::{EthereumBuilderConfig, EthereumExecutionPayloadValidator};
use reth_node_api::{AddOnsContext, FullNodeComponents, FullNodeTypes};
use reth_node_builder::{
    BuilderContext, components::PayloadBuilderBuilder, rpc::EngineValidatorBuilder,
};
use reth_node_types::NodeTypes;
use reth_payload_primitives::{
    EngineApiMessageVersion, EngineObjectValidationError, NewPayloadError, PayloadAttributes,
    PayloadBuilderAttributes, PayloadBuilderError, PayloadOrAttributes, PayloadTypes,
    validate_version_specific_fields,
};
use reth_primitives::Block;
use reth_transaction_pool::{PoolTransaction, TransactionPool};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc};

pub const BLS_PUBKEY_LENGTH: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlsPubkey(#[serde(with = "hex")] pub [u8; BLS_PUBKEY_LENGTH]);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BerachainPayloadAttributes {
    #[serde(flatten)]
    pub inner: EthPayloadAttributes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_validator_pubkey: Option<BlsPubkey>,
}

impl PayloadAttributes for BerachainPayloadAttributes {
    fn timestamp(&self) -> u64 {
        self.inner.timestamp()
    }

    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> {
        self.inner.withdrawals()
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.inner.parent_beacon_block_root()
    }
}

/// New type around the payload builder attributes type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BerachainPayloadBuilderAttributes(EthPayloadBuilderAttributes);

impl PayloadBuilderAttributes for BerachainPayloadBuilderAttributes {
    type RpcPayloadAttributes = BerachainPayloadAttributes;
    type Error = Infallible;

    fn try_new(
        parent: B256,
        attributes: BerachainPayloadAttributes,
        _version: u8,
    ) -> Result<Self, Infallible> {
        Ok(Self(EthPayloadBuilderAttributes::new(parent, attributes.inner)))
    }

    fn payload_id(&self) -> PayloadId {
        self.0.id
    }

    fn parent(&self) -> B256 {
        self.0.parent
    }

    fn timestamp(&self) -> u64 {
        self.0.timestamp
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.0.parent_beacon_block_root
    }

    fn suggested_fee_recipient(&self) -> Address {
        self.0.suggested_fee_recipient
    }

    fn prev_randao(&self) -> B256 {
        self.0.prev_randao
    }

    fn withdrawals(&self) -> &Withdrawals {
        &self.0.withdrawals
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[non_exhaustive]
pub struct BerachainEngineTypes;

impl PayloadTypes for BerachainEngineTypes {
    type ExecutionData = ExecutionData;
    type BuiltPayload = EthBuiltPayload;
    type PayloadAttributes = BerachainPayloadAttributes;
    type PayloadBuilderAttributes = BerachainPayloadBuilderAttributes;

    fn block_to_payload(
        block: SealedBlock<
            <<Self::BuiltPayload as api::BuiltPayload>::Primitives as api::NodePrimitives>::Block,
        >,
    ) -> ExecutionData {
        let (payload, sidecar) =
            ExecutionPayload::from_block_unchecked(block.hash(), &block.into_block());
        ExecutionData { payload, sidecar }
    }
}

impl EngineTypes for BerachainEngineTypes {
    type ExecutionPayloadEnvelopeV1 = ExecutionPayloadV1;
    type ExecutionPayloadEnvelopeV2 = ExecutionPayloadEnvelopeV2;
    type ExecutionPayloadEnvelopeV3 = ExecutionPayloadEnvelopeV3;
    type ExecutionPayloadEnvelopeV4 = ExecutionPayloadEnvelopeV4;
    type ExecutionPayloadEnvelopeV5 = ExecutionPayloadEnvelopeV5;
}

/// Custom engine validator
#[derive(Debug, Clone)]
pub struct BerachainEngineValidator {
    inner: EthereumExecutionPayloadValidator<BerachainChainSpec>,
}

impl BerachainEngineValidator {
    /// Instantiates a new validator.
    pub const fn new(chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner: EthereumExecutionPayloadValidator::new(chain_spec) }
    }

    /// Returns the chain spec used by the validator.
    #[inline]
    fn chain_spec(&self) -> &BerachainChainSpec {
        self.inner.chain_spec()
    }
}

impl PayloadValidator for BerachainEngineValidator {
    type Block = Block;
    type ExecutionData = ExecutionData;

    fn ensure_well_formed_payload(
        &self,
        payload: ExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        let sealed_block = self.inner.ensure_well_formed_payload(payload)?;
        sealed_block.try_recover().map_err(|e| NewPayloadError::Other(e.into()))
    }
}

impl<T> EngineValidator<T> for BerachainEngineValidator
where
    T: PayloadTypes<PayloadAttributes = BerachainPayloadAttributes, ExecutionData = ExecutionData>,
{
    fn validate_version_specific_fields(
        &self,
        version: EngineApiMessageVersion,
        payload_or_attrs: PayloadOrAttributes<'_, Self::ExecutionData, T::PayloadAttributes>,
    ) -> Result<(), EngineObjectValidationError> {
        validate_version_specific_fields(self.chain_spec(), version, payload_or_attrs)
    }

    fn ensure_well_formed_attributes(
        &self,
        version: EngineApiMessageVersion,
        attributes: &T::PayloadAttributes,
    ) -> Result<(), EngineObjectValidationError> {
        validate_version_specific_fields(
            self.chain_spec(),
            version,
            PayloadOrAttributes::<Self::ExecutionData, T::PayloadAttributes>::PayloadAttributes(
                attributes,
            ),
        )?;

        // custom validation logic - ensure that the bls pubkey is present if the fork is active
        if attributes.prev_validator_pubkey.is_none() &&
            self.chain_spec().is_prague1_active_at_timestamp(attributes.timestamp())
        {
            // TODO: Change error
            return Err(EngineObjectValidationError::UnsupportedFork)
        }

        Ok(())
    }
}

/// Custom engine validator builder
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct BerachainEngineValidatorBuilder;

impl<N> EngineValidatorBuilder<N> for BerachainEngineValidatorBuilder
where
    N: FullNodeComponents<
        Types: NodeTypes<
            Payload = BerachainEngineTypes,
            ChainSpec = BerachainChainSpec,
            Primitives = EthPrimitives,
        >,
    >,
{
    type Validator = BerachainEngineValidator;

    async fn build(self, ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::Validator> {
        Ok(BerachainEngineValidator::new(ctx.config.chain.clone()))
    }
}

/// The type responsible for building custom payloads
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BerachainPayloadBuilder<Pool, Client> {
    inner: reth_ethereum_payload_builder::EthereumPayloadBuilder<Pool, Client, BerachainEvmConfig>,
}

impl<Pool, Client> PayloadBuilder for BerachainPayloadBuilder<Pool, Client>
where
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec = BerachainChainSpec> + Clone,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TransactionSigned>>,
{
    type Attributes = BerachainPayloadBuilderAttributes;
    type BuiltPayload = EthBuiltPayload;

    fn try_build(
        &self,
        args: BuildArguments<Self::Attributes, Self::BuiltPayload>,
    ) -> Result<BuildOutcome<Self::BuiltPayload>, PayloadBuilderError> {
        let BuildArguments { cached_reads, config, cancel, best_payload } = args;
        let PayloadConfig { parent_header, attributes } = config;

        // This reuses the default EthereumPayloadBuilder to build the payload
        // but any custom logic can be implemented here
        self.inner.try_build(BuildArguments {
            cached_reads,
            config: PayloadConfig { parent_header, attributes: attributes.0 },
            cancel,
            best_payload,
        })
    }

    fn build_empty_payload(
        &self,
        config: PayloadConfig<Self::Attributes>,
    ) -> Result<Self::BuiltPayload, PayloadBuilderError> {
        let PayloadConfig { parent_header, attributes } = config;
        self.inner.build_empty_payload(PayloadConfig { parent_header, attributes: attributes.0 })
    }
}

/// A custom payload service builder that supports the custom engine types
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainPayloadBuilderBuilder;

impl<Node, Pool> PayloadBuilderBuilder<Node, Pool, BerachainEvmConfig>
    for BerachainPayloadBuilderBuilder
where
    Node: FullNodeTypes<
        Types: NodeTypes<
            Payload = BerachainEngineTypes,
            ChainSpec = BerachainChainSpec,
            Primitives = EthPrimitives,
        >,
    >,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TransactionSigned>>
        + Unpin
        + 'static,
{
    type PayloadBuilder = BerachainPayloadBuilder<Pool, Node::Provider>;

    async fn build_payload_builder(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
        evm_config: BerachainEvmConfig,
    ) -> eyre::Result<Self::PayloadBuilder> {
        let payload_builder = BerachainPayloadBuilder {
            inner: reth_ethereum_payload_builder::EthereumPayloadBuilder::new(
                ctx.provider().clone(),
                pool,
                evm_config,
                EthereumBuilderConfig::new(),
            ),
        };
        Ok(payload_builder)
    }
}
