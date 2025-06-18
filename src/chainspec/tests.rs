//! Tests for BerachainChainSpec functionality

use super::*;
use alloy_eips::eip2124::Head;
use alloy_genesis::Genesis;
use reth_chainspec::DEV;

#[test]
fn test_berachain_chain_spec_base_fee_params() {
    let chain_spec = BerachainChainSpec::default();

    // Test base fee params before Prague1
    let params = chain_spec.base_fee_params_at_timestamp(0);
    assert_eq!(params.max_change_denominator, 8);

    // Test base fee params after Prague1
    let params = chain_spec.base_fee_params_at_block(100);
    assert_eq!(params.max_change_denominator, 8);
}

#[test]
fn test_berachain_chain_spec_deposit_contract() {
    let chain_spec = BerachainChainSpec::default();
    let deposit_contract = chain_spec.deposit_contract();

    // Berachain doesn't use deposit contract, should be None
    assert!(deposit_contract.is_none());
}

#[test]
fn test_berachain_chain_spec_hardforks() {
    let chain_spec = BerachainChainSpec::default();

    // Test that hardfork methods don't panic
    let fork_id = chain_spec.fork_id(Head::new(0, 0, Default::default()));
    assert!(fork_id.hash != [0; 4]);

    let latest_fork_id = chain_spec.latest_fork_id();
    assert!(latest_fork_id.hash != [0; 4]);
}

#[test]
fn test_berachain_chain_spec_from_genesis() {
    let genesis = Genesis::default();
    let chain_spec = BerachainChainSpec::from(genesis);

    // Should create a valid chain spec
    assert_eq!(
        *chain_spec.chain().kind(),
        reth_chainspec::ChainKind::Named(reth_chainspec::NamedChain::Dev)
    );
}

#[test]
fn test_berachain_chain_spec_prague1_fork_activation() {
    use crate::hardforks::{BerachainHardfork, BerachainHardforks};

    let chain_spec = BerachainChainSpec::default();

    // Test Prague1 activation at genesis (timestamp 0)
    assert!(
        chain_spec.berachain_fork_activation(BerachainHardfork::Prague1).active_at_timestamp(0)
    );
    assert!(
        chain_spec.berachain_fork_activation(BerachainHardfork::Prague1).active_at_timestamp(100)
    );
}

#[test]
fn test_berachain_chain_spec_genesis_header() {
    let chain_spec = BerachainChainSpec::default();
    let genesis_header = chain_spec.genesis_header();

    // Should have a valid genesis header
    assert_eq!(genesis_header.number, 0);
    assert_eq!(genesis_header.gas_limit, 30_000_000);
}

#[test]
fn test_berachain_chain_spec_pruning() {
    let chain_spec = BerachainChainSpec::default();

    // Test prune delete limit
    let prune_limit = chain_spec.prune_delete_limit();
    assert!(prune_limit > 0);
}

#[test]
fn test_berachain_chain_spec_next_block_base_fee() {
    let chain_spec = BerachainChainSpec::default();
    let genesis_header = chain_spec.genesis_header();

    // Test next block base fee calculation
    let next_base_fee = chain_spec.next_block_base_fee(&genesis_header);
    assert!(next_base_fee > 0);
}
