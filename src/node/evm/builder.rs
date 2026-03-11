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

fn fix_pol_senders(
    outcome: &mut BlockBuilderOutcome<BerachainPrimitives>,
    chain_spec: &BerachainChainSpec,
) -> Result<(), BlockExecutionError> {
    let num_txs = outcome.block.body().transactions.len();
    let num_senders = outcome.block.senders().len();
    let timestamp = outcome.block.header().timestamp();
    let is_prague1 = chain_spec.is_prague1_active_at_timestamp(timestamp);

    if num_senders > num_txs {
        return Err(BlockExecutionError::msg(format!(
            "more senders ({num_senders}) than transactions ({num_txs}) at timestamp \
             {timestamp}"
        )));
    }

    if !is_prague1 {
        if num_txs != num_senders {
            return Err(BlockExecutionError::msg(format!(
                "transaction/sender mismatch pre-Prague1: {num_txs} txs vs {num_senders} \
                 senders at timestamp {timestamp}"
            )));
        }
        return Ok(());
    }

    if num_txs == num_senders {
        return Err(BlockExecutionError::msg(format!(
            "no transaction/sender mismatch post-Prague1: {num_txs} txs vs {num_senders} \
             senders at timestamp {timestamp}. PoL injection should always occur"
        )));
    }

    let num_injected = num_txs - num_senders;
    if num_injected != 1 {
        return Err(BlockExecutionError::msg(format!(
            "expected exactly 1 injected PoL transaction, found {num_injected}"
        )));
    }

    let pol_tx = &outcome.block.body().transactions[0];
    let pol_sender = match pol_tx {
        BerachainTxEnvelope::Berachain(pol) => pol.from,
        _ => {
            return Err(BlockExecutionError::msg(format!(
                "first transaction is not PoL (type {:?})",
                pol_tx.tx_type()
            )));
        }
    };

    let mut fixed_senders = Vec::with_capacity(num_txs);
    fixed_senders.push(pol_sender);
    fixed_senders.extend(outcome.block.senders().iter().copied());

    outcome.block = RecoveredBlock::new_unhashed(outcome.block.clone_block(), fixed_senders);

    Ok(())
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
        fix_pol_senders(&mut outcome, &self.chain_spec)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        primitives::{BerachainBlock, BerachainBlockBody, BerachainHeader},
        test_utils::bepolia_chainspec,
        transaction::PoLTx,
    };
    use alloy_consensus::{BlockBody, Signed, TxLegacy};
    use alloy_evm::block::BlockExecutionResult;
    use alloy_primitives::{Address, Bytes, Sealed, Signature, address};
    use reth_trie_common::{HashedPostState, updates::TrieUpdates};

    const SYSTEM_ADDR: Address = address!("fffffffffffffffffffffffffffffffffffffffe");
    const USER_ADDR: Address = address!("1111111111111111111111111111111111111111");

    fn make_pol_tx(from: Address) -> BerachainTxEnvelope {
        BerachainTxEnvelope::Berachain(Sealed::new(PoLTx {
            chain_id: 1,
            from,
            to: Address::ZERO,
            nonce: 0,
            gas_limit: 30_000_000,
            gas_price: 1000,
            input: Bytes::new(),
        }))
    }

    fn make_eth_tx() -> BerachainTxEnvelope {
        use alloy_consensus::EthereumTxEnvelope;
        let signed = Signed::new_unhashed(TxLegacy::default(), Signature::test_signature());
        BerachainTxEnvelope::Ethereum(EthereumTxEnvelope::Legacy(signed))
    }

    fn make_outcome(
        txs: Vec<BerachainTxEnvelope>,
        senders: Vec<Address>,
        timestamp: u64,
    ) -> BlockBuilderOutcome<BerachainPrimitives> {
        let header = BerachainHeader { timestamp, ..Default::default() };
        let body = BerachainBlockBody { transactions: txs, ..BlockBody::default() };
        let block = BerachainBlock { header, body };
        let recovered = RecoveredBlock::new_unhashed(block, senders);

        BlockBuilderOutcome {
            execution_result: BlockExecutionResult::default(),
            hashed_state: HashedPostState::default(),
            trie_updates: TrieUpdates::default(),
            block: recovered,
        }
    }

    fn pre_prague1_timestamp() -> u64 {
        0
    }

    fn post_prague1_timestamp(chain_spec: &BerachainChainSpec) -> u64 {
        use crate::hardforks::BerachainHardfork;
        chain_spec
            .inner
            .hardforks
            .fork(BerachainHardfork::Prague1)
            .as_timestamp()
            .expect("Prague1 must have a timestamp on bepolia")
    }

    #[test]
    fn pre_prague1_equal_counts_ok() {
        let chain_spec = bepolia_chainspec();
        let ts = pre_prague1_timestamp();
        assert!(!chain_spec.is_prague1_active_at_timestamp(ts));

        let mut outcome = make_outcome(vec![], vec![], ts);
        assert!(fix_pol_senders(&mut outcome, &chain_spec).is_ok());

        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![eth_tx], vec![USER_ADDR], ts);
        assert!(fix_pol_senders(&mut outcome, &chain_spec).is_ok());
    }

    #[test]
    fn pre_prague1_mismatch_errors() {
        let chain_spec = bepolia_chainspec();
        let ts = pre_prague1_timestamp();

        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![eth_tx], vec![], ts);
        let err = fix_pol_senders(&mut outcome, &chain_spec).unwrap_err();
        assert!(err.to_string().contains("transaction/sender mismatch pre-Prague1"));
    }

    #[test]
    fn post_prague1_one_injection_fixes_senders() {
        let chain_spec = bepolia_chainspec();
        let ts = post_prague1_timestamp(&chain_spec);

        let pol_tx = make_pol_tx(SYSTEM_ADDR);
        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![pol_tx, eth_tx], vec![USER_ADDR], ts);

        assert!(fix_pol_senders(&mut outcome, &chain_spec).is_ok());
        assert_eq!(outcome.block.senders().len(), 2);
        assert_eq!(outcome.block.senders()[0], SYSTEM_ADDR);
        assert_eq!(outcome.block.senders()[1], USER_ADDR);
    }

    #[test]
    fn post_prague1_no_mismatch_errors() {
        let chain_spec = bepolia_chainspec();
        let ts = post_prague1_timestamp(&chain_spec);

        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![eth_tx], vec![USER_ADDR], ts);
        let err = fix_pol_senders(&mut outcome, &chain_spec).unwrap_err();
        assert!(err.to_string().contains("no transaction/sender mismatch post-Prague1"));
    }

    #[test]
    fn post_prague1_multiple_injections_errors() {
        let chain_spec = bepolia_chainspec();
        let ts = post_prague1_timestamp(&chain_spec);

        let pol1 = make_pol_tx(SYSTEM_ADDR);
        let pol2 = make_pol_tx(SYSTEM_ADDR);
        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![pol1, pol2, eth_tx], vec![USER_ADDR], ts);
        let err = fix_pol_senders(&mut outcome, &chain_spec).unwrap_err();
        assert!(err.to_string().contains("expected exactly 1 injected PoL transaction, found 2"));
    }

    #[test]
    fn post_prague1_non_pol_at_index_zero_errors() {
        let chain_spec = bepolia_chainspec();
        let ts = post_prague1_timestamp(&chain_spec);

        let eth_tx = make_eth_tx();
        let mut outcome = make_outcome(vec![eth_tx], vec![], ts);
        let err = fix_pol_senders(&mut outcome, &chain_spec).unwrap_err();
        assert!(err.to_string().contains("first transaction is not PoL"));
    }

    #[test]
    fn post_prague1_pol_only_block() {
        let chain_spec = bepolia_chainspec();
        let ts = post_prague1_timestamp(&chain_spec);

        let pol_tx = make_pol_tx(SYSTEM_ADDR);
        let mut outcome = make_outcome(vec![pol_tx], vec![], ts);
        assert!(fix_pol_senders(&mut outcome, &chain_spec).is_ok());
        assert_eq!(outcome.block.senders().len(), 1);
        assert_eq!(outcome.block.senders()[0], SYSTEM_ADDR);
    }
}
