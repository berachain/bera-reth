//! Berachain transaction pool defaults.
//!
//! Pinned against reth `v1.11.4`. Upstream regression tests fail on dependency bumps
//! when Reth changes a default — review, then adopt (update pins) or override.

use reth_node_core::args::{DefaultTxPoolValues, TxPoolArgs};
use std::time::Duration;

/// BRIP-0010: Osaka does not adopt EIP-7594 (PeerDAS); reject v1 sidecars, keep EIP-4844.
pub const BERACHAIN_ACCEPTS_EIP7594: bool = false;

pub const SUBPOOL_MAX_TXS: usize = 10_000;
pub const SUBPOOL_MAX_SIZE_MB: usize = 20;
pub const MAX_ACCOUNT_SLOTS: usize = 16;
pub const PRICE_BUMP: u128 = 10;
pub const BLOB_REPLACE_PRICE_BUMP: u128 = 100;
/// Inherited from reth v1.11.4 (`MIN_PROTOCOL_BASE_FEE`). Berachain chain min is 1 gwei.
pub const MINIMAL_PROTOCOL_BASEFEE: u64 = 7;
pub const ENFORCED_GAS_LIMIT: u64 = 30_000_000;
pub const MAX_TX_INPUT_BYTES: usize = 128 * 1024;
pub const MAX_QUEUED_LIFETIME: Duration = Duration::from_secs(3 * 60 * 60);

/// Inherited from reth v1.11.4 (`DefaultTxPoolValues::default().max_batch_size`).
/// No upstream named constant; validated live in `tests/txpool_defaults.rs`.
#[doc(hidden)]
pub const INHERITED_TXPOOL_MAX_BATCH_SIZE: usize = 1;

struct BerachainPinnedTxPoolLimits;

impl BerachainPinnedTxPoolLimits {
    fn apply_to_defaults(self, defaults: DefaultTxPoolValues) -> DefaultTxPoolValues {
        defaults
            .with_pending_max_count(SUBPOOL_MAX_TXS)
            .with_pending_max_size(SUBPOOL_MAX_SIZE_MB)
            .with_basefee_max_count(SUBPOOL_MAX_TXS)
            .with_basefee_max_size(SUBPOOL_MAX_SIZE_MB)
            .with_queued_max_count(SUBPOOL_MAX_TXS)
            .with_queued_max_size(SUBPOOL_MAX_SIZE_MB)
            .with_blobpool_max_count(SUBPOOL_MAX_TXS)
            .with_blobpool_max_size(SUBPOOL_MAX_SIZE_MB)
            .with_max_account_slots(MAX_ACCOUNT_SLOTS)
            .with_price_bump(PRICE_BUMP)
            .with_minimal_protocol_basefee(MINIMAL_PROTOCOL_BASEFEE)
            .with_enforced_gas_limit(ENFORCED_GAS_LIMIT)
            .with_blob_transaction_price_bump(BLOB_REPLACE_PRICE_BUMP)
            .with_max_tx_input_bytes(MAX_TX_INPUT_BYTES)
            .with_max_queued_lifetime(MAX_QUEUED_LIFETIME)
    }

    #[cfg(test)]
    fn apply_to_args(self, args: &mut TxPoolArgs) {
        args.pending_max_count = SUBPOOL_MAX_TXS;
        args.pending_max_size = SUBPOOL_MAX_SIZE_MB;
        args.basefee_max_count = SUBPOOL_MAX_TXS;
        args.basefee_max_size = SUBPOOL_MAX_SIZE_MB;
        args.queued_max_count = SUBPOOL_MAX_TXS;
        args.queued_max_size = SUBPOOL_MAX_SIZE_MB;
        args.blobpool_max_count = SUBPOOL_MAX_TXS;
        args.blobpool_max_size = SUBPOOL_MAX_SIZE_MB;
        args.max_account_slots = MAX_ACCOUNT_SLOTS;
        args.price_bump = PRICE_BUMP;
        args.minimal_protocol_basefee = MINIMAL_PROTOCOL_BASEFEE;
        args.enforced_gas_limit = ENFORCED_GAS_LIMIT;
        args.blob_transaction_price_bump = BLOB_REPLACE_PRICE_BUMP;
        args.max_tx_input_bytes = MAX_TX_INPUT_BYTES;
        args.max_queued_lifetime = MAX_QUEUED_LIFETIME;
    }
}

