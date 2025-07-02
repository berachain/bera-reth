//! Berachain node implementation using Reth's component-based architecture

pub mod cli;
pub mod evm;

use crate::{
    chainspec::BerachainChainSpec,
    engine::{
        BerachainEngineTypes, builder::BerachainPayloadServiceBuilder,
        validator::BerachainEngineValidatorBuilder,
    },
    node::evm::BerachainExecutorBuilder,
    primitives::BerachainPrimitives,
};
use reth::api::{BlockTy, FullNodeTypes, NodeTypes};
use reth_node_api::FullNodeComponents;
use reth_node_builder::{
    DebugNode, Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
    rpc::BasicEngineApiBuilder,
};
use reth_node_ethereum::{
    EthereumAddOns, EthereumEthApiBuilder, EthereumNode,
    node::{EthereumConsensusBuilder, EthereumNetworkBuilder, EthereumPoolBuilder},
};

/// Type configuration for a regular Berachain node.

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainNode;

// Same as ETH Except we use BerachainChainSpec
impl NodeTypes for BerachainNode {
    type Primitives = BerachainPrimitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = <EthereumNode as NodeTypes>::Storage;
    type Payload = BerachainEngineTypes;
}

impl<N> Node<N> for BerachainNode
where
    N: FullNodeTypes<Types = Self>,
{
    /// Reth SDK ComponentsBuilder defining the core node architecture.
    ///
    /// Each component handles a specific domain of blockchain node operations:
    ///
    /// - **EthereumPoolBuilder**: Transaction pool management and validation
    ///   - Maintains mempool of pending transactions
    ///   - Validates transactions according to chain rules
    ///   - Provides transactions for block building
    ///
    /// - **`BasicPayloadServiceBuilder<BerachainPayloadServiceBuilder>`**: Block building and
    ///   payload creation
    ///   - Triggered by Engine API `forkchoice_updated` calls from consensus layer
    ///   - Assembles transactions from pool into block payloads
    ///   - Handles payload building jobs and manages build timeouts
    ///   - Uses BerachainPayloadBuilder for Berachain-specific block construction
    ///
    /// - **EthereumNetworkBuilder**: P2P networking and peer management
    ///   - Handles block/transaction propagation via devp2p
    ///   - Manages peer connections and discovery
    ///   - Synchronizes blockchain state with network peers
    ///
    /// - **BerachainExecutorBuilder**: EVM execution environment
    ///   - Creates standard Ethereum EVM with Berachain chain specification
    ///   - Executes transactions and manages state transitions
    ///   - Handles hardfork logic including Prague1 minimum base fee
    ///
    /// - **EthereumConsensusBuilder**: Block validation and consensus rules
    ///   - Validates block headers, transactions, and state transitions
    ///   - Enforces Ethereum consensus rules with Berachain extensions
    ///   - Manages fork choice and canonical chain determination
    type ComponentsBuilder = ComponentsBuilder<
        N,
        EthereumPoolBuilder,
        BasicPayloadServiceBuilder<BerachainPayloadServiceBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        EthereumConsensusBuilder,
    >;

    /// Reth SDK AddOns providing RPC and Engine API interfaces.
    ///
    /// - **EthApiBuilder**: Standard Ethereum JSON-RPC API implementation
    /// - **BerachainEngineValidatorBuilder**: Validates Engine API requests with Berachain rules
    /// - **BasicEngineApiBuilder**: Handles consensus layer communication via Engine API
    ///   - Processes `forkchoice_updated` to trigger payload building
    ///   - Handles `new_payload` for block execution and validation
    ///   - Manages consensus-execution layer synchronization
    type AddOns = EthereumAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
        EthereumEthApiBuilder,
        BerachainEngineValidatorBuilder,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        ComponentsBuilder::default()
            .node_types()
            .pool(EthereumPoolBuilder::default())
            .executor(BerachainExecutorBuilder)
            .payload(BasicPayloadServiceBuilder::new(BerachainPayloadServiceBuilder::default()))
            .network(EthereumNetworkBuilder::default())
            .consensus(EthereumConsensusBuilder::default())
    }

    fn add_ons(&self) -> Self::AddOns {
        EthereumAddOns::default()
            .with_engine_validator(BerachainEngineValidatorBuilder::default())
            .with_engine_api(BasicEngineApiBuilder::<BerachainEngineValidatorBuilder>::default())
    }
}

impl<N> DebugNode<N> for BerachainNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block;

    fn rpc_to_primitive_block(rpc_block: Self::RpcBlock) -> BlockTy<Self> {
        // rpc_block.into_consensus().convert_transactions()
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_types() {
        let node = BerachainNode::default();

        // Test that BerachainNode can be instantiated and has Debug
        let debug_str = format!("{node:?}");
        assert!(debug_str.contains("BerachainNode"));
    }
}
