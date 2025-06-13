use jsonrpsee_core::server::RpcModule;
use std::sync::Arc;
use reth::api::{AddOnsContext, FullNodeComponents, NodeTypes};
use reth::primitives::EthPrimitives;
use reth::rpc::api::IntoEngineApiRpcModule;
use reth_chainspec::ChainSpec;
use reth_node_builder::rpc::{EngineApiBuilder, EngineValidatorBuilder};
use reth_node_ethereum::{EthEngineTypes, EthereumEngineValidator};
use crate::chainspec::BerachainChainSpec;
use crate::node::BerachainNode;

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainEngineValidatorBuilder;

impl<Node, Types> EngineValidatorBuilder<Node> for BerachainEngineValidatorBuilder
where
    Types:
        NodeTypes<
            ChainSpec = <BerachainNode as NodeTypes>::ChainSpec,
            Primitives = <BerachainNode as NodeTypes>::Primitives,
            Payload = <BerachainNode as NodeTypes>::Payload,
        >,
    Node: FullNodeComponents<Types = Types>,
{
    type Validator = EthereumEngineValidator;

    async fn build(self, ctx: &AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
        Ok(EthereumEngineValidator::new(Arc::from(ctx.config.chain.inner())))
    }
}

// #[derive(Debug, Default)]
// #[non_exhaustive]
// pub struct BerachainEngineApi;
//
// impl IntoEngineApiRpcModule for BerachainEngineApi {
//     fn into_rpc_module(self) -> RpcModule<()> {
//         todo!("implement")
//         RpcModule::new(())
//     }
// }
//
// #[derive(Debug, Default)]
// pub struct BerachainEngineApiBuilder;
//
// impl<N> EngineApiBuilder<N> for BerachainEngineApiBuilder
// where
//     N: FullNodeComponents,
// {
//     type EngineApi = BerachainEngineApi;
//
//     async fn build_engine_api(self, _ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::EngineApi> {
//         Ok(BerachainEngineApi::default())
//     }
// }

// /// Builder for [`EthereumEngineValidator`].
// #[derive(Debug, Default, Clone)]
// #[non_exhaustive]
// pub struct BerachainEngineValidatorBuilder;
//
// impl<Node, Types> EngineValidatorBuilder<Node> for BerachainEngineValidatorBuilder
// where
//     Types: NodeTypes<ChainSpec = BerachainChainSpec, Payload = EthEngineTypes, Primitives = EthPrimitives>,
//     Node: FullNodeComponents<Types = Types>,
// {
//     type Validator = EthereumEngineValidator;
//
//     async fn build(self, ctx: &AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
//         Ok(EthereumEngineValidator::new(ctx.config.chain.inner().clone().into()))
//     }
// }