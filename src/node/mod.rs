pub mod cli;
mod evm;
mod rpc;

use crate::chainspec::BerachainChainSpec;
use crate::node::evm::BerachainExecutorBuilder;
use crate::node::rpc::engine_api::BerachainEngineValidatorBuilder;
use crate::node::rpc::BerachainApiBuilder;
use reth::api::{BlockTy, FullNodeComponents, FullNodeTypes, NodeAddOns, NodeTypes};
use reth::revm::context::TxEnv;
use reth::rpc::api::eth::FromEvmError;
use reth::rpc::api::BlockSubmissionValidationApiServer;
use reth::rpc::builder::config::RethRpcServerConfig;
use reth::rpc::builder::RethRpcModule;
use reth::rpc::eth::FullEthApiServer;
use reth::rpc::server_types::eth::EthApiError;
use reth_evm::{ConfigureEvm, EvmFactory, EvmFactoryFor, NextBlockEnvAttributes};
use reth_node_api::AddOnsContext;
use reth_node_builder::components::{BasicPayloadServiceBuilder, ComponentsBuilder};
use reth_node_builder::rpc::{BasicEngineApiBuilder, EngineValidatorAddOn, EngineValidatorBuilder, RethRpcAddOns, RpcAddOns, RpcHandle};
use reth_node_builder::{DebugNode, Node, NodeAdapter, NodeComponentsBuilder};
use reth_node_ethereum::node::{EthereumConsensusBuilder, EthereumNetworkBuilder, EthereumPayloadBuilder, EthereumPoolBuilder};
use reth_node_ethereum::{EthereumEngineValidator, EthereumNode};
use reth_rpc::eth::EthApiFor;
use reth_rpc::ValidationApi;
use std::sync::Arc;

/// Type configuration for a regular Berachain node.

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainNode;

impl BerachainNode {
    pub fn components<Node>(
        &self,
    ) -> ComponentsBuilder<
        Node,
        EthereumPoolBuilder,
        BasicPayloadServiceBuilder<EthereumPayloadBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        EthereumConsensusBuilder,
    >
    where Node: FullNodeTypes<Types = Self>{
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(EthereumPoolBuilder::default())
            .executor(BerachainExecutorBuilder::default())
            .payload(BasicPayloadServiceBuilder::default())
            .network(EthereumNetworkBuilder::default())
            .consensus(EthereumConsensusBuilder::default())
    }
}

// Same as ETH Except we use BerachainChainSpec
impl NodeTypes for BerachainNode {
    type Primitives = <EthereumNode as NodeTypes>::Primitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = <EthereumNode as NodeTypes>::Storage;
    type Payload = <EthereumNode as NodeTypes>::Payload;
}

#[derive(Debug)]
pub struct BerachainAddOns<N: FullNodeComponents>
where
    EthApiFor<N>: FullEthApiServer<Provider = N::Provider, Pool = N::Pool>,
{
    inner: RpcAddOns<N, BerachainApiBuilder, BerachainEngineValidatorBuilder, BasicEngineApiBuilder<BerachainEngineValidatorBuilder>>,
}

impl<N> NodeAddOns<N> for BerachainAddOns<N>
where
    N: FullNodeComponents<
        Types: NodeTypes<
            ChainSpec = <BerachainNode as NodeTypes>::ChainSpec,
            StateCommitment = <BerachainNode as NodeTypes>::StateCommitment,
            Storage = <BerachainNode as NodeTypes>::Storage,
            Primitives = <BerachainNode as NodeTypes>::Primitives,
            Payload = <BerachainNode as NodeTypes>::Payload,
        >,
        Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
    >,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type Handle = RpcHandle<N, EthApiFor<N>>;

    async fn launch_add_ons(
        self,
        ctx: reth_node_api::AddOnsContext<'_, N>,
    ) -> eyre::Result<Self::Handle> {
        let validation_api = ValidationApi::new(
            ctx.node.provider().clone(),
            Arc::new(ctx.node.consensus().clone()),
            ctx.node.evm_config().clone(),
            ctx.config.rpc.flashbots_config(),
            Box::new(ctx.node.task_executor().clone()),
            Arc::new(EthereumEngineValidator::new(Arc::from(ctx.config.chain.inner().clone()))),
        );

        self.inner
            .launch_add_ons_with(ctx, move |container| {
                container.modules.merge_if_module_configured(
                    RethRpcModule::Flashbots,
                    validation_api.into_rpc(),
                )?;

                Ok(())
            })
            .await
    }
}

impl<N: FullNodeComponents> Default for BerachainAddOns<N>
where
    EthApiFor<N>: FullEthApiServer<Provider = N::Provider, Pool = N::Pool>,
{
    fn default() -> Self {
        Self { inner: Default::default() }
    }
}

impl<N> RethRpcAddOns<N> for BerachainAddOns<N>
where
    N: FullNodeComponents<
        Types: NodeTypes<
            ChainSpec = <BerachainNode as NodeTypes>::ChainSpec,
            StateCommitment = <BerachainNode as NodeTypes>::StateCommitment,
            Storage = <BerachainNode as NodeTypes>::Storage,
            Primitives = <BerachainNode as NodeTypes>::Primitives,
            Payload = <BerachainNode as NodeTypes>::Payload,
        >,
        Evm: ConfigureEvm<NextBlockEnvCtx = NextBlockEnvAttributes>,
    >,
    EthApiError: FromEvmError<N::Evm>,
    EvmFactoryFor<N::Evm>: EvmFactory<Tx = TxEnv>,
{
    type EthApi = EthApiFor<N>;

    fn hooks_mut(&mut self) -> &mut reth_node_builder::rpc::RpcHooks<N, Self::EthApi> {
        self.inner.hooks_mut()
    }
}

impl<N> EngineValidatorAddOn<N> for BerachainAddOns<N>
where
    N: FullNodeComponents<
        Types: NodeTypes<
            ChainSpec = <BerachainNode as NodeTypes>::ChainSpec,
            StateCommitment = <BerachainNode as NodeTypes>::StateCommitment,
            Storage = <BerachainNode as NodeTypes>::Storage,
            Primitives = <BerachainNode as NodeTypes>::Primitives,
            Payload = <BerachainNode as NodeTypes>::Payload,
        >,
    >,
    EthApiFor<N>: FullEthApiServer<Provider = N::Provider, Pool = N::Pool>,
{
    type Validator = EthereumEngineValidator;

    async fn engine_validator(&self, ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::Validator> {
        BerachainEngineValidatorBuilder::default().build(ctx).await
    }
}

impl<N> Node<N> for BerachainNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        EthereumPoolBuilder,
        BasicPayloadServiceBuilder<EthereumPayloadBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        EthereumConsensusBuilder,
    >;
    type AddOns = BerachainAddOns<
        NodeAdapter<N,<Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        Self::components(self)
    }

    fn add_ons(&self) -> Self::AddOns {
        BerachainAddOns::default()
    }
}


impl <N> DebugNode<N> for BerachainNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block;

    fn rpc_to_primitive_block(_rpc_block: Self::RpcBlock) -> BlockTy<Self> {
        todo!()
    }
}


