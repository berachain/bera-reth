pub mod cli;
mod evm;

use crate::{chainspec::BerachainChainSpec, node::evm::BerachainExecutorBuilder};
use reth::api::{BlockTy, FullNodeComponents, FullNodeTypes, NodeTypes};
use reth_node_builder::{
    DebugNode, Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
    rpc::BasicEngineApiBuilder,
};
use reth_node_ethereum::{
    EthereumAddOns, EthereumEngineValidatorBuilder, EthereumEthApiBuilder, EthereumNode,
    node::{
        EthereumConsensusBuilder, EthereumNetworkBuilder, EthereumPayloadBuilder,
        EthereumPoolBuilder,
    },
};

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
    where
        Node: FullNodeTypes<Types = Self>,
    {
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(EthereumPoolBuilder::default())
            .executor(BerachainExecutorBuilder)
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
    type AddOns = EthereumAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
        EthereumEthApiBuilder,
        EthereumEngineValidatorBuilder<BerachainChainSpec>,
        BasicEngineApiBuilder<EthereumEngineValidatorBuilder<BerachainChainSpec>>,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        Self::components(self)
    }

    fn add_ons(&self) -> Self::AddOns {
        EthereumAddOns::default()
            .with_engine_validator(EthereumEngineValidatorBuilder::<BerachainChainSpec>::default())
            .with_engine_api(BasicEngineApiBuilder::<
                EthereumEngineValidatorBuilder<BerachainChainSpec>,
            >::default())
    }
}

impl<N> DebugNode<N> for BerachainNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block;

    fn rpc_to_primitive_block(_rpc_block: Self::RpcBlock) -> BlockTy<Self> {
        todo!()
    }
}
