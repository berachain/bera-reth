//! End-to-end integration tests for Bera-Reth node
//!
//! These tests follow Reth's e2e testing patterns, using NodeTestContext
//! for comprehensive integration testing with real RPC servers and full
//! blockchain state.

use alloy_primitives::{Address, B256};
use alloy_signer_local::PrivateKeySigner;
use bera_reth::{
    chainspec::BerachainChainSpec,
    engine::payload::{BerachainPayloadAttributes, BerachainPayloadBuilderAttributes},
    primitives::header::BlsPublicKey,
};
use reth::tasks::TaskManager;
use reth_cli::chainspec::parse_genesis;
use reth_ethereum_engine_primitives::EthPayloadAttributes;
use reth_payload_primitives::PayloadBuilderAttributes;
use std::{str::FromStr, sync::Arc};

pub mod transaction_tests;

const TEST_PRIVATE_KEY: &str = "0xfffdbb37105441e14b0ee6330d855d8504ff39e705c3afa8f859ac9865f99306";

/// Setup test node boilerplate - returns TaskManager and chain spec for individual test setup
pub async fn setup_test_boilerplate() -> eyre::Result<(TaskManager, Arc<BerachainChainSpec>)> {
    let tasks = TaskManager::current();

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path).expect("Failed to read genesis file");
    let genesis = parse_genesis(&genesis_json).expect("Failed to parse genesis");
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));

    Ok((tasks, chain_spec))
}

/// Create a test signer from the constant private key
pub fn test_signer() -> eyre::Result<PrivateKeySigner> {
    let private_key = B256::from_str(TEST_PRIVATE_KEY)?;
    Ok(PrivateKeySigner::from_bytes(&private_key)?)
}

/// Create Berachain payload attributes for testing
pub fn berachain_payload_attributes(timestamp: u64) -> BerachainPayloadBuilderAttributes {
    let eth_attributes = EthPayloadAttributes {
        timestamp,
        prev_randao: B256::random(),
        suggested_fee_recipient: Address::random(),
        withdrawals: Some(vec![]),
        parent_beacon_block_root: Some(B256::random()),
    };
    let berachain_attributes = BerachainPayloadAttributes {
        inner: eth_attributes,
        prev_proposer_pubkey: Some(BlsPublicKey::random()),
    };
    BerachainPayloadBuilderAttributes::try_new(B256::ZERO, berachain_attributes, 1).unwrap()
}
