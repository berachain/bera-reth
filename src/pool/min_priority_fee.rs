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
//! This wrapper closes both. It derives the *effective* tip against the current base fee
//! for every transaction type and rejects externally-received transactions below the
//! floor. While the floor is active, non-local transactions that cannot cover the base fee
//! are rejected rather than parked, because the pool promotes parked transactions without
//! re-validation once the base fee dips, which would readmit zero-tip spam. It is a
//! mempool policy only (it never changes block validity), so it is safe to deploy without
//! a fork, with the usual caveat that it only governs what this node admits and builds.

use std::sync::atomic::{AtomicU64, Ordering};

use alloy_consensus::{BlockHeader, Transaction};
use reth_primitives_traits::SealedBlock;
use reth_transaction_pool::{
    LocalTransactionConfig, PoolTransaction, TransactionOrigin, TransactionValidationOutcome,
    TransactionValidator, error::InvalidPoolTransactionError,
};

/// Effective tip a transaction pays at the given base fee, saturating to zero when the
/// transaction cannot cover the base fee.
///
/// Mirrors `alloy`'s `effective_tip_per_gas` but works from the pool-transaction fee
/// accessors so it applies uniformly to every transaction type:
///   * legacy / EIP-2930 have no priority-fee field (`max_priority_fee` is `None`), so the tip is
///     `gas_price - base_fee`;
///   * EIP-1559 / EIP-4844 / EIP-7702 tip `min(max_priority_fee, max_fee - base_fee)`.
///
/// Unlike `alloy`, a below-base-fee transaction yields `0` instead of `None`. Skipping it
/// would let the pool park it and later promote it without re-validation, bypassing the
/// floor, so it is treated as paying no tip.
fn effective_tip(max_fee: u128, max_priority_fee: Option<u128>, base_fee: u128) -> u128 {
    let fee_over_base = max_fee.saturating_sub(base_fee);
    match max_priority_fee {
        Some(priority) => fee_over_base.min(priority),
        None => fee_over_base,
    }
}

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
pub struct MinPriorityFeeValidator<V> {
    inner: V,
    /// Minimum effective tip in wei. `0` disables the check.
    minimum_priority_fee: u128,
    /// Local-transaction policy (honors `--txpool.locals` / `--txpool.no-locals`) that
    /// decides which transactions are exempt from the tip floor.
    local_transactions_config: LocalTransactionConfig,
    /// Base fee (wei) of the current head, seeded at construction so the floor is live
    /// before the first canonical block arrives, then updated on every new head.
    base_fee: AtomicU64,
}

impl<V> MinPriorityFeeValidator<V> {
    /// Wrap `inner`, requiring at least `minimum_priority_fee` wei of effective tip from
    /// non-local transactions. `initial_base_fee` should be the current head's base fee so
    /// the floor is enforced from startup.
    pub const fn new(
        inner: V,
        minimum_priority_fee: u128,
        local_transactions_config: LocalTransactionConfig,
        initial_base_fee: u64,
    ) -> Self {
        Self {
            inner,
            minimum_priority_fee,
            local_transactions_config,
            base_fee: AtomicU64::new(initial_base_fee),
        }
    }
}

impl<V> MinPriorityFeeValidator<V>
where
    V: TransactionValidator,
{
    /// Whether the floor applies to this transaction and its effective tip falls below it.
    fn is_below_minimum(&self, origin: TransactionOrigin, transaction: &V::Transaction) -> bool {
        self.minimum_priority_fee > 0 &&
            !self.local_transactions_config.is_local(origin, transaction.sender_ref()) &&
            effective_tip(
                transaction.max_fee_per_gas(),
                transaction.max_priority_fee_per_gas(),
                self.base_fee.load(Ordering::Relaxed) as u128,
            ) < self.minimum_priority_fee
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

impl<V> TransactionValidator for MinPriorityFeeValidator<V>
where
    V: TransactionValidator,
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
        if let Some(base_fee) = new_tip_block.header().base_fee_per_gas() {
            self.base_fee.store(base_fee, Ordering::Relaxed);
        }
        self.inner.on_new_head_block(new_tip_block);
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_tip, merge_outcomes};

    const GWEI: u128 = 1_000_000_000;

    #[test]
    fn legacy_zero_tip_is_caught() {
        // Legacy tx (no priority field) with gas_price == base_fee -> zero effective tip.
        // Previously exempt from reth's filter; now caught.
        assert_eq!(effective_tip(5 * GWEI, None, 5 * GWEI), 0);
    }

    #[test]
    fn legacy_tip_above_base_fee() {
        // Legacy tip is gas_price - base_fee.
        assert_eq!(effective_tip(7 * GWEI, None, 5 * GWEI), 2 * GWEI);
    }

    #[test]
    fn dynamic_fee_high_cap_low_max_fee_is_caught() {
        // The cap-loophole: declared priority cap of 10 gwei, but maxFee == baseFee, so the
        // effective tip is zero. reth's cap check would pass this; the effective check does not.
        assert_eq!(effective_tip(5 * GWEI, Some(10 * GWEI), 5 * GWEI), 0);
    }

    #[test]
    fn dynamic_fee_effective_tip_is_capped_by_priority() {
        // maxFee - baseFee = 6 gwei, but priority cap is 1 gwei -> tip is 1 gwei.
        assert_eq!(effective_tip(11 * GWEI, Some(GWEI), 5 * GWEI), GWEI);
    }

    #[test]
    fn dynamic_fee_effective_tip_is_capped_by_max_fee() {
        // priority cap 6 gwei, but maxFee - baseFee = 2 gwei -> tip is 2 gwei.
        assert_eq!(effective_tip(7 * GWEI, Some(6 * GWEI), 5 * GWEI), 2 * GWEI);
    }

    #[test]
    fn below_base_fee_saturates_to_zero_tip() {
        // Cannot cover the base fee -> zero tip, so the floor rejects it instead of letting
        // the pool park it and promote it later without re-validation.
        assert_eq!(effective_tip(4 * GWEI, None, 5 * GWEI), 0);
        assert_eq!(effective_tip(4 * GWEI, Some(10 * GWEI), 5 * GWEI), 0);
    }

    #[test]
    fn zero_base_fee_treats_full_price_as_tip() {
        assert_eq!(effective_tip(3 * GWEI, None, 0), 3 * GWEI);
    }

    #[test]
    fn merge_outcomes_preserves_input_order() {
        // Rejected slots (Some) interleave with inner-validated outcomes in input order.
        let merged = merge_outcomes(vec![None, Some(10), None, Some(11), None], vec![0, 1, 2]);
        assert_eq!(merged, vec![0, 10, 1, 11, 2]);

        let all_rejected = merge_outcomes(vec![Some(10), Some(11)], vec![]);
        assert_eq!(all_rejected, vec![10, 11]);
    }
}
