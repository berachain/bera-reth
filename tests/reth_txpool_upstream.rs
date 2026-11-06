//! Validates inherited defaults that Reth does not export as named constants.
//!
//! Must remain the only test in this binary so `TxPoolArgs::default()` observes upstream
//! defaults before Berachain calls `try_init`.

use bera_reth::pool::config::INHERITED_TXPOOL_MAX_BATCH_SIZE;
use reth_node_core::args::TxPoolArgs;

#[test]
fn inherited_max_batch_size_matches_reth_upstream() {
    assert_eq!(TxPoolArgs::default().max_batch_size, INHERITED_TXPOOL_MAX_BATCH_SIZE);
}
