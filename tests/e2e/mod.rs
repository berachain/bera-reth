//! End-to-end integration tests for Bera-Reth node
//!
//! These tests follow Reth's e2e testing patterns, using NodeTestContext
//! for comprehensive integration testing with real RPC servers and full
//! blockchain state.

use alloy_primitives::{Address, B256};
use bera_reth::{
    engine::payload::{BerachainPayloadAttributes, BerachainPayloadBuilderAttributes},
    primitives::header::BlsPublicKey,
};
use reth_ethereum_engine_primitives::EthPayloadAttributes;
use reth_payload_primitives::PayloadBuilderAttributes;

pub mod pol_transactions;

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
