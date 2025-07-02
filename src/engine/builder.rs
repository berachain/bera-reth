use crate::{
    chainspec::BerachainChainSpec,
    engine::payload::{
        BerachainBuiltPayload, BerachainPayloadAttributes, BerachainPayloadBuilderAttributes,
    },
    primitives::BerachainPrimitives,
    transaction::BerachainTxEnvelope,
};
use reth::{
    api::{FullNodeTypes, NodeTypes, PayloadBuilderError, PayloadTypes, PrimitivesTy, TxTy},
    providers::StateProviderFactory,
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, MissingPayloadBehaviour, PayloadBuilder, PayloadConfig,
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_ethereum_engine_primitives::{EthBuiltPayload, EthPayloadBuilderAttributes};
use reth_ethereum_payload_builder::EthereumBuilderConfig;
use reth_evm::{ConfigureEvm, NextBlockEnvAttributes};
use reth_evm_ethereum::EthEvmConfig;
use reth_node_builder::{BuilderContext, PayloadBuilderConfig, components::PayloadBuilderBuilder};

/// Service builder for creating Berachain payload builders
///
/// This component integrates with the Reth node builder system to provide
/// a Berachain-specific payload service that handles the conversion between
/// Berachain payload attributes and Ethereum payload building logic.
#[derive(Clone, Default, Debug)]
#[non_exhaustive]
pub struct BerachainPayloadServiceBuilder;

impl<Types, Node, Pool, Evm> PayloadBuilderBuilder<Node, Pool, Evm>
    for BerachainPayloadServiceBuilder
where
    Types: NodeTypes<ChainSpec = BerachainChainSpec, Primitives = BerachainPrimitives>,
    Node: FullNodeTypes<Types = Types>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
        + Unpin
        + 'static,
    Evm: ConfigureEvm<
            Primitives = PrimitivesTy<Types>,
            NextBlockEnvCtx = reth_evm::NextBlockEnvAttributes,
        > + 'static,
    Types::Payload: PayloadTypes<
            BuiltPayload = BerachainBuiltPayload,
            PayloadAttributes = BerachainPayloadAttributes,
            PayloadBuilderAttributes = BerachainPayloadBuilderAttributes,
        >,
{
    type PayloadBuilder = BerachainPayloadBuilder<Pool, Node::Provider, Evm>;

    async fn build_payload_builder(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
        evm_config: Evm,
    ) -> eyre::Result<Self::PayloadBuilder> {
        let conf = ctx.payload_builder_config();
        let chain = ctx.chain_spec().chain();
        let gas_limit = conf.gas_limit_for(chain);

        Ok(BerachainPayloadBuilder::new(
            ctx.provider().clone(),
            pool,
            evm_config,
            EthereumBuilderConfig::new().with_gas_limit(gas_limit),
        ))
    }
}

/// Berachain-specific payload builder implementation
///
/// This payload builder handles Berachain-specific payload attributes while
/// delegating the actual payload building to the proven Ethereum implementation.
/// It provides the necessary type conversions and maintains compatibility
/// with Berachain's chain specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BerachainPayloadBuilder<Pool, Client, EvmConfig = EthEvmConfig> {
    /// Client providing access to node state
    client: Client,
    /// Transaction pool
    pool: Pool,
    /// The type responsible for creating the evm
    evm_config: EvmConfig,
    /// Payload builder configuration
    builder_config: EthereumBuilderConfig,
}

impl<Pool, Client, EvmConfig> BerachainPayloadBuilder<Pool, Client, EvmConfig> {
    /// Create a new Berachain payload builder
    pub const fn new(
        client: Client,
        pool: Pool,
        evm_config: EvmConfig,
        builder_config: EthereumBuilderConfig,
    ) -> Self {
        Self { client, pool, evm_config, builder_config }
    }
}

impl<Pool, Client, EvmConfig> PayloadBuilder for BerachainPayloadBuilder<Pool, Client, EvmConfig>
where
    EvmConfig:
        ConfigureEvm<Primitives = BerachainPrimitives, NextBlockEnvCtx = NextBlockEnvAttributes>,
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec = BerachainChainSpec> + Clone,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = BerachainTxEnvelope>>,
{
    type Attributes = BerachainPayloadBuilderAttributes;
    type BuiltPayload = BerachainBuiltPayload;

    fn try_build(
        &self,
        args: BuildArguments<Self::Attributes, BerachainBuiltPayload>,
    ) -> Result<BuildOutcome<BerachainBuiltPayload>, PayloadBuilderError> {
        let eth_config = PayloadConfig {
            parent_header: args.config.parent_header,
            // TODO: Convert BerachainPayloadBuilderAttributes to EthPayloadBuilderAttributes for
            // compatibility
            attributes: EthPayloadBuilderAttributes::new(
                args.config.attributes.parent,
                args.config.attributes.to_eth_payload_attributes(),
            ),
        };

        let eth_args = BuildArguments {
            cached_reads: args.cached_reads,
            config: eth_config,
            cancel: args.cancel,
            best_payload: args.best_payload,
        };

        todo!()
        // default_ethereum_payload(
        //     self.evm_config.clone(),
        //     self.client.clone(),
        //     self.pool.clone(),
        //     self.builder_config.clone(),
        //     eth_args,
        //     |attributes| self.pool.best_transactions_with_attributes(attributes),
        // )
    }

    fn on_missing_payload(
        &self,
        _args: BuildArguments<Self::Attributes, Self::BuiltPayload>,
    ) -> MissingPayloadBehaviour<Self::BuiltPayload> {
        if self.builder_config.await_payload_on_missing {
            MissingPayloadBehaviour::AwaitInProgress
        } else {
            MissingPayloadBehaviour::RaceEmptyPayload
        }
    }

    fn build_empty_payload(
        &self,
        config: PayloadConfig<Self::Attributes>,
    ) -> Result<BerachainBuiltPayload, PayloadBuilderError> {
        let eth_config = PayloadConfig {
            parent_header: config.parent_header,
            attributes: EthPayloadBuilderAttributes::new(
                config.attributes.parent,
                config.attributes.to_eth_payload_attributes(),
            ),
        };

        let args: BuildArguments<EthPayloadBuilderAttributes, BerachainBuiltPayload> =
            BuildArguments::new(Default::default(), eth_config, Default::default(), None);

        todo!()
        // default_ethereum_payload(
        //     self.evm_config.clone(),
        //     self.client.clone(),
        //     self.pool.clone(),
        //     self.builder_config.clone(),
        //     args,
        //     |attributes| self.pool.best_transactions_with_attributes(attributes),
        // )?
        // .into_payload()
        // .ok_or_else(|| PayloadBuilderError::MissingPayload)
    }
}
