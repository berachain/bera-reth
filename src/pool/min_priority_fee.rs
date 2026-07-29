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
    use super::MinPriorityFeeValidator;
    use crate::{
        chainspec::BerachainChainSpec,
        primitives::{BerachainBlock, BerachainHeader},
    };
    use alloy_genesis::Genesis;
    use alloy_primitives::U256;
    use reth::rpc::types::serde_helpers::OtherFields;
    use reth_chainspec::EthChainSpec;
    use reth_primitives_traits::SealedBlock;
    use reth_transaction_pool::{
        LocalTransactionConfig, PoolTransaction,
        TransactionOrigin::{self, External, Local},
        TransactionValidationOutcome::{self, Invalid},
        TransactionValidator,
        error::InvalidPoolTransactionError::{PriorityFeeBelowMinimum, Underpriced},
        test_utils::MockTransaction,
    };
    use serde_json::json;
    use std::sync::Arc;

    const FLOOR: u128 = 1_000_000_000;
    const BASE_FEE: u128 = 50_000_000_000;

    type Wrapper = MinPriorityFeeValidator<SentinelInner, BerachainChainSpec>;

    /// Inner stub marking every transaction it receives with a sentinel error, so tests can
    /// tell whether the wrapper forwarded a transaction or floor-rejected it first.
    #[derive(Debug)]
    struct SentinelInner;

    impl TransactionValidator for SentinelInner {
        type Transaction = MockTransaction;
        type Block = BerachainBlock;

        async fn validate_transaction(
            &self,
            _origin: TransactionOrigin,
            transaction: Self::Transaction,
        ) -> TransactionValidationOutcome<Self::Transaction> {
            Invalid(transaction, Underpriced)
        }
    }

    fn chain_spec() -> Arc<BerachainChainSpec> {
        let mut genesis = Genesis::default();
        genesis.config.cancun_time = Some(0);
        genesis.config.prague_time = Some(0);
        genesis.config.terminal_total_difficulty = Some(U256::ZERO);
        genesis.config.extra_fields = OtherFields::try_from(json!({
            "berachain": {
                "prague1": { "time": 0, "baseFeeChangeDenominator": 48, "minimumBaseFeeWei": 10,
                             "polDistributorAddress": "0x4200000000000000000000000000000000000042" },
                "prague2": { "time": 0, "minimumBaseFeeWei": 0 }
            }
        }))
        .unwrap();
        Arc::new(BerachainChainSpec::from(genesis))
    }

    fn validator(floor: u128) -> Wrapper {
        let locals = LocalTransactionConfig::default();
        MinPriorityFeeValidator::new(SentinelInner, floor, locals, chain_spec(), BASE_FEE as u64)
    }

    async fn rejects(v: &Wrapper, origin: TransactionOrigin, tx: MockTransaction) -> bool {
        let outcome = v.validate_transaction(origin, tx).await;
        matches!(outcome, Invalid(_, PriorityFeeBelowMinimum { .. }))
    }

    fn legacy(gas_price: u128) -> MockTransaction {
        MockTransaction::legacy().with_gas_price(gas_price)
    }

    fn eip1559(max_fee: u128, tip_cap: u128) -> MockTransaction {
        MockTransaction::eip1559().with_max_fee(max_fee).with_priority_fee(tip_cap)
    }

    #[tokio::test]
    async fn enforces_effective_tip_floor_on_external_transactions() {
        let v = validator(FLOOR);
        // The two gaps in reth's built-in check: legacy is exempt entirely, and a declared
        // cap clearing the floor passes even when maxFee == baseFee makes the paid tip zero.
        assert!(rejects(&v, External, legacy(BASE_FEE)).await);
        assert!(rejects(&v, External, eip1559(BASE_FEE, 10 * FLOOR)).await);
        assert!(rejects(&v, External, eip1559(BASE_FEE + FLOOR, FLOOR - 1)).await);
        assert!(!rejects(&v, External, eip1559(BASE_FEE + FLOOR, FLOOR)).await);
        // Local transactions are exempt, and floor 0 disables the check entirely.
        assert!(!rejects(&v, Local, legacy(BASE_FEE)).await);
        assert!(!rejects(&validator(0), External, legacy(BASE_FEE)).await);
    }

    #[tokio::test]
    async fn batch_preserves_input_order_with_interleaved_rejections() {
        let v = validator(FLOOR);
        let txs = vec![
            eip1559(BASE_FEE + FLOOR, FLOOR),
            legacy(BASE_FEE),
            eip1559(BASE_FEE + 2 * FLOOR, 2 * FLOOR),
            eip1559(BASE_FEE, 10 * FLOOR),
        ];
        let hashes: Vec<_> = txs.iter().map(|tx| *tx.hash()).collect();
        let outcomes = v.validate_transactions(txs.into_iter().map(|tx| (External, tx))).await;

        for (i, expect_rejected) in [false, true, false, true].into_iter().enumerate() {
            let Invalid(tx, err) = &outcomes[i] else { panic!("bad outcome: {:?}", outcomes[i]) };
            assert_eq!(*tx.hash(), hashes[i], "outcome {i} does not match input order");
            assert_eq!(matches!(err, PriorityFeeBelowMinimum { .. }), expect_rejected);
        }
    }

    #[tokio::test]
    async fn new_head_updates_floor_to_next_block_base_fee() {
        let v = validator(FLOOR);
        // A full block, so the next-block base fee rises above the head's own base fee.
        let header = BerachainHeader {
            timestamp: 1000,
            gas_limit: 30_000_000,
            gas_used: 30_000_000,
            base_fee_per_gas: Some(BASE_FEE as u64),
            ..Default::default()
        };
        let pending = u128::from(chain_spec().next_block_base_fee(&header, 1000).unwrap());
        assert!(pending > BASE_FEE);
        let head = SealedBlock::seal_slow(BerachainBlock { header, body: Default::default() });
        v.on_new_head_block(&head);

        // The floor boundary now sits exactly at the derived next-block base fee. Evaluating
        // against the head's own (lower) base fee would let the second transaction through.
        assert!(!rejects(&v, External, eip1559(pending + FLOOR, FLOOR)).await);
        assert!(rejects(&v, External, eip1559(pending + FLOOR - 1, FLOOR)).await);
    }
}
