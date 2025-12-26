use jsonrpsee::client_transport::ws::Url;
use reth_rpc_eth_api::helpers::config::EthConfigApiServer;
use std::sync::Arc;
pub mod api;
pub mod config;
pub mod receipt;

use crate::{
    chainspec::BerachainChainSpec,
    engine::{
        BerachainExecutionData, rpc::BerachainEngineApiBuilder,
        validator::BerachainEngineValidatorBuilder,
    },
    flashblocks::{BerachainFlashblockPayload, BerachainFlashblockPayloadBase},
    node::evm::config::{BerachainEvmConfig, BerachainNextBlockEnvAttributes},
    primitives::BerachainPrimitives,
    rpc::{
        api::{BerachainApi, BerachainNetwork},
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
use reth_optimism_flashblocks::{FlashBlockService, FlashblocksListeners, WsFlashBlockStream};
use reth_optimism_rpc::OpRpcTypes;
use reth_rpc_convert::{RpcConvert, RpcConverter};
use reth_rpc_eth_api::helpers::pending_block::BuildPendingEnv;
use tokio::sync::watch;
use tracing::info;

/// Builds `BerachainEthApi` for Berachain.
#[derive(Clone, Default)]
pub struct BerachainEthApiBuilder {
    /// A URL pointing to a secure websocket connection (wss) that streams out flashblocks.
    flashblocks_url: Option<Url>,
    /// Pre-built flashblocks listeners (for testing/dependency injection).
    /// Wrapped in Arc for clonability.
    flashblocks_listeners:
        Option<Arc<FlashblocksListeners<BerachainPrimitives, BerachainFlashblockPayload>>>,
}

impl std::fmt::Debug for BerachainEthApiBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BerachainEthApiBuilder")
            .field("flashblocks_url", &self.flashblocks_url)
            .field("has_flashblocks_listeners", &self.flashblocks_listeners.is_some())
            .finish()
    }
}

impl BerachainEthApiBuilder {
    /// Configure flashblocks with a WebSocket URL.
    pub fn with_flashblocks_url(mut self, url: Option<Url>) -> Self {
        self.flashblocks_url = url;
        self
    }

    /// Configure flashblocks with pre-built listeners.
    ///
    /// This is primarily useful for testing, where you can build `FlashblocksListeners`
    /// from a custom stream source instead of connecting to a real WebSocket server.
    pub fn with_flashblocks_listeners(
        mut self,
        listeners: FlashblocksListeners<BerachainPrimitives, BerachainFlashblockPayload>,
    ) -> Self {
        self.flashblocks_listeners = Some(Arc::new(listeners));
        self
    }
}

pub type BerachainEthRpcConverterFor<N> = RpcConverter<
    BerachainNetwork,
    <N as FullNodeComponents>::Evm,
    BerachainEthReceiptConverter<<<N as FullNodeTypes>::Provider as ChainSpecProvider>::ChainSpec>,
>;

impl OpRpcTypes for BerachainNetwork {
    type Flashblock = BerachainFlashblockPayload;
}

impl<N> EthApiBuilder<N> for BerachainEthApiBuilder
where
    N: FullNodeComponents<
            Types: NodeTypes<
                ChainSpec: EthereumHardforks + Hardforks,
                Primitives = BerachainPrimitives,
            >,
            Evm: ConfigureEvm<
                NextBlockEnvCtx: BuildPendingEnv<HeaderTy<N::Types>>
                                     + From<BerachainFlashblockPayloadBase>
                                     + Unpin,
            >,
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
        if self.flashblocks_listeners.is_some() && self.flashblocks_url.is_some() {
            return Err(eyre::eyre!(
                "Cannot configure both flashblocks_listeners and flashblocks_url. \
                 Use flashblocks_listeners for testing or flashblocks_url for production."
            ));
        }

        let tx_resp_builder = BerachainEthRpcConverterFor::<N>::new(
            BerachainEthReceiptConverter::new(ctx.components.provider().clone().chain_spec()),
        );

        let flashblocks = if let Some(listeners) = self.flashblocks_listeners {
            info!(target: "bera-reth:rpc", "Using pre-built flashblocks listeners");
            Some(listeners)
        } else if let Some(ws_url) = self.flashblocks_url {
            info!(target: "bera-reth:rpc", %ws_url, "Launching flashblocks service");

            let (tx, pending_rx) = watch::channel(None);
            let stream: WsFlashBlockStream<_, _, _, BerachainFlashblockPayload> =
                WsFlashBlockStream::new(ws_url);
            let service = FlashBlockService::new(
                stream,
                ctx.components.evm_config().clone(),
                ctx.components.provider().clone(),
                ctx.components.task_executor().clone(),
                false,
            );

            let flashblocks_sequence = service.block_sequence_broadcaster().clone();
            let received_flashblocks = service.flashblocks_broadcaster().clone();
            let in_progress_rx = service.subscribe_in_progress();
            ctx.components.task_executor().spawn(Box::pin(service.run(tx)));

            Some(Arc::new(FlashblocksListeners::new(
                pending_rx,
                flashblocks_sequence,
                in_progress_rx,
                received_flashblocks,
            )))
        } else {
            None
        };

        let inner = ctx.eth_api_builder().with_rpc_converter(tx_resp_builder.clone()).build();

        Ok(BerachainApi { inner, flashblocks })
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
        Self::new(BerachainEthApiBuilder::default())
    }
}

impl<N> BerachainAddOns<N, BerachainEthApiBuilder, BerachainEngineValidatorBuilder>
where
    N: FullNodeComponents,
    BerachainEthApiBuilder: EthApiBuilder<N>,
{
    /// Creates new Berachain add-ons with a custom EthApiBuilder.
    ///
    /// This is useful for testing, where you can inject pre-built flashblocks listeners:
    /// ```ignore
    /// let eth_api_builder = BerachainEthApiBuilder::default()
    ///     .with_flashblocks_listeners(listeners);
    /// let add_ons = BerachainAddOns::new(eth_api_builder);
    /// ```
    pub fn new(eth_api_builder: BerachainEthApiBuilder) -> Self {
        Self {
            inner: RpcAddOns::new(
                eth_api_builder,
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

        self.inner
            .launch_add_ons_with(ctx, move |container| {
                container
                    .modules
                    .merge_if_module_configured(RethRpcModule::Eth, berachain_config.into_rpc())?;
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
