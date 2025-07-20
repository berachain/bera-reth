mod api;
mod receipt;

use crate::{
    engine::{BerachainExecutionData, rpc::BerachainEngineApiBuilder},
    node::evm::config::BerachainNextBlockEnvAttributes,
    primitives::BerachainPrimitives,
    rpc::{
        api::{BerachainApi, BerachainNetwork},
        receipt::BerachainEthReceiptConverter,
    },
};
use reth::{
    api::{FullNodeComponents, HeaderTy, PrimitivesTy},
    chainspec::EthereumHardforks,
    revm::context::TxEnv,
    rpc::{api::eth::FromEvmError, server_types::eth::EthApiError},
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec};
use reth_evm::{ConfigureEvm, EvmFactory, EvmFactoryFor, TxEnvFor};
use reth_node_api::{AddOnsContext, FullNodeTypes, NodeAddOns, NodeTypes};
use reth_node_builder::rpc::{
    EngineApiBuilder, EngineValidatorAddOn, EngineValidatorBuilder, EthApiBuilder, EthApiCtx,
    RethRpcAddOns, RpcAddOns, RpcHandle,
};
use reth_rpc_convert::{RpcConvert, RpcConverter};
use reth_rpc_eth_api::helpers::pending_block::BuildPendingEnv;

/// Builds `BerachainEthApi` for Berachain.
#[derive(Debug, Default)]
pub struct BerachainEthApiBuilder;

pub type BerachainEthRpcConverterFor<N> = RpcConverter<
    BerachainNetwork,
    <N as FullNodeComponents>::Evm,
    BerachainEthReceiptConverter<<<N as FullNodeTypes>::Provider as ChainSpecProvider>::ChainSpec>,
>;

impl<N> EthApiBuilder<N> for BerachainEthApiBuilder
where
    N: FullNodeComponents<
            Types: NodeTypes<ChainSpec: EthereumHardforks, Primitives = BerachainPrimitives>,
            Evm: ConfigureEvm<NextBlockEnvCtx: BuildPendingEnv<HeaderTy<N::Types>>>,
        >,
    BerachainEthRpcConverterFor<N>: RpcConvert<
            Primitives = PrimitivesTy<N::Types>,
            TxEnv = TxEnvFor<N::Evm>,
            Error = EthApiError,
            Network = BerachainNetwork,
        >,
    EthApiError: FromEvmError<N::Evm>,
{
    type EthApi = BerachainApi<
        <N as FullNodeTypes>::Provider,
        <N as FullNodeComponents>::Pool,
        <N as FullNodeComponents>::Network,
        <N as FullNodeComponents>::Evm,
        BerachainEthRpcConverterFor<N>,
    >;

    async fn build_eth_api(self, ctx: EthApiCtx<'_, N>) -> eyre::Result<Self::EthApi> {
        let tx_resp_builder = BerachainEthRpcConverterFor::<N>::new(
            BerachainEthReceiptConverter::new(ctx.components.provider().clone().chain_spec()),
            (),
        );

        let api = reth_rpc::EthApiBuilder::new(
            ctx.components.provider().clone(),
            ctx.components.pool().clone(),
            ctx.components.network().clone(),
            ctx.components.evm_config().clone(),
        )
        .with_rpc_converter(tx_resp_builder.clone())
        .eth_cache(ctx.cache)
        .task_spawner(ctx.components.task_executor().clone())
        .gas_cap(ctx.config.rpc_gas_cap.into())
        .max_simulate_blocks(ctx.config.rpc_max_simulate_blocks)
        .eth_proof_window(ctx.config.eth_proof_window)
        .fee_history_cache_config(ctx.config.fee_history_cache)
        .proof_permits(ctx.config.proof_permits)
        .gas_oracle_config(ctx.config.gas_oracle)
        .build();

        Ok(BerachainApi { inner: api })
    }
}

/// Add-ons w.r.t. Berachain.
#[derive(Debug)]
pub struct BerachainAddOns<
    N: FullNodeComponents,
    EthB: EthApiBuilder<N>,
    EV,
    EB = BerachainEngineApiBuilder<EV>,
> {
    inner: RpcAddOns<N, EthB, EV, EB>,
}

impl<N> Default
    for BerachainAddOns<
        N,
        BerachainEthApiBuilder,
        crate::engine::validator::BerachainEngineValidatorBuilder,
    >
where
    N: FullNodeComponents,
    BerachainEthApiBuilder: EthApiBuilder<N>,
{
    fn default() -> Self {
        Self {
            inner: RpcAddOns::new(
                BerachainEthApiBuilder,
                crate::engine::validator::BerachainEngineValidatorBuilder::default(),
                BerachainEngineApiBuilder::default(),
                Default::default(),
            ),
        }
    }
}

impl<N, EthB, EV, EB> BerachainAddOns<N, EthB, EV, EB>
where
    N: FullNodeComponents,
    EthB: EthApiBuilder<N>,
{
    /// Replace the engine API builder.
    pub fn with_engine_api<T>(self, engine_api_builder: T) -> BerachainAddOns<N, EthB, EV, T>
    where
        T: Send,
    {
        let Self { inner } = self;
        BerachainAddOns { inner: inner.with_engine_api(engine_api_builder) }
    }

    /// Replace the engine validator builder.
    pub fn with_engine_validator<T>(
        self,
        engine_validator_builder: T,
    ) -> BerachainAddOns<N, EthB, T, EB>
    where
        T: Send,
    {
        let Self { inner } = self;
        BerachainAddOns { inner: inner.with_engine_validator(engine_validator_builder) }
    }
}

impl<N, EthB, EV, EB> NodeAddOns<N> for BerachainAddOns<N, EthB, EV, EB>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthChainSpec + EthereumHardforks,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx = BerachainNextBlockEnvAttributes>,
        >,
    EthB: EthApiBuilder<N>,
    EV: EngineValidatorBuilder<N>,
    EB: EngineApiBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type Handle = RpcHandle<N, EthB::EthApi>;

    async fn launch_add_ons(
        self,
        ctx: reth_node_api::AddOnsContext<'_, N>,
    ) -> eyre::Result<Self::Handle> {
        self.inner.launch_add_ons(ctx).await
    }
}

impl<N, EthB, EV, EB> RethRpcAddOns<N> for BerachainAddOns<N, EthB, EV, EB>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthChainSpec + EthereumHardforks,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx = BerachainNextBlockEnvAttributes>,
        >,
    EthB: EthApiBuilder<N>,
    EV: EngineValidatorBuilder<N>,
    EB: EngineApiBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type EthApi = EthB::EthApi;

    fn hooks_mut(&mut self) -> &mut reth_node_builder::rpc::RpcHooks<N, Self::EthApi> {
        self.inner.hooks_mut()
    }
}

impl<N, EthB, EV, EB> EngineValidatorAddOn<N> for BerachainAddOns<N, EthB, EV, EB>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthChainSpec + EthereumHardforks,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx = BerachainNextBlockEnvAttributes>,
        >,
    EthB: EthApiBuilder<N>,
    EV: EngineValidatorBuilder<N>,
    EB: EngineApiBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type Validator = EV::Validator;

    async fn engine_validator(&self, ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::Validator> {
        self.inner.engine_validator(ctx).await
    }
}
