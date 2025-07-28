//! End-to-end integration tests for Bera-Reth node
//!
//! These tests follow Reth's e2e testing patterns, using NodeTestContext
//! for comprehensive integration testing with real RPC servers and full
//! blockchain state.

pub mod pol_transactions;
// pub mod rpc_integration;

use bera_reth::engine::BerachainEngineTypes;
use reth_chainspec::ChainSpec;
use reth_e2e_test_utils::testsuite::setup::{NetworkSetup, Setup};
use std::{sync::Arc, time::Duration};

/// Standard Berachain test setup for e2e tests
pub fn berachain_test_setup() -> Setup<BerachainEngineTypes> {
    Setup::default()
        .with_chain_spec(berachain_test_chain_spec())
        .with_network(NetworkSetup::single_node())
}

/// Berachain test chain specification with Prague1 active at genesis
pub fn berachain_test_chain_spec() -> Arc<ChainSpec> {
    // TODO: Use proper Berachain chain spec - for now use dev/test spec
    // This should be replaced with actual BerachainChainSpec when available
    reth_chainspec::DEV.clone()
}
