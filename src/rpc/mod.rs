use crate::{
    engine::builder::BerachainPayloadBuilder, pool::BerachainPool, primitives::BerachainPrimitives,
};
use alloy_network::Ethereum;
use alloy_rpc_types::engine::ExecutionData;
use reth::{
    api::FullNodeComponents,
    chainspec::EthereumHardforks,
    providers::BlockReader,
    revm::context::TxEnv,
    rpc::{
        api::eth::FromEvmError,
        eth::{EthApiFor, EthApiTypes, RpcNodeCore, helpers::types::EthRpcConverter},
        server_types::eth::EthApiError,
    },
};
use reth_chainspec::EthChainSpec;
use reth_evm::{ConfigureEvm, EvmFactory, EvmFactoryFor, NextBlockEnvAttributes};
use reth_evm_ethereum::EthEvmConfig;
use reth_node_api::{AddOnsContext, NodeAddOns, NodeTypes};
use reth_node_builder::rpc::{
    BasicEngineApiBuilder, EngineApiBuilder, EngineValidatorAddOn, EngineValidatorBuilder,
    EthApiBuilder, EthApiCtx, RethRpcAddOns, RpcAddOns, RpcHandle,
};
use std::{fmt, future::Future};

/// Builds [`BerachainEthApi`] for Berachain.
#[derive(Debug, Default)]
pub struct BerachainEthApiBuilder;

impl<N> EthApiBuilder<N> for BerachainEthApiBuilder
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthChainSpec + EthereumHardforks,
                Primitives = BerachainPrimitives,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
        >,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type EthApi = BerachainEthApi<N>;

    fn build_eth_api(
        self,
        ctx: EthApiCtx<'_, N>,
    ) -> impl Future<Output = eyre::Result<Self::EthApi>> + Send {
        async move { todo!() }
    }
}

pub trait BerachainNodeCore: RpcNodeCore<Provider: BlockReader> {}

impl<T> BerachainNodeCore for T where T: RpcNodeCore<Provider: BlockReader> {}

impl<N: BerachainNodeCore> fmt::Debug for BerachainEthApi<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BscEthApi").finish_non_exhaustive()
    }
}

impl<N> Clone for BerachainEthApi<N>
where
    N: BerachainNodeCore,
{
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<N> RpcNodeCore for BerachainEthApi<N>
where
    N: BerachainNodeCore,
{
    type Primitives = BerachainPrimitives;
    type Provider = N::Provider;
    type Pool = N::Pool;
    type Evm = <N as RpcNodeCore>::Evm;
    type Network = <N as RpcNodeCore>::Network;
    type PayloadBuilder = BerachainPayloadBuilder<Self::Pool, Self::Provider>;

    fn pool(&self) -> &Self::Pool {
        todo!()
    }

    fn evm_config(&self) -> &Self::Evm {
        todo!()
    }

    fn network(&self) -> &Self::Network {
        todo!()
    }

    fn payload_builder(&self) -> &Self::PayloadBuilder {
        todo!()
    }

    fn provider(&self) -> &Self::Provider {
        todo!()
    }
}

struct BerachainEthApi<N: BerachainNodeCore> {
    _core: std::marker::PhantomData<N>,
}

impl<N> EthApiTypes for BerachainEthApi<N>
where
    N: BerachainNodeCore,
{
    type Error = EthApiError;
    type NetworkTypes = Ethereum;
    type RpcConvert = EthRpcConverter;

    fn tx_resp_builder(&self) -> &Self::RpcConvert {
        todo!()
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
            Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
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
            Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
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
            Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
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
