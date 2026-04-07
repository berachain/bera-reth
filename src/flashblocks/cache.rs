use crate::flashblocks::{
    FlashBlockCompleteSequence,
    payload::PendingFlashBlock,
    sequence::{FlashBlockPendingSequence, SequenceExecutionOutcome},
    traits::{FlashblockDiff, FlashblockPayload, FlashblockPayloadBase},
    worker::BuildArgs,
};
use alloy_eips::eip2718::WithEncoded;
use alloy_primitives::B256;
use reth_primitives_traits::{NodePrimitives, Recovered};
use reth_revm::cached::CachedReads;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use tokio::sync::broadcast;
use tracing::*;

type CachedSequenceEntry<P> = (
    FlashBlockCompleteSequence<P>,
    Vec<WithEncoded<Recovered<<P as FlashblockPayload>::SignedTx>>>,
);

type SequenceBuildArgs<P> = BuildArgs<
    Vec<WithEncoded<Recovered<<P as FlashblockPayload>::SignedTx>>>,
    <P as FlashblockPayload>::Base,
>;

const CACHE_SIZE: usize = 3;
pub(crate) const FLASHBLOCK_BLOCK_TIME: u64 = 200;

#[derive(Debug)]
pub(crate) struct SequenceManager<P: FlashblockPayload> {
    pending: FlashBlockPendingSequence<P>,
    pending_transactions: Vec<WithEncoded<Recovered<P::SignedTx>>>,
    completed_cache: AllocRingBuffer<CachedSequenceEntry<P>>,
    block_broadcaster: broadcast::Sender<FlashBlockCompleteSequence<P>>,
    compute_state_root: bool,
}

impl<P: FlashblockPayload> SequenceManager<P> {
    pub(crate) fn new(compute_state_root: bool) -> Self {
        let (block_broadcaster, _) = broadcast::channel(128);
        Self {
            pending: FlashBlockPendingSequence::new(),
            pending_transactions: Vec::new(),
            completed_cache: AllocRingBuffer::new(CACHE_SIZE),
            block_broadcaster,
            compute_state_root,
        }
    }

    pub(crate) const fn block_sequence_broadcaster(
        &self,
    ) -> &broadcast::Sender<FlashBlockCompleteSequence<P>> {
        &self.block_broadcaster
    }

    pub(crate) fn subscribe_block_sequence(
        &self,
    ) -> broadcast::Receiver<FlashBlockCompleteSequence<P>> {
        self.block_broadcaster.subscribe()
    }

    pub(crate) fn insert_flashblock(&mut self, flashblock: P) -> eyre::Result<()> {
        if flashblock.index() == 0 && self.pending.count() > 0 {
            let completed = self.pending.finalize()?;
            let block_number = completed.block_number();
            let parent_hash = completed.payload_base().parent_hash();

            trace!(
                target: "flashblocks",
                block_number,
                %parent_hash,
                cache_size = self.completed_cache.len(),
                "Caching completed flashblock sequence"
            );

            if self.block_broadcaster.receiver_count() > 0 {
                let _ = self.block_broadcaster.send(completed.clone());
            }

            let txs = std::mem::take(&mut self.pending_transactions);
            self.completed_cache.push((completed, txs));

            let _ = self.pending.take_cached_reads();
        }

        self.pending_transactions
            .extend(flashblock.recover_transactions().collect::<Result<Vec<_>, _>>()?);
        self.pending.insert(flashblock);
        Ok(())
    }

    pub(crate) const fn pending(&self) -> &FlashBlockPendingSequence<P> {
        &self.pending
    }

    pub(crate) fn next_buildable_args(
        &mut self,
        local_tip_hash: B256,
        local_tip_timestamp: u64,
    ) -> Option<SequenceBuildArgs<P>> {
        let (
            base,
            last_index,
            last_hash,
            last_state_root_zero,
            transactions,
            cached_state,
            source_name,
        ) = if let Some(base) =
            self.pending.payload_base().cloned().filter(|b| b.parent_hash() == local_tip_hash)
        {
            let cached_state = self.pending.take_cached_reads().map(|r| (base.parent_hash(), r));
            let last_fb = self.pending.last_flashblock()?;
            let transactions = self.pending_transactions.clone();
            (
                base,
                last_fb.index(),
                last_fb.diff().block_hash(),
                last_fb.diff().state_root().is_zero(),
                transactions,
                cached_state,
                "pending",
            )
        } else if let Some((cached, txs)) = self
            .completed_cache
            .iter()
            .find(|(c, _)| c.payload_base().parent_hash() == local_tip_hash)
        {
            let base = cached.payload_base().clone();
            let last_fb = cached.last();
            let transactions = txs.clone();
            (
                base,
                last_fb.index(),
                last_fb.diff().block_hash(),
                last_fb.diff().state_root().is_zero(),
                transactions,
                None,
                "cached",
            )
        } else {
            return None;
        };

        let block_time_ms = base.timestamp().saturating_sub(local_tip_timestamp) * 1000;
        let expected_final_flashblock = block_time_ms / FLASHBLOCK_BLOCK_TIME;
        let compute_state_root = self.compute_state_root &&
            last_state_root_zero &&
            last_index >= expected_final_flashblock.saturating_sub(1);

        trace!(
            target: "flashblocks",
            block_number = base.block_number(),
            source = source_name,
            flashblock_index = last_index,
            expected_final_flashblock,
            compute_state_root_enabled = self.compute_state_root,
            state_root_is_zero = last_state_root_zero,
            will_compute_state_root = compute_state_root,
            "Building from flashblock sequence"
        );

        Some(BuildArgs {
            base,
            transactions,
            cached_state,
            last_flashblock_index: last_index,
            last_flashblock_hash: last_hash,
            compute_state_root,
        })
    }

    pub(crate) fn on_build_complete<N: NodePrimitives>(
        &mut self,
        parent_hash: B256,
        result: Option<(PendingFlashBlock<N>, CachedReads)>,
    ) {
        let Some((computed_block, cached_reads)) = result else {
            return;
        };

        let execution_outcome = computed_block.computed_state_root().map(|state_root| {
            SequenceExecutionOutcome { block_hash: computed_block.block().hash(), state_root }
        });

        if self.pending.payload_base().is_some_and(|base| base.parent_hash() == parent_hash) {
            self.pending.set_execution_outcome(execution_outcome);
            self.pending.set_cached_reads(cached_reads);
            trace!(
                target: "flashblocks",
                block_number = self.pending.block_number(),
                has_computed_state_root = execution_outcome.is_some(),
                "Updated pending sequence with build results"
            );
        } else if let Some((cached, _)) = self
            .completed_cache
            .iter_mut()
            .find(|(c, _)| c.payload_base().parent_hash() == parent_hash)
        {
            let needs_rebroadcast =
                execution_outcome.is_some() && cached.execution_outcome().is_none();

            cached.set_execution_outcome(execution_outcome);

            if needs_rebroadcast && self.block_broadcaster.receiver_count() > 0 {
                trace!(
                    target: "flashblocks",
                    block_number = cached.block_number(),
                    "Re-broadcasting sequence with computed state_root"
                );
                let _ = self.block_broadcaster.send(cached.clone());
            }
        }
    }
}
