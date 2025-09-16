//! Berachain node implementation using Reth's component-based architecture

pub mod evm;

use crate::{
    chainspec::BerachainChainSpec,
    consensus::BerachainConsensusBuilder,
    engine::{
        BerachainEngineTypes, builder::BerachainPayloadServiceBuilder,
        validator::BerachainEngineValidatorBuilder,
    },
    node::evm::BerachainExecutorBuilder,
    pool::BerachainPoolBuilder,
    primitives::{BerachainHeader, BerachainPrimitives},
    rpc::{BerachainAddOns, BerachainEthApiBuilder},
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::{SignableTransaction, error::ValueError};
use alloy_primitives::Signature;
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
use reth_node_ethereum::node::EthereumNetworkBuilder;
use reth_payload_primitives::{PayloadAttributesBuilder, PayloadTypes};
use std::sync::Arc;

/// Type configuration for a regular Berachain node.

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct BerachainNode;

impl NodeTypes for BerachainNode {
    type Primitives = BerachainPrimitives;
    type ChainSpec = BerachainChainSpec;
    type Storage = EthStorage<BerachainTxEnvelope, BerachainHeader>;
    type Payload = BerachainEngineTypes;
}

impl TryIntoSimTx<BerachainTxEnvelope> for TransactionRequest {
    fn try_into_sim_tx(self) -> Result<BerachainTxEnvelope, ValueError<Self>> {
        let tx = self
            .build_typed_tx()
            .map_err(|req| ValueError::new(req, "Transaction is not buildable"))?;
        let signature = Signature::new(Default::default(), Default::default(), false);
        Ok(tx.into_signed(signature).into())
    }
}

impl<N> Node<N> for BerachainNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        BerachainPoolBuilder,
        BasicPayloadServiceBuilder<BerachainPayloadServiceBuilder>,
        EthereumNetworkBuilder,
        BerachainExecutorBuilder,
        BerachainConsensusBuilder,
    >;

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
