use crate::flashblocks::traits::FlashblockPayload;
use alloy_primitives::{B256, Bytes};
use alloy_rpc_types_engine::PayloadId;
use core::mem;
use eyre::{OptionExt, bail};
use reth_revm::cached::CachedReads;
use std::{collections::BTreeMap, ops::Deref};
use tokio::sync::broadcast;
use tracing::*;

use crate::flashblocks::traits::FlashblockDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceExecutionOutcome {
    pub block_hash: B256,
    pub state_root: B256,
}

#[derive(Debug)]
pub struct FlashBlockPendingSequence<P: FlashblockPayload> {
    inner: BTreeMap<u64, P>,
    block_broadcaster: broadcast::Sender<FlashBlockCompleteSequence<P>>,
    execution_outcome: Option<SequenceExecutionOutcome>,
    cached_reads: Option<CachedReads>,
}

impl<P: FlashblockPayload> FlashBlockPendingSequence<P> {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            inner: BTreeMap::new(),
            block_broadcaster: tx,
            execution_outcome: None,
            cached_reads: None,
        }
    }

    pub const fn block_sequence_broadcaster(
        &self,
    ) -> &broadcast::Sender<FlashBlockCompleteSequence<P>> {
        &self.block_broadcaster
    }

    pub fn subscribe_block_sequence(&self) -> broadcast::Receiver<FlashBlockCompleteSequence<P>> {
        self.block_broadcaster.subscribe()
    }

    pub fn insert(&mut self, flashblock: P) {
        if flashblock.index() == 0 {
            trace!(target: "flashblocks", number=%flashblock.block_number(), "Tracking new flashblock sequence");
            self.inner.insert(flashblock.index(), flashblock);
            return;
        }

        let same_block = self.block_number() == Some(flashblock.block_number());
        let same_payload = self.payload_id() == Some(flashblock.payload_id());

        if same_block && same_payload {
            trace!(target: "flashblocks", number=%flashblock.block_number(), index = %flashblock.index(), block_count = self.inner.len(), "Received followup flashblock");
            self.inner.insert(flashblock.index(), flashblock);
        } else {
            trace!(target: "flashblocks", number=%flashblock.block_number(), index = %flashblock.index(), current=?self.block_number(), "Ignoring untracked flashblock following");
        }
    }

    pub const fn set_execution_outcome(
        &mut self,
        execution_outcome: Option<SequenceExecutionOutcome>,
    ) {
        self.execution_outcome = execution_outcome;
    }

    pub fn set_cached_reads(&mut self, cached_reads: CachedReads) {
        self.cached_reads = Some(cached_reads);
    }

    pub const fn take_cached_reads(&mut self) -> Option<CachedReads> {
        self.cached_reads.take()
    }

    pub fn block_number(&self) -> Option<u64> {
        Some(self.inner.values().next()?.block_number())
    }

    pub fn payload_base(&self) -> Option<&P::Base> {
        self.inner.values().next()?.base()
    }

    pub fn count(&self) -> usize {
        self.inner.len()
    }

    pub fn last_flashblock(&self) -> Option<&P> {
        self.inner.last_key_value().map(|(_, b)| b)
    }

    pub fn index(&self) -> Option<u64> {
        Some(self.inner.values().last()?.index())
    }

    pub fn payload_id(&self) -> Option<PayloadId> {
        Some(self.inner.values().next()?.payload_id())
    }

    pub fn finalize(&mut self) -> eyre::Result<FlashBlockCompleteSequence<P>> {
        if self.inner.is_empty() {
            bail!("Cannot finalize empty flashblock sequence");
        }

        let flashblocks = mem::take(&mut self.inner);
        let execution_outcome = mem::take(&mut self.execution_outcome);
        self.cached_reads = None;

        FlashBlockCompleteSequence::new(flashblocks.into_values().collect(), execution_outcome)
    }

    pub fn flashblocks(&self) -> impl Iterator<Item = &P> {
        self.inner.values()
    }
}

impl<P: FlashblockPayload> Default for FlashBlockPendingSequence<P> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct FlashBlockCompleteSequence<P: FlashblockPayload> {
    inner: Vec<P>,
    execution_outcome: Option<SequenceExecutionOutcome>,
}

impl<P: FlashblockPayload> FlashBlockCompleteSequence<P> {
    pub fn new(
        blocks: Vec<P>,
        execution_outcome: Option<SequenceExecutionOutcome>,
    ) -> eyre::Result<Self> {
        let first_block = blocks.first().ok_or_eyre("No flashblocks in sequence")?;
        first_block.base().ok_or_eyre("Flashblock at index 0 has no base")?;

        if !blocks.iter().enumerate().all(|(idx, block)| {
            idx == block.index() as usize &&
                block.payload_id() == first_block.payload_id() &&
                block.block_number() == first_block.block_number()
        }) {
            bail!("Flashblock inconsistencies detected in sequence");
        }

        Ok(Self { inner: blocks, execution_outcome })
    }

    pub fn block_number(&self) -> u64 {
        self.inner.first().unwrap().block_number()
    }

    pub fn payload_base(&self) -> &P::Base {
        self.inner.first().unwrap().base().unwrap()
    }

    pub const fn count(&self) -> usize {
        self.inner.len()
    }

    pub fn last(&self) -> &P {
        self.inner.last().unwrap()
    }

    pub const fn execution_outcome(&self) -> Option<SequenceExecutionOutcome> {
        self.execution_outcome
    }

    pub const fn set_execution_outcome(
        &mut self,
        execution_outcome: Option<SequenceExecutionOutcome>,
    ) {
        self.execution_outcome = execution_outcome;
    }

    pub fn all_transactions(&self) -> Vec<Bytes> {
        self.inner.iter().flat_map(|fb| fb.diff().transactions_raw().iter().cloned()).collect()
    }

    pub fn flashblocks(&self) -> impl Iterator<Item = &P> {
        self.inner.iter()
    }
}

impl<P: FlashblockPayload> Deref for FlashBlockCompleteSequence<P> {
    type Target = Vec<P>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P: FlashblockPayload> TryFrom<FlashBlockPendingSequence<P>> for FlashBlockCompleteSequence<P> {
    type Error = eyre::Error;
    fn try_from(sequence: FlashBlockPendingSequence<P>) -> Result<Self, Self::Error> {
        Self::new(sequence.inner.into_values().collect(), sequence.execution_outcome)
    }
}
