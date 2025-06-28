use crate::engine::payload::{BeraPayloadAttributes, BeraPayloadBuilderAttributes};
use reth::{
    api::{FullNodeTypes, NodeTypes, PayloadBuilderError, PayloadTypes, PrimitivesTy, TxTy},
    chainspec::EthereumHardforks,
    primitives::{EthPrimitives, TransactionSigned},
    providers::StateProviderFactory,
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, MissingPayloadBehaviour, PayloadBuilder, PayloadConfig,
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_ethereum_engine_primitives::{EthBuiltPayload, EthPayloadBuilderAttributes};
use reth_ethereum_payload_builder::{EthereumBuilderConfig, default_ethereum_payload};
use reth_evm::{ConfigureEvm, NextBlockEnvAttributes};
use reth_evm_ethereum::EthEvmConfig;
use reth_node_builder::{BuilderContext, PayloadBuilderConfig, components::PayloadBuilderBuilder};

#[derive(Clone, Default, Debug)]
#[non_exhaustive]
pub struct BerachainPayloadBuilder;

impl<Types, Node, Pool, Evm> PayloadBuilderBuilder<Node, Pool, Evm> for BerachainPayloadBuilder
where
    Types: NodeTypes<ChainSpec: EthereumHardforks, Primitives = EthPrimitives>,
    Node: FullNodeTypes<Types = Types>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
        + Unpin
        + 'static,
    Evm: ConfigureEvm<
            Primitives = PrimitivesTy<Types>,
            NextBlockEnvCtx = reth_evm::NextBlockEnvAttributes,
        > + 'static,
    Types::Payload: PayloadTypes<
            BuiltPayload = EthBuiltPayload,
            PayloadAttributes = BeraPayloadAttributes,
            PayloadBuilderAttributes = BeraPayloadBuilderAttributes,
        >,
{
    type PayloadBuilder = BerachainPayloadBuilderTODO<Pool, Node::Provider, Evm>;

    async fn build_payload_builder(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
        evm_config: Evm,
    ) -> eyre::Result<Self::PayloadBuilder> {
        let conf = ctx.payload_builder_config();
        let chain = ctx.chain_spec().chain();
        let gas_limit = conf.gas_limit_for(chain);

        Ok(BerachainPayloadBuilderTODO::new(
            ctx.provider().clone(),
            pool,
            evm_config,
            EthereumBuilderConfig::new().with_gas_limit(gas_limit),
        ))
    }
}

/// =====

#[derive(Debug, Clone, PartialEq, Eq)]
// TODO rename
pub struct BerachainPayloadBuilderTODO<Pool, Client, EvmConfig = EthEvmConfig> {
    /// Client providing access to node state.
    client: Client,
    /// Transaction pool.
    pool: Pool,
    /// The type responsible for creating the evm.
    evm_config: EvmConfig,
    /// Payload builder configuration.
    builder_config: EthereumBuilderConfig,
}

impl<Pool, Client, EvmConfig> BerachainPayloadBuilderTODO<Pool, Client, EvmConfig> {
    /// `EthereumPayloadBuilder` constructor.
    pub const fn new(
        client: Client,
        pool: Pool,
        evm_config: EvmConfig,
        builder_config: EthereumBuilderConfig,
    ) -> Self {
        Self { client, pool, evm_config, builder_config }
    }
}

// Default implementation of [PayloadBuilder] for unit type
impl<Pool, Client, EvmConfig> PayloadBuilder
    for BerachainPayloadBuilderTODO<Pool, Client, EvmConfig>
where
    EvmConfig: ConfigureEvm<Primitives = EthPrimitives, NextBlockEnvCtx = NextBlockEnvAttributes>,
    Client: StateProviderFactory + ChainSpecProvider<ChainSpec: EthereumHardforks> + Clone,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TransactionSigned>>,
{
    type Attributes = BeraPayloadBuilderAttributes;
    type BuiltPayload = EthBuiltPayload;

    fn try_build(
        &self,
        args: BuildArguments<Self::Attributes, EthBuiltPayload>,
    ) -> Result<BuildOutcome<EthBuiltPayload>, PayloadBuilderError> {
        let eth_config = PayloadConfig {
            parent_header: args.config.parent_header,
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

        default_ethereum_payload(
            self.evm_config.clone(),
            self.client.clone(),
            self.pool.clone(),
            self.builder_config.clone(),
            eth_args,
            |attributes| self.pool.best_transactions_with_attributes(attributes),
        )
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
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        let eth_config = PayloadConfig {
            parent_header: config.parent_header,
            attributes: EthPayloadBuilderAttributes::new(
                config.attributes.parent,
                config.attributes.to_eth_payload_attributes(),
            ),
        };

        let args: BuildArguments<EthPayloadBuilderAttributes, EthBuiltPayload> =
            BuildArguments::new(Default::default(), eth_config, Default::default(), None);

        default_ethereum_payload(
            self.evm_config.clone(),
            self.client.clone(),
            self.pool.clone(),
            self.builder_config.clone(),
            args,
            |attributes| self.pool.best_transactions_with_attributes(attributes),
        )?
        .into_payload()
        .ok_or_else(|| PayloadBuilderError::MissingPayload)
    }
}
