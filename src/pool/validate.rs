//! Berachain-specific transaction pool validation extensions.
//!
//! Upstream reth only enforces `--txpool.minimum-priority-fee` for EIP-1559 (type-2)
//! transactions. Legacy and access-list transactions bypass that check, which lets
//! low-tip spam evade validator-side filters.

use reth_primitives_traits::{BlockHeader, SealedBlock};
use reth_storage_api::BlockReaderIdExt;
use reth_transaction_pool::{
    LocalTransactionConfig, PoolTransaction, TransactionOrigin, TransactionValidationOutcome,
    TransactionValidator,
    error::InvalidPoolTransactionError,
};

/// Returns the configured minimum when a non-EIP-1559 transaction's effective tip is too low.
pub(crate) fn legacy_priority_fee_violation(
    is_dynamic_fee: bool,
    is_local: bool,
    minimum_priority_fee: Option<u128>,
    effective_tip: u128,
) -> Option<u128> {
    let minimum_priority_fee = minimum_priority_fee?;
    if is_local || is_dynamic_fee {
        return None;
    }
    (effective_tip < minimum_priority_fee).then_some(minimum_priority_fee)
}

/// Wraps an inner [`TransactionValidator`] and rejects non-EIP-1559 transactions whose
/// effective priority fee is below the configured minimum.
#[derive(Debug)]
pub struct LegacyMinimumPriorityFeeValidator<V, Client> {
    inner: V,
    client: Client,
    minimum_priority_fee: Option<u128>,
    local_transactions_config: LocalTransactionConfig,
}

impl<V, Client> LegacyMinimumPriorityFeeValidator<V, Client> {
    pub const fn new(
        inner: V,
        client: Client,
        minimum_priority_fee: Option<u128>,
        local_transactions_config: LocalTransactionConfig,
    ) -> Self {
        Self { inner, client, minimum_priority_fee, local_transactions_config }
    }
}

impl<V, Client, Tx> TransactionValidator for LegacyMinimumPriorityFeeValidator<V, Client>
where
    V: TransactionValidator<Transaction = Tx>,
    Client: BlockReaderIdExt,
    Tx: PoolTransaction,
{
    type Transaction = Tx;
    type Block = V::Block;

    async fn validate_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Self::Transaction,
    ) -> TransactionValidationOutcome<Tx> {
        let outcome =
            self.inner.validate_transaction(origin, transaction.clone()).await;

        let TransactionValidationOutcome::Valid { .. } = outcome else {
            return outcome;
        };

        let is_local =
            self.local_transactions_config.is_local(origin, transaction.sender_ref());

        let base_fee = match self.client.latest_header() {
            Ok(Some(header)) => header.base_fee_per_gas().unwrap_or_default(),
            Ok(None) | Err(_) => return outcome,
        };

        let effective_tip = transaction.effective_tip_per_gas(base_fee).unwrap_or(0);
        if let Some(minimum_priority_fee) = legacy_priority_fee_violation(
            transaction.is_dynamic_fee(),
            is_local,
            self.minimum_priority_fee,
            effective_tip,
        ) {
            return TransactionValidationOutcome::Invalid(
                transaction,
                InvalidPoolTransactionError::PriorityFeeBelowMinimum { minimum_priority_fee },
            );
        }

        outcome
    }

    fn on_new_head_block(&self, new_tip_block: &SealedBlock<Self::Block>) {
        self.inner.on_new_head_block(new_tip_block);
    }
}

#[cfg(test)]
mod tests {
    use super::legacy_priority_fee_violation;

    #[test]
    fn rejects_low_legacy_tip() {
        assert_eq!(
            legacy_priority_fee_violation(false, false, Some(10_000_000), 2),
            Some(10_000_000)
        );
    }

    #[test]
    fn accepts_legacy_tip_at_floor() {
        assert_eq!(
            legacy_priority_fee_violation(false, false, Some(10_000_000), 10_000_000),
            None
        );
    }

    #[test]
    fn skips_dynamic_fee_transactions() {
        assert_eq!(
            legacy_priority_fee_violation(true, false, Some(10_000_000), 1),
            None
        );
    }

    #[test]
    fn skips_local_transactions() {
        assert_eq!(
            legacy_priority_fee_violation(false, true, Some(10_000_000), 1),
            None
        );
    }

    #[test]
    fn disabled_when_minimum_unset() {
        assert_eq!(legacy_priority_fee_violation(false, false, None, 1), None);
    }
}
