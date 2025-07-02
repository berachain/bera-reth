use crate::primitives::BerachainPrimitives;
use reth::{
    api::NodeTypes,
    beacon_consensus::EthBeaconConsensus,
    chainspec::EthereumHardforks,
    consensus::{ConsensusError, FullConsensus},
};
use reth_chainspec::EthChainSpec;
use reth_node_api::FullNodeTypes;
use reth_node_builder::{BuilderContext, components::ConsensusBuilder};
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainConsensusBuilder {
    // TODO add closure to modify consensus
}

impl<Node> ConsensusBuilder<Node> for BerachainConsensusBuilder
where
    Node: FullNodeTypes<
        Types: NodeTypes<
            ChainSpec: EthChainSpec + EthereumHardforks,
            Primitives = BerachainPrimitives,
        >,
    >,
{
    type Consensus = Arc<dyn FullConsensus<BerachainPrimitives, Error = ConsensusError>>;

    async fn build_consensus(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::Consensus> {
        Ok(Arc::new(EthBeaconConsensus::new(ctx.chain_spec())))
    }
}
