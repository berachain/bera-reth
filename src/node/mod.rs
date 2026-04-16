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
use alloy_primitives::{Signature, TxHash};
use alloy_rpc_types::TransactionRequest;
use reth::{
    api::{BlockTy, FullNodeTypes, NodeTypes, PrimitivesTy, TxTy},
    providers::EthStorage,
    rpc::compat::TryIntoSimTx,
};
use reth_chainspec::Hardforks;
use reth_engine_local::LocalPayloadAttributesBuilder;
use reth_network::{
    NetworkHandle, primitives::BasicNetworkPrimitives, transactions::TransactionProvenanceSink,
};
use reth_network_peers::PeerId;
use reth_node_api::FullNodeComponents;
use reth_node_builder::{
    BuilderContext, DebugNode, Node, NodeAdapter, NodeComponentsBuilder,
    components::{BasicPayloadServiceBuilder, ComponentsBuilder, NetworkBuilder},
};
use reth_payload_primitives::{PayloadAttributesBuilder, PayloadTypes};
use reth_transaction_pool::{PoolPooledTx, PoolTransaction, TransactionPool};
use std::sync::Arc;

#[derive(Clone)]
struct PogTxProvenanceSink {
    store: Arc<crate::pog::PogAttributionStore>,
}

impl TransactionProvenanceSink for PogTxProvenanceSink {
    fn record_accepted_from_peer(&self, peer_id: PeerId, accepted_tx_hashes: &[TxHash]) {
        if let Ok(mut p) = self.store.provenance.lock() {
            for &hash in accepted_tx_hashes {
                p.insert(hash, peer_id);
            }
        }
    }
}

/// Network builder for Berachain that injects a PoG provenance callback when PoG is enabled.
#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainNetworkBuilder;

impl<Node, Pool> NetworkBuilder<Node, Pool> for BerachainNetworkBuilder
where
    Node: FullNodeTypes<Types: NodeTypes<ChainSpec: Hardforks>>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
        + Unpin
        + 'static,
{
    type Network =
        NetworkHandle<BasicNetworkPrimitives<PrimitivesTy<Node::Types>, PoolPooledTx<Pool>>>;

    async fn build_network(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
    ) -> eyre::Result<Self::Network> {
        let network = ctx.network_builder().await?;
        let handle = if crate::pog::pog_cli_enabled() {
            let cb = Arc::new(PogTxProvenanceSink { store: crate::pog::attribution_store() });
            ctx.start_network_with_provenance_callback(network, pool, cb)
        } else {
            ctx.start_network(network, pool)
        };
        Ok(handle)
    }
}

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
        BerachainNetworkBuilder,
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
            .network(BerachainNetworkBuilder)
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
    ) -> impl PayloadAttributesBuilder<
        <<Self as NodeTypes>::Payload as PayloadTypes>::PayloadAttributes,
        BerachainHeader,
    > {
        LocalPayloadAttributesBuilder::new(Arc::new(chain_spec.clone()))
    }
}
