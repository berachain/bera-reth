use crate::{
    hardforks::BerachainHardforks,
    primitives::{BerachainBlock, BerachainHeader, BerachainPrimitives},
};
use alloy_consensus::BlockHeader;
use reth::{
    api::NodeTypes,
    beacon_consensus::EthBeaconConsensus,
    chainspec::EthereumHardforks,
    consensus::{Consensus, ConsensusError, FullConsensus, HeaderValidator},
    providers::BlockExecutionResult,
};
use reth_chainspec::{ChainSpec, EthChainSpec};
use reth_node_api::FullNodeTypes;
use reth_node_builder::{BuilderContext, components::ConsensusBuilder};
use reth_primitives_traits::{
    GotExpected, NodePrimitives, RecoveredBlock, SealedBlock, SealedHeader,
};
use std::{fmt::Debug, sync::Arc};

/// Berachain consensus builder that delegates to Ethereum beacon consensus.
///
/// This wrapper is required to provide type compatibility with BerachainPrimitives
/// while using standard Ethereum consensus validation.
#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainConsensusBuilder;

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
        Ok(Arc::new(BerachainBeaconConsensus::new(ctx.chain_spec())))
    }
}

/// Berachain beacon consensus that delegates to Ethereum beacon consensus.
///
/// This wrapper provides type compatibility with BerachainPrimitives while
/// using the standard Ethereum consensus validation logic.
#[derive(Debug, Clone)]
pub struct BerachainBeaconConsensus<ChainSpec> {
    /// Inner Ethereum beacon consensus implementation
    inner: EthBeaconConsensus<ChainSpec>,
    chain_spec: Arc<ChainSpec>,
}

impl<ChainSpec: EthChainSpec + EthereumHardforks> BerachainBeaconConsensus<ChainSpec> {
    /// Create a new instance of [`BerachainBeaconConsensus`]
    pub fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self { inner: EthBeaconConsensus::new(chain_spec.clone()), chain_spec }
    }
}

impl<ChainSpec> FullConsensus<BerachainPrimitives> for BerachainBeaconConsensus<ChainSpec>
where
    ChainSpec: Send
        + Sync
        + EthChainSpec<Header = BerachainHeader>
        + EthereumHardforks
        + Debug
        + BerachainHardforks,
{
    fn validate_block_post_execution(
        &self,
        block: &RecoveredBlock<BerachainBlock>,
        result: &BlockExecutionResult<<BerachainPrimitives as NodePrimitives>::Receipt>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<ChainSpec> as FullConsensus<BerachainPrimitives>>::validate_block_post_execution(&self.inner, block, result)
    }
}

impl<ChainSpec> Consensus<BerachainBlock> for BerachainBeaconConsensus<ChainSpec>
where
    ChainSpec: EthChainSpec<Header = BerachainHeader> + EthereumHardforks + Debug + Send + Sync,
{
    type Error = ConsensusError;

    fn validate_body_against_header(
        &self,
        body: &<BerachainBlock as reth_primitives_traits::Block>::Body,
        header: &SealedHeader<BerachainHeader>,
    ) -> Result<(), Self::Error> {
        <EthBeaconConsensus<ChainSpec> as Consensus<BerachainBlock>>::validate_body_against_header(
            &self.inner,
            body,
            header,
        )
    }

    fn validate_block_pre_execution(
        &self,
        block: &SealedBlock<BerachainBlock>,
    ) -> Result<(), Self::Error> {
        <EthBeaconConsensus<ChainSpec> as Consensus<BerachainBlock>>::validate_block_pre_execution(
            &self.inner,
            block,
        )
    }
}

impl<ChainSpec> HeaderValidator<BerachainHeader> for BerachainBeaconConsensus<ChainSpec>
where
    ChainSpec: EthChainSpec<Header = BerachainHeader> + EthereumHardforks + Debug + Send + Sync,
{
    fn validate_header(
        &self,
        header: &SealedHeader<BerachainHeader>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<ChainSpec> as HeaderValidator<BerachainHeader>>::validate_header(
            &self.inner,
            header,
        )
    }

    fn validate_header_against_parent(
        &self,
        header: &SealedHeader<BerachainHeader>,
        parent: &SealedHeader<BerachainHeader>,
    ) -> Result<(), ConsensusError> {
        <EthBeaconConsensus<ChainSpec> as HeaderValidator<BerachainHeader>>::validate_header_against_parent(&self.inner, header, parent)
    }
}