/// Berachain txpool defaults applied before CLI parsing in `main.rs`.
pub fn berachain_txpool_defaults() -> DefaultTxPoolValues {
    BerachainPinnedTxPoolLimits.apply_to_defaults(DefaultTxPoolValues::default())
}

/// Asserts every txpool CLI field pinned by [`berachain_txpool_defaults`].
#[doc(hidden)]
pub fn assert_berachain_pinned_txpool_args(args: &TxPoolArgs) {
    assert_eq!(args.pending_max_count, SUBPOOL_MAX_TXS);
    assert_eq!(args.pending_max_size, SUBPOOL_MAX_SIZE_MB);
    assert_eq!(args.basefee_max_count, SUBPOOL_MAX_TXS);
    assert_eq!(args.basefee_max_size, SUBPOOL_MAX_SIZE_MB);
    assert_eq!(args.queued_max_count, SUBPOOL_MAX_TXS);
    assert_eq!(args.queued_max_size, SUBPOOL_MAX_SIZE_MB);
    assert_eq!(args.blobpool_max_count, SUBPOOL_MAX_TXS);
    assert_eq!(args.blobpool_max_size, SUBPOOL_MAX_SIZE_MB);
    assert_eq!(args.max_account_slots, MAX_ACCOUNT_SLOTS);
    assert_eq!(args.price_bump, PRICE_BUMP);
    assert_eq!(args.minimal_protocol_basefee, MINIMAL_PROTOCOL_BASEFEE);
    assert_eq!(args.enforced_gas_limit, ENFORCED_GAS_LIMIT);
    assert_eq!(args.blob_transaction_price_bump, BLOB_REPLACE_PRICE_BUMP);
    assert_eq!(args.max_tx_input_bytes, MAX_TX_INPUT_BYTES);
    assert_eq!(args.max_queued_lifetime, MAX_QUEUED_LIFETIME);
}

/// Asserts unpinned txpool CLI fields match current Reth upstream defaults.
#[doc(hidden)]
pub fn assert_inherited_unpinned_match_reth_upstream_constants(args: &TxPoolArgs) {
    use reth_transaction_pool::{
        blobstore::disk::DEFAULT_MAX_CACHED_BLOBS,
        pool::{NEW_TX_LISTENER_BUFFER_SIZE, PENDING_TX_LISTENER_BUFFER_SIZE},
        DEFAULT_TXPOOL_ADDITIONAL_VALIDATION_TASKS, MAX_NEW_PENDING_TXS_NOTIFICATIONS,
    };

    assert_eq!(args.additional_validation_tasks, DEFAULT_TXPOOL_ADDITIONAL_VALIDATION_TASKS);
    assert_eq!(args.max_cached_entries, DEFAULT_MAX_CACHED_BLOBS);
    assert_eq!(args.pending_tx_listener_buffer_size, PENDING_TX_LISTENER_BUFFER_SIZE);
    assert_eq!(args.new_tx_listener_buffer_size, NEW_TX_LISTENER_BUFFER_SIZE);
    assert_eq!(args.max_new_pending_txs_notifications, MAX_NEW_PENDING_TXS_NOTIFICATIONS);
    assert_eq!(args.max_batch_size, INHERITED_TXPOOL_MAX_BATCH_SIZE);
    assert!(!args.disable_blobs_support);
    assert_eq!(args.blob_cache_size, None);
    assert_eq!(args.minimum_priority_fee, None);
    assert_eq!(args.max_tx_gas_limit, None);
    assert!(!args.no_locals);
    assert!(args.locals.is_empty());
    assert!(!args.no_local_transactions_propagation);
    assert_eq!(args.transactions_backup_path, None);
    assert!(!args.disable_transactions_backup);
}

