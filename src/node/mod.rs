pub mod cli;
mod evm;
mod rpc;

use crate::chainspec::BerachainChainSpec;
use crate::node::evm::BerachainExecutorBuilder;
use crate::node::rpc::engine_api::{BerachainEngineApiBuilder, BerachainEngineValidatorBuilder};
use crate::node::rpc::BerachainApiBuilder;
use reth::api::{BlockTy, FullNodeComponents, FullNodeTypes, NodeTypes};
use reth_node_builder::components::{BasicPayloadServiceBuilder, ComponentsBuilder};
use reth_node_builder::rpc::RpcAddOns;
use reth_node_builder::{DebugNode, Node, NodeAdapter, NodeComponentsBuilder};
use reth_node_ethereum::node::{EthereumConsensusBuilder, EthereumNetworkBuilder, EthereumPayloadBuilder, EthereumPoolBuilder};
use reth_node_ethereum::EthereumNode;

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

pub type BerachainAddOns<N> = RpcAddOns<N, BerachainApiBuilder, BerachainEngineValidatorBuilder, BerachainEngineApiBuilder>;

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


