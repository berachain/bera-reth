//! Berachain node implementation using Reth's component-based architecture

pub mod evm;

use crate::{
    chainspec::BerachainChainSpec,
    consensus::BerachainConsensusBuilder,
    engine::{
        BerachainEngineTypes, builder::BerachainPayloadServiceBuilder,
        rpc::BerachainEngineApiBuilder, validator::BerachainEngineValidatorBuilder,
    },
    node::evm::BerachainExecutorBuilder,
    pool::BerachainPoolBuilder,
    primitives::{BerachainHeader, BerachainPrimitives},
    rpc::{BerachainAddOns, BerachainEthApiBuilder},
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::error::ValueError;
use alloy_rpc_types::TransactionRequest;
use reth::{
    api::{BlockTy, FullNodeTypes, NodeTypes},
    providers::EthStorage,
    rpc::compat::TryIntoSimTx,
};
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_node_api::FullNodeComponents;
use reth_node_builder::{
    DebugNode, Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder},
};
use reth_node_ethereum::{EthereumNode, node::EthereumNetworkBuilder};
use reth_payload_primitives::{PayloadAttributesBuilder, PayloadTypes};
use std::sync::Arc;

/// Type configuration for a regular Berachain node.

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainNode;

// Same as ETH Except we use BerachainChainSpec
impl NodeTypes for BerachainNode {
    type Primitives = BerachainPrimitives;
    type ChainSpec = BerachainChainSpec;
    type StateCommitment = <EthereumNode as NodeTypes>::StateCommitment;
    type Storage = EthStorage<BerachainTxEnvelope, BerachainHeader>;
    type Payload = BerachainEngineTypes;
}

impl TryIntoSimTx<BerachainTxEnvelope> for TransactionRequest {
    fn try_into_sim_tx(self) -> Result<BerachainTxEnvelope, ValueError<Self>> {
        // TODO: Add support for simulation API
        Err(ValueError::new(self, "Simulation API is not supported on bera-reth yet"))
    }
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
        BerachainPoolBuilder,
        BasicPayloadServiceBuilder<BerachainPayloadServiceBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        BerachainConsensusBuilder,
    >;

    /// Reth SDK AddOns providing RPC and Engine API interfaces.
    ///
    /// - **EthApiBuilder**: Standard Ethereum JSON-RPC API implementation
    /// - **BerachainEngineValidatorBuilder**: Validates Engine API requests with Berachain rules
    /// - **BasicEngineApiBuilder**: Handles consensus layer communication via Engine API
    ///   - Processes `forkchoice_updated` to trigger payload building
    ///   - Handles `new_payload` for block execution and validation
    ///   - Manages consensus-execution layer synchronization
    type AddOns = BerachainAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
        BerachainEthApiBuilder,
        BerachainEngineValidatorBuilder,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        ComponentsBuilder::default()
            .node_types()
            .pool(BerachainPoolBuilder)
            .executor(BerachainExecutorBuilder)
            .payload(BasicPayloadServiceBuilder::new(BerachainPayloadServiceBuilder::default()))
            .network(EthereumNetworkBuilder::default())
            .consensus(BerachainConsensusBuilder)
    }

    fn add_ons(&self) -> Self::AddOns {
        BerachainAddOns::default()
            .with_engine_validator(BerachainEngineValidatorBuilder::default())
            .with_engine_api(BerachainEngineApiBuilder::<BerachainEngineValidatorBuilder>::default())
    }
}

impl<N> DebugNode<N> for BerachainNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block<BerachainTxEnvelope, BerachainHeader>;

    fn rpc_to_primitive_block(rpc_block: Self::RpcBlock) -> BlockTy<Self> {
        rpc_block.into_consensus_block().convert_transactions()
    }

    fn local_payload_attributes_builder(
        chain_spec: &Self::ChainSpec,
    ) -> impl PayloadAttributesBuilder<<<Self as NodeTypes>::Payload as PayloadTypes>::PayloadAttributes>
    {
        LocalPayloadAttributesBuilder::new(Arc::new(chain_spec.clone()))
    }
}
