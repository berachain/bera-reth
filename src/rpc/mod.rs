use reth_rpc_eth_api::helpers::config::EthConfigApiServer;
pub mod api;
pub mod bera_admin;
pub mod config;
pub mod receipt;

use crate::{
    chainspec::BerachainChainSpec,
    engine::{
        BerachainExecutionData, rpc::BerachainEngineApiBuilder,
        validator::BerachainEngineValidatorBuilder,
    },
    node::evm::config::{BerachainEvmConfig, BerachainNextBlockEnvAttributes},
    primitives::BerachainPrimitives,
    rpc::{
        api::{BerachainApi, BerachainNetwork},
        bera_admin::{BerAdminApiServer, BerAdminImpl},
        config::BerachainConfigHandler,
        receipt::BerachainEthReceiptConverter,
    },
};
use reth::{
    api::{FullNodeComponents, HeaderTy, PrimitivesTy},
    chainspec::EthereumHardforks,
    revm::context::TxEnv,
    rpc::{
        api::eth::FromEvmError,
        builder::{Identity, RethRpcModule},
        server_types::eth::EthApiError,
    },
};
use reth_chainspec::{ChainSpecProvider, EthChainSpec, Hardforks};
use reth_evm::{ConfigureEvm, EvmFactory, EvmFactoryFor};
use reth_node_api::{FullNodeTypes, NodeAddOns, NodeTypes};
use reth_node_builder::rpc::{
    BasicEngineValidatorBuilder, EngineApiBuilder, EngineValidatorAddOn, EngineValidatorBuilder,
    EthApiBuilder, EthApiCtx, PayloadValidatorBuilder, RethRpcAddOns, RethRpcMiddleware, RpcAddOns,
    RpcHandle,
};
use reth_rpc_convert::{RpcConvert, RpcConverter};
use reth_rpc_eth_api::helpers::pending_block::BuildPendingEnv;
use std::sync::Arc;

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
            Types: NodeTypes<
                ChainSpec: EthereumHardforks + Hardforks,
                Primitives = BerachainPrimitives,
            >,
            Evm: ConfigureEvm<NextBlockEnvCtx: BuildPendingEnv<HeaderTy<N::Types>>>,
        >,
    BerachainEthRpcConverterFor<N>: RpcConvert<
            Primitives = PrimitivesTy<N::Types>,
            Evm = N::Evm,
            Error = EthApiError,
            Network = BerachainNetwork,
        >,
    EthApiError: FromEvmError<N::Evm>,
{
    type EthApi = BerachainApi<N, BerachainEthRpcConverterFor<N>>;

    async fn build_eth_api(self, ctx: EthApiCtx<'_, N>) -> eyre::Result<Self::EthApi> {
        let tx_resp_builder = BerachainEthRpcConverterFor::<N>::new(
            BerachainEthReceiptConverter::new(ctx.components.provider().clone().chain_spec()),
        );

        let inner = ctx.eth_api_builder().with_rpc_converter(tx_resp_builder.clone()).build();

        Ok(BerachainApi { inner })
    }
}

/// Add-ons w.r.t. Berachain.
#[derive(Debug)]
pub struct BerachainAddOns<
    N: FullNodeComponents,
    EthB: EthApiBuilder<N>,
    PVB,
    EB = BerachainEngineApiBuilder<PVB>,
    EVB = BasicEngineValidatorBuilder<PVB>,
    RpcMiddleware = Identity,
> {
    inner: RpcAddOns<N, EthB, PVB, EB, EVB, RpcMiddleware>,
}

impl<N> Default for BerachainAddOns<N, BerachainEthApiBuilder, BerachainEngineValidatorBuilder>
where
    N: FullNodeComponents,
    BerachainEthApiBuilder: EthApiBuilder<N>,
{
    fn default() -> Self {
        Self {
            inner: RpcAddOns::new(
                BerachainEthApiBuilder,
                BerachainEngineValidatorBuilder::default(),
                BerachainEngineApiBuilder::<BerachainEngineValidatorBuilder>::default(),
                BasicEngineValidatorBuilder::new(BerachainEngineValidatorBuilder::default()),
                Default::default(),
            ),
        }
    }
}

impl<N, EthB, PVB, EB, EVB, RpcMiddleware> BerachainAddOns<N, EthB, PVB, EB, EVB, RpcMiddleware>
where
    N: FullNodeComponents,
    EthB: EthApiBuilder<N>,
{
    /// Replace the engine API builder.
    pub fn with_engine_api<T>(
        self,
        engine_api_builder: T,
    ) -> BerachainAddOns<N, EthB, PVB, T, EVB, RpcMiddleware>
    where
        T: Send,
    {
        let Self { inner } = self;
        BerachainAddOns { inner: inner.with_engine_api(engine_api_builder) }
    }

    /// Replace the payload validator builder.
    pub fn with_payload_validator<V, T>(
        self,
        payload_validator_builder: T,
    ) -> BerachainAddOns<N, EthB, T, EB, EVB, RpcMiddleware> {
        let Self { inner } = self;
        BerachainAddOns { inner: inner.with_payload_validator(payload_validator_builder) }
    }
}

