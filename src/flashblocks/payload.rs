use alloy_consensus::BlockHeader;
use alloy_primitives::B256;
use derive_more::Deref;
use reth_primitives_traits::NodePrimitives;
use reth_rpc_eth_types::PendingBlock;

#[derive(Debug, Clone, Deref)]
pub struct PendingFlashBlock<N: NodePrimitives> {
    #[deref]
    pub pending: PendingBlock<N>,
    pub last_flashblock_index: u64,
    pub last_flashblock_hash: B256,
    pub has_computed_state_root: bool,
}

impl<N: NodePrimitives> PendingFlashBlock<N> {
    pub const fn new(
        pending: PendingBlock<N>,
        last_flashblock_index: u64,
        last_flashblock_hash: B256,
        has_computed_state_root: bool,
    ) -> Self {
        Self { pending, last_flashblock_index, last_flashblock_hash, has_computed_state_root }
    }

    pub fn computed_state_root(&self) -> Option<B256> {
        self.has_computed_state_root.then_some(self.pending.block().state_root())
    }
}
