//! Integration test for the `main.rs` txpool default initialization path.
//!
//! Runs in a separate process so `DefaultTxPoolValues::try_init` is not preceded by
//! `TxPoolArgs::default()` initializing the global with upstream defaults.

use bera_reth::pool::config::{
    assert_berachain_pinned_txpool_args, assert_inherited_unpinned_match_reth_upstream_constants,
    berachain_txpool_defaults,
};
use reth_node_core::args::TxPoolArgs;

#[test]
fn berachain_txpool_defaults_regression() {
    berachain_txpool_defaults()
        .try_init()
        .expect("txpool defaults must initialize before CLI parsing");

    let berachain = TxPoolArgs::default();
    assert_berachain_pinned_txpool_args(&berachain);
    assert_inherited_unpinned_match_reth_upstream_constants(&berachain);
}
