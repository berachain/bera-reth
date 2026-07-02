//! [`MinPriorityFeeValidator`]: enforce a minimum priority fee (tip) on every transaction
//! type.
//!
//! reth's built-in `--txpool.minimum-priority-fee` has two gaps a spammer can exploit:
//!   1. it only checks *dynamic-fee* transactions (`is_dynamic_fee()`), so legacy and EIP-2930
//!      transactions bypass it entirely, and
//!   2. it compares the declared `max_priority_fee_per_gas` *cap*, not the tip actually paid, so a
//!      transaction with `maxFeePerGas == baseFee` pays a zero effective tip while still passing
//!      the cap check.
//!
//! This wrapper closes both. It derives the *effective* tip for every transaction type
//! against the next-block (pending) base fee, the same base fee the pool's own fee policy
//! uses, and rejects externally-received transactions below the floor. While the floor is
//! active, non-local transactions that cannot cover the base fee are rejected rather than
//! parked, because the pool promotes parked transactions without re-validation once the
//! base fee dips, which would readmit zero-tip spam.
//!
//! The floor is enforced at admission only, like reth's built-in check. A transaction
//! admitted at the floor can later be demoted by a base fee rise and re-promoted without
//! re-validation once the base fee dips, paying below the floor at inclusion. It is a
//! mempool policy only (it never changes block validity), so it is safe to deploy without
//! a fork, with the usual caveat that it only governs what this node admits and builds.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use alloy_consensus::{BlockHeader, Transaction};
use reth_chainspec::EthChainSpec;
use reth_primitives_traits::{Block, SealedBlock};
use reth_transaction_pool::{
    LocalTransactionConfig, PoolTransaction, TransactionOrigin, TransactionValidationOutcome,
    TransactionValidator, error::InvalidPoolTransactionError,
};

/// Reassembles batch outcomes in input order, filling the empty slots from `validated`.
fn merge_outcomes<T>(slots: Vec<Option<T>>, validated: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut validated = validated.into_iter();
    slots
        .into_iter()
        .map(|slot| {
            slot.unwrap_or_else(|| {
                validated.next().expect("inner validator returns one outcome per transaction")
            })
        })
        .collect()
}

/// Wraps a [`TransactionValidator`] and rejects non-local transactions whose effective tip
/// is below `minimum_priority_fee`, across all transaction types.
#[derive(Debug)]
pub struct MinPriorityFeeValidator<V, C> {
    inner: V,
    /// Minimum effective tip in wei. `0` disables the check.
    minimum_priority_fee: u128,
    /// Local-transaction policy (honors `--txpool.locals` / `--txpool.no-locals`) that
    /// decides which transactions are exempt from the tip floor.
    local_transactions_config: LocalTransactionConfig,
    /// Chain spec used to derive the next-block base fee from each new head.
    chain_spec: Arc<C>,
    /// Next-block (pending) base fee in wei, matching the base fee the pool's own fee
    /// policy uses, so admission mirrors what the transaction would pay if included in the
    /// next block. Seeded at construction so the floor is live before the first canonical
    /// block arrives, then re-derived from every new head.
    base_fee: AtomicU64,
}

impl<V, C> MinPriorityFeeValidator<V, C> {
    /// Wrap `inner`, requiring at least `minimum_priority_fee` wei of effective tip from
    /// non-local transactions. `initial_base_fee` should be the next-block base fee of the
    /// current head so the floor is enforced from startup.
    pub const fn new(
        inner: V,
        minimum_priority_fee: u128,
        local_transactions_config: LocalTransactionConfig,
        chain_spec: Arc<C>,
        initial_base_fee: u64,
    ) -> Self {
        Self {
            inner,
            minimum_priority_fee,
            local_transactions_config,
            chain_spec,
            base_fee: AtomicU64::new(initial_base_fee),
        }
    }
}

impl<V, C> MinPriorityFeeValidator<V, C>
where
    V: TransactionValidator,
{
    /// Whether the floor applies to this transaction and its effective tip falls below it.
    ///
    /// A transaction that cannot cover the base fee (`effective_tip_per_gas` returns
    /// `None`) is treated as paying no tip, so the floor rejects it instead of letting the
    /// pool park it and promote it later without re-validation.
    fn is_below_minimum(&self, origin: TransactionOrigin, transaction: &V::Transaction) -> bool {
        self.minimum_priority_fee > 0 &&
            !self.local_transactions_config.is_local(origin, transaction.sender_ref()) &&
            transaction
                .effective_tip_per_gas(self.base_fee.load(Ordering::Relaxed))
                .unwrap_or_default() <
                self.minimum_priority_fee
    }

    fn reject(&self, transaction: V::Transaction) -> TransactionValidationOutcome<V::Transaction> {
        TransactionValidationOutcome::Invalid(
            transaction,
            InvalidPoolTransactionError::PriorityFeeBelowMinimum {
                minimum_priority_fee: self.minimum_priority_fee,
            },
        )
    }
}

impl<V, C> TransactionValidator for MinPriorityFeeValidator<V, C>
where
    V: TransactionValidator,
    C: EthChainSpec<Header = <V::Block as Block>::Header>,
{
    type Transaction = V::Transaction;
    type Block = V::Block;

    async fn validate_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Self::Transaction,
    ) -> TransactionValidationOutcome<Self::Transaction> {
        if self.is_below_minimum(origin, &transaction) {
            return self.reject(transaction);
        }
        self.inner.validate_transaction(origin, transaction).await
    }

    /// Overridden so batches keep the inner validator's batch path, which shares one state
    /// provider across the whole batch. The trait default would fall back to per-transaction
    /// validation and acquire a fresh provider for every transaction.
    async fn validate_transactions(
        &self,
        transactions: impl IntoIterator<Item = (TransactionOrigin, Self::Transaction), IntoIter: Send>
        + Send,
    ) -> Vec<TransactionValidationOutcome<Self::Transaction>> {
        let mut slots = Vec::new();
        let mut to_validate = Vec::new();
        for (origin, transaction) in transactions {
            if self.is_below_minimum(origin, &transaction) {
                slots.push(Some(self.reject(transaction)));
            } else {
                slots.push(None);
                to_validate.push((origin, transaction));
            }
        }
        merge_outcomes(slots, self.inner.validate_transactions(to_validate).await)
    }

    fn on_new_head_block(&self, new_tip_block: &SealedBlock<Self::Block>) {
        if let Some(base_fee) = self
            .chain_spec
            .next_block_base_fee(new_tip_block.header(), new_tip_block.header().timestamp())
        {
            self.base_fee.store(base_fee, Ordering::Relaxed);
        }
        self.inner.on_new_head_block(new_tip_block);
    }
}

#[cfg(test)]
mod tests {
    use super::merge_outcomes;

    #[test]
    fn merge_outcomes_preserves_input_order() {
        // Rejected slots (Some) interleave with inner-validated outcomes in input order.
        let merged = merge_outcomes(vec![None, Some(10), None, Some(11), None], vec![0, 1, 2]);
        assert_eq!(merged, vec![0, 10, 1, 11, 2]);

        let all_rejected = merge_outcomes(vec![Some(10), Some(11)], vec![]);
        assert_eq!(all_rejected, vec![10, 11]);
    }
}
