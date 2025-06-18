//! Tests for Berachain node types and functionality

use super::BerachainNode;
use crate::chainspec::BerachainChainSpec;

#[test]
fn test_berachain_chain_spec_integration() {
    // Test that our types work together
    let _spec = BerachainChainSpec::default();

    // This test ensures our types compile together correctly
    fn _compile_test() -> BerachainChainSpec {
        BerachainChainSpec::default()
    }

    let _ = _compile_test();
}

#[test]
fn test_berachain_node_instantiation() {
    let _node = BerachainNode;
    // Test that BerachainNode can be instantiated
}
