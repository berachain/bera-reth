use crate::{
    chainspec::BerachainChainSpec, hardforks::BerachainHardforks, primitives::BerachainPrimitives,
    transaction::BerachainTxEnvelope,
};
use alloy_consensus::BlockHeader;
use reth::revm::context::result::ExecutionResult;
use reth_evm::{
    Evm,
    block::{BlockExecutionError, BlockExecutor, CommitChanges},
    execute::{BlockBuilder, BlockBuilderOutcome, ExecutorTx},
};
use reth_primitives_traits::RecoveredBlock;
use reth_storage_api::StateProvider;
use std::sync::Arc;

type EResult<E> = ExecutionResult<<<E as BlockExecutor>::Evm as Evm>::HaltReason>;

/// Berachain block builder wrapper that fixes sender/transaction mismatch from PoL injection.
///
/// # Problem (see <https://github.com/berachain/bera-reth/issues/129>)
///
/// `BasicBlockBuilder::finish()` tracks executed transactions in `self.transactions` and
/// creates senders via `unzip()`. However, `BerachainBlockAssembler::assemble_block()`
/// injects PoL transactions at position 0 that were never in that list, causing:
///
/// - `transactions`: `[PoL, Tx1, Tx2, ...]` (from assembler)
/// - `senders`: `[Sender1, Sender2, ...]` (from unzip - missing PoL sender)
/// - `receipts`: `[PoL_receipt, Receipt1, ...]` (from execution)
///
/// This mismatch breaks receipt lookups for pending blocks.
///
/// # Solution
///
/// This wrapper overrides `finish()` to detect the mismatch and reconstruct the
/// senders list by extracting the `from` field from injected PoL transactions.
///
/// # Invariants (strictly enforced)
///
/// - Pre-Prague1: `num_transactions == num_senders` (no PoL injection)
/// - Post-Prague1: `num_transactions > num_senders` (PoL always injected at position 0)
///
/// Violations in either direction indicate a bug in block assembly.
pub struct BerachainBlockBuilder<B>
where
    B: BlockBuilder<Primitives = BerachainPrimitives>,
{
    inner: B,
    chain_spec: Arc<BerachainChainSpec>,
}

impl<B> BerachainBlockBuilder<B>
where
    B: BlockBuilder<Primitives = BerachainPrimitives>,
{
    pub fn new(inner: B, chain_spec: Arc<BerachainChainSpec>) -> Self {
        Self { inner, chain_spec }
    }
}

impl<B> BlockBuilder for BerachainBlockBuilder<B>
where
    B: BlockBuilder<Primitives = BerachainPrimitives>,
{
    type Primitives = BerachainPrimitives;
    type Executor = B::Executor;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        self.inner.apply_pre_execution_changes()
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        tx: impl ExecutorTx<Self::Executor>,
        f: impl FnOnce(&EResult<Self::Executor>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        self.inner.execute_transaction_with_commit_condition(tx, f)
    }

    /// Finishes block building and fixes the sender/transaction mismatch from PoL injection.
    ///
    /// See struct-level docs for the full problem description. This override:
    /// 1. Calls `inner.finish()` which produces misaligned senders when PoL was injected
    /// 2. Enforces invariants based on Prague1 activation
    /// 3. Extracts senders from the injected PoL transactions at block start
    /// 4. Reconstructs the block with the corrected sender list
    fn finish(
        self,
        state_provider: impl StateProvider,
    ) -> Result<BlockBuilderOutcome<Self::Primitives>, BlockExecutionError> {
        let mut outcome = self.inner.finish(state_provider)?;

        let num_txs = outcome.block.body().transactions.len();
        let num_senders = outcome.block.senders().len();
        let timestamp = outcome.block.header().timestamp();
        let is_prague1 = self.chain_spec.is_prague1_active_at_timestamp(timestamp);

        match (num_txs > num_senders, is_prague1) {
            // Pre-Prague1 with mismatch = bug
            (true, false) => {
                return Err(BlockExecutionError::msg(format!(
                    "transaction/sender mismatch pre-Prague1: {} txs vs {} senders at timestamp {}. \
                     This should never happen before PoL injection is active",
                    num_txs, num_senders, timestamp
                )));
            }
            // Post-Prague1 with mismatch = expected, fix it
            (true, true) => {
                // There should only be 1 injected tx -> assert this?
                let num_injected = num_txs - num_senders;
                let mut fixed_senders = Vec::with_capacity(num_txs);

                for tx in outcome.block.body().transactions.iter().take(num_injected) {
                    match tx {
                        BerachainTxEnvelope::Berachain(pol) => {
                            fixed_senders.push(pol.from);
                        }
                        _ => {
                            return Err(BlockExecutionError::msg(format!(
                                "transaction/sender mismatch post-Prague1: found {} extra \
                                 transactions but transaction is not PoL (type {:?}). \
                                 Only PoL transactions should be injected",
                                num_injected,
                                tx.tx_type()
                            )));
                        }
                    }
                }

                fixed_senders.extend(outcome.block.senders().iter().copied());

                let fixed_block =
                    RecoveredBlock::new_unhashed(outcome.block.clone_block(), fixed_senders);
                outcome.block = fixed_block;
            }
            // Post-Prague1 without mismatch = bug (PoL injection should always occur)
            (false, true) => {
                return Err(BlockExecutionError::msg(format!(
                    "no transaction/sender mismatch post-Prague1: {} txs vs {} senders at timestamp {}. \
                     PoL injection should always occur after Prague1",
                    num_txs, num_senders, timestamp
                )));
            }
            // Pre-Prague1 without mismatch = expected
            (false, false) => {}
        }

        Ok(outcome)
    }

    fn executor_mut(&mut self) -> &mut Self::Executor {
        self.inner.executor_mut()
    }

    fn executor(&self) -> &Self::Executor {
        self.inner.executor()
    }

    fn into_executor(self) -> Self::Executor {
        self.inner.into_executor()
    }
}