impl<N, EthB, PVB, EB, EVB, RpcMiddleware> NodeAddOns<N>
    for BerachainAddOns<N, EthB, PVB, EB, EVB, RpcMiddleware>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec = BerachainChainSpec,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Provider: ChainSpecProvider<ChainSpec: EthereumHardforks>,
            Network: crate::rpc::bera_admin::PogCanarySend,
            Evm = BerachainEvmConfig,
        >,
    EthB: EthApiBuilder<N>,
    PVB: PayloadValidatorBuilder<N>,
    EB: EngineApiBuilder<N>,
    EVB: EngineValidatorBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
    RpcMiddleware: RethRpcMiddleware,
{
    type Handle = RpcHandle<N, EthB::EthApi>;

    async fn launch_add_ons(
        self,
        ctx: reth_node_api::AddOnsContext<'_, N>,
    ) -> eyre::Result<Self::Handle> {
        let berachain_config =
            BerachainConfigHandler::new(ctx.node.provider().clone(), ctx.node.evm_config().clone());

        if !crate::pog::pog_cli_enabled() {
            return self
                .inner
                .launch_add_ons_with(ctx, move |container| {
                    container.modules.merge_if_module_configured(
                        RethRpcModule::Eth,
                        berachain_config.into_rpc(),
                    )?;
                    Ok(())
                })
                .await;
        }

        let task_executor = ctx.node.task_executor().clone();
        let network = ctx.node.network().clone();
        let provider = ctx.node.provider().clone();
        let provider_watcher = provider.clone();
        let chain_spec = ctx.node.provider().chain_spec();
        let chain_id = chain_spec.chain().id();
        let datadir = ctx.config.datadir().data_dir().to_path_buf();
        let known_peers_file =
            ctx.config.network.persistent_peers_file(ctx.config.datadir().known_peers());
        crate::pog::configure_shutdown_peer_curation(datadir.clone(), known_peers_file);
        let client_version = std::env::var("BERA_RETH_P2P_CLIENT_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());

        let pog = Arc::new(
            crate::pog::PogCoordinator::new(datadir.clone(), chain_id)
                .map_err(|e| eyre::eyre!("PoG database init failed: {e}"))?,
        );
        let attribution = crate::pog::attribution_store();
        let bera_admin =
            Arc::new(BerAdminImpl::new(network, provider, chain_spec, client_version, pog.clone()));

        let canon_events = {
            use reth::providers::CanonStateSubscriptions as _;
            provider_watcher.subscribe_to_canonical_state()
        };
        let sealed_fact_cfg = crate::pog::sealed_fact_config();
        let provider_for_watcher = provider_watcher.clone();
        task_executor.spawn_with_graceful_shutdown_signal(move |shutdown| {
            let coord = pog.clone();
            let store = attribution.clone();
            let provider = provider_for_watcher;
            async move {
                crate::pog::run_pog_watcher(
                    shutdown,
                    coord,
                    store,
                    provider,
                    canon_events,
                    sealed_fact_cfg,
                )
                .await
            }
        });

        let bera_for_rpc = bera_admin.clone();
        self.inner
            .launch_add_ons_with(ctx, move |container| {
                container
                    .modules
                    .merge_if_module_configured(RethRpcModule::Eth, berachain_config.into_rpc())?;
                container
                    .modules
                    .merge_configured(BerAdminApiServer::into_rpc(bera_for_rpc.clone()))?;
                Ok(())
            })
            .await
    }
}

impl<N, EthB, PVB, EB, EVB> RethRpcAddOns<N> for BerachainAddOns<N, EthB, PVB, EB, EVB>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec = BerachainChainSpec,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Network: crate::rpc::bera_admin::PogCanarySend,
            Evm = BerachainEvmConfig,
        >,
    EthB: EthApiBuilder<N>,
    PVB: PayloadValidatorBuilder<N>,
    EB: EngineApiBuilder<N>,
    EVB: EngineValidatorBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type EthApi = EthB::EthApi;

    fn hooks_mut(&mut self) -> &mut reth_node_builder::rpc::RpcHooks<N, Self::EthApi> {
        self.inner.hooks_mut()
    }
}

impl<N, EthB, PVB, EB, EVB> EngineValidatorAddOn<N> for BerachainAddOns<N, EthB, PVB, EB, EVB>
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthChainSpec + EthereumHardforks,
                Primitives = BerachainPrimitives,
                Payload: reth_engine_primitives::EngineTypes<
                    ExecutionData = BerachainExecutionData,
                >,
            >,
            Network: crate::rpc::bera_admin::PogCanarySend,
            Evm: ConfigureEvm<NextBlockEnvCtx = BerachainNextBlockEnvAttributes>,
        >,
    EthB: EthApiBuilder<N>,
    PVB: Send,
    EB: EngineApiBuilder<N>,
    EVB: EngineValidatorBuilder<N>,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type ValidatorBuilder = EVB;

    fn engine_validator_builder(&self) -> Self::ValidatorBuilder {
        self.inner.engine_validator_builder()
    }
}
