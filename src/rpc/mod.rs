mod api;
mod receipt;

use crate::{
    chainspec::BerachainChainSpec,
    engine::BerachainEngineTypes,
    node::evm::config::BerachainNextBlockEnvAttributes,
    primitives::BerachainPrimitives,
    rpc::api::{BerachainApi, BerachainNetwork},
};
use alloy_rpc_types::engine::ExecutionData;
use reth::{
    api::FullNodeComponents,
    chainspec::EthereumHardforks,
    revm::context::TxEnv,
    rpc::{api::eth::FromEvmError, compat::TxInfoMapper, server_types::eth::EthApiError},
};
use reth_chainspec::EthChainSpec;
use reth_evm::{ConfigureEvm, EvmFactory, EvmFactoryFor};
use reth_node_api::{AddOnsContext, FullNodeTypes, NodeAddOns, NodeTypes};
use reth_node_builder::rpc::{
    BasicEngineApiBuilder, EngineApiBuilder, EngineValidatorAddOn, EngineValidatorBuilder,
    EthApiBuilder, EthApiCtx, RethRpcAddOns, RpcAddOns, RpcHandle,
};

/// Builds [`BerachainEthApi`] for Berachain.
#[derive(Debug, Default)]
pub struct BerachainEthApiBuilder;

impl<N> EthApiBuilder<N> for BerachainEthApiBuilder
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec = BerachainChainSpec,
                Primitives = BerachainPrimitives,
                Payload = BerachainEngineTypes,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx = BerachainNextBlockEnvAttributes>,
        >,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type EthApi = BerachainApi<
        <N as FullNodeTypes>::Provider,
        <N as FullNodeComponents>::Pool,
        <N as FullNodeComponents>::Network,
        <N as FullNodeComponents>::Evm,
        BerachainNetwork,
    >;

    async fn build_eth_api(self, ctx: EthApiCtx<'_, N>) -> eyre::Result<Self::EthApi> {
        let api = reth_rpc::EthApiBuilder::new(
            ctx.components.provider().clone(),
            ctx.components.pool().clone(),
            ctx.components.network().clone(),
            ctx.components.evm_config().clone(),
        )
        .eth_cache(ctx.cache)
        .task_spawner(ctx.components.task_executor().clone())
        .gas_cap(ctx.config.rpc_gas_cap.into())
        .max_simulate_blocks(ctx.config.rpc_max_simulate_blocks)
        .eth_proof_window(ctx.config.eth_proof_window)
        .fee_history_cache_config(ctx.config.fee_history_cache)
        .proof_permits(ctx.config.proof_permits)
        .gas_oracle_config(ctx.config.gas_oracle)
        .build();

        Ok(BerachainApi { inner: api, tx_resp_builder: Default::default() })
    }
}

/// Add-ons w.r.t. Berachain.
#[derive(Debug)]
pub struct BerachainAddOns<
    N: FullNodeComponents,
    EthB: EthApiBuilder<N>,
    EV,
    EB = BasicEngineApiBuilder<EV>,
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
                BasicEngineApiBuilder::default(),
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
                Payload: reth_engine_primitives::EngineTypes<ExecutionData = ExecutionData>,
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
                Payload: reth_engine_primitives::EngineTypes<ExecutionData = ExecutionData>,
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
                Payload: reth_engine_primitives::EngineTypes<ExecutionData = ExecutionData>,
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
