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
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tracing::{debug, warn};

/// BERA-305 / VC-1: latched once when the provenance sink first observes an accept-batch
/// with `listening_addr = None` (i.e. a peer's devp2p `Hello.port == 0`). Surfaces a
/// single `warn!` so operators get an actionable signal that some peers will only ever
/// produce `first_enode = NULL` rows; subsequent occurrences are tracked via metrics.
static OBSERVED_NONE_LISTENING_ADDR: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct PogTxProvenanceSink {
    store: Arc<crate::pog::PogAttributionStore>,
}

impl TransactionProvenanceSink for PogTxProvenanceSink {
    fn record_accepted_from_peer(
        &self,
        peer_id: PeerId,
        listening_addr: Option<SocketAddr>,
        accepted_tx_hashes: &[TxHash],
    ) {
        // reth fires this callback once per tx hash that was just successfully added to
        // the pool by this peer (first-seen-wins already enforced upstream by
        // `TransactionsManager::on_new_pooled_transactions`' retain closure). We simply
        // forward each hash to the InflightTransactions RAM store; the safety belt /
        // metrics bookkeeping lives inside `record_first_hear`.
        //
        // BERA-305: `listening_addr` (peer's first-hear advertised socket per devp2p Hello)
        // is captured alongside `peer_id` so the seal-flush path can persist a re-dialable
        // `first_enode`. When it is `None` (`Hello.port == 0`), `InflightTransactions` skips
        // the first-hear insert so sealed-tx facts do not attribute txs to undialable peers.
        //
        // VC-1 observability: at trace/debug we log every accept-batch with whether the
        // listening_addr was supplied. The first None observation in this process also
        // emits a one-shot `warn!` so operators see an actionable signal without having
        // to enable -vvvv. Per-tx outcomes are tracked via metric labels on
        // `pog_inflight_tx_first_hears_total{listening_addr_present=...}` and at the
        // SQL insert site via `pog_sealed_tx_facts_flushed_first_enode_total{outcome=...}`.
        debug!(
            target: "bera_reth::pog",
            peer_id = %peer_id,
            listening_addr_present = listening_addr.is_some(),
            listening_addr = ?listening_addr,
            n_hashes = accepted_tx_hashes.len(),
            "record_accepted_from_peer",
        );
        if listening_addr.is_none()
            && !OBSERVED_NONE_LISTENING_ADDR.swap(true, Ordering::Relaxed)
        {
            warn!(
                target: "bera_reth::pog",
                peer_id = %peer_id,
                n_hashes = accepted_tx_hashes.len(),
                "first peer accept with listening_addr=None observed (devp2p Hello.port=0); \
                 first-hear inserts for such accepts are skipped (no p2p peer attribution). \
                 Track Hello.port hit-rate via \
                 pog_inflight_tx_first_hears_total{{listening_addr_present}} and skips via \
                 pog_inflight_tx_first_hears_skipped_no_listening_addr_total.",
            );
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        if let Ok(mut inflight) = self.store.inflight.lock() {
            for &hash in accepted_tx_hashes {
                inflight.record_first_hear(hash, peer_id, listening_addr, now_ms);
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