#[cfg(test)]
fn berachain_txpool_args() -> TxPoolArgs {
    let mut args = TxPoolArgs::default();
    BerachainPinnedTxPoolLimits.apply_to_args(&mut args);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_node_core::cli::config::RethTransactionPoolConfig;
    use reth_transaction_pool::{PriceBumpConfig, SubPoolLimit};

    const SUBPOOL_MAX_BYTES: usize = SUBPOOL_MAX_SIZE_MB * 1024 * 1024;

    /// Fails when a pinned Berachain limit no longer matches the current Reth upstream default.
    /// On failure: adopt Reth's new value (update constants) or keep Berachain's choice (drop assertion).
    #[test]
    fn berachain_pinned_limits_match_reth_upstream() {
        use alloy_eips::eip1559::{ETHEREUM_BLOCK_GAS_LIMIT_30M, MIN_PROTOCOL_BASE_FEE};
        use reth_transaction_pool::{
            maintain::MAX_QUEUED_TRANSACTION_LIFETIME, validate::DEFAULT_MAX_TX_INPUT_BYTES,
            DEFAULT_PRICE_BUMP, REPLACE_BLOB_PRICE_BUMP, TXPOOL_MAX_ACCOUNT_SLOTS_PER_SENDER,
            TXPOOL_SUBPOOL_MAX_SIZE_MB_DEFAULT, TXPOOL_SUBPOOL_MAX_TXS_DEFAULT,
        };

        assert_eq!(SUBPOOL_MAX_TXS, TXPOOL_SUBPOOL_MAX_TXS_DEFAULT);
        assert_eq!(SUBPOOL_MAX_SIZE_MB, TXPOOL_SUBPOOL_MAX_SIZE_MB_DEFAULT);
        assert_eq!(MAX_ACCOUNT_SLOTS, TXPOOL_MAX_ACCOUNT_SLOTS_PER_SENDER);
        assert_eq!(PRICE_BUMP, DEFAULT_PRICE_BUMP);
        assert_eq!(BLOB_REPLACE_PRICE_BUMP, REPLACE_BLOB_PRICE_BUMP);
        assert_eq!(MINIMAL_PROTOCOL_BASEFEE, MIN_PROTOCOL_BASE_FEE);
        assert_eq!(ENFORCED_GAS_LIMIT, ETHEREUM_BLOCK_GAS_LIMIT_30M);
        assert_eq!(MAX_TX_INPUT_BYTES, DEFAULT_MAX_TX_INPUT_BYTES);
        assert_eq!(MAX_QUEUED_LIFETIME, MAX_QUEUED_TRANSACTION_LIFETIME);
    }

    /// Fails when Reth changes an unpinned txpool default that Berachain still inherits.
    #[test]
    fn berachain_inherited_unpinned_limits_match_reth_upstream() {
        assert_inherited_unpinned_match_reth_upstream_constants(&berachain_txpool_args());
    }

    #[test]
    fn berachain_pinned_txpool_args_regression() {
        assert_berachain_pinned_txpool_args(&berachain_txpool_args());
    }

    #[test]
    fn berachain_pool_config_limits_regression() {
        let args = berachain_txpool_args();
        assert_berachain_pinned_txpool_args(&args);
        let config = args.pool_config();

        assert_eq!(
            config.pending_limit,
            SubPoolLimit::new(SUBPOOL_MAX_TXS, SUBPOOL_MAX_BYTES)
        );
        assert_eq!(
            config.basefee_limit,
            SubPoolLimit::new(SUBPOOL_MAX_TXS, SUBPOOL_MAX_BYTES)
        );
        assert_eq!(
            config.queued_limit,
            SubPoolLimit::new(SUBPOOL_MAX_TXS, SUBPOOL_MAX_BYTES)
        );
        assert_eq!(
            config.blob_limit,
            SubPoolLimit::new(SUBPOOL_MAX_TXS, SUBPOOL_MAX_BYTES)
        );
        assert_eq!(config.max_account_slots, MAX_ACCOUNT_SLOTS);
        assert_eq!(
            config.price_bumps,
            PriceBumpConfig {
                default_price_bump: PRICE_BUMP,
                replace_blob_tx_price_bump: BLOB_REPLACE_PRICE_BUMP,
            }
        );
        assert_eq!(config.minimal_protocol_basefee, MINIMAL_PROTOCOL_BASEFEE);
        assert_eq!(config.gas_limit, ENFORCED_GAS_LIMIT);
        assert_eq!(config.minimum_priority_fee, None);
        assert_eq!(config.max_queued_lifetime, MAX_QUEUED_LIFETIME);
        assert_eq!(config.blob_cache_size, None);
        assert!(!config.local_transactions_config.no_exemptions);
        assert!(config.local_transactions_config.local_addresses.is_empty());
        assert!(config.local_transactions_config.propagate_local_transactions);
    }

    #[test]
    fn berachain_rejects_eip7594_sidecars() {
        assert!(!BERACHAIN_ACCEPTS_EIP7594);
    }
}
