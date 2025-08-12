//! PoL transaction revert test
//!
//! This test verifies that when a PoL contract reverts during execution,
//! the PoL transaction is still included in the block but marked as failed.
//! This ensures that block production continues even when PoL distribution fails.

use crate::e2e::{POL_DISTRIBUTOR_ADDRESS, berachain_payload_attributes_generator};
use alloy_genesis::Genesis;
use alloy_network::ReceiptResponse;
use alloy_primitives::{Address, Bytes};
use alloy_sol_macro::sol;
use bera_reth::{
    chainspec::BerachainChainSpec, node::BerachainNode, transaction::BerachainTxEnvelope,
};
use reth::{providers::BlockNumReader, rpc::api::EthApiServer, tasks::TaskManager};
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::{str::FromStr, sync::Arc};

sol! {
    #[sol(bytecode = "0x608060405234801561000f575f80fd5b5060043610610029575f3560e01c806360644a6b1461002d575b5f80fd5b61004061003b3660046100a8565b610042565b005b6040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152601760248201527f506f4c20646973747269627574696f6e206661696c6564000000000000000000604482015260640160405180910390fd5b5f80602083850312156100b9575f80fd5b823567ffffffffffffffff8111156100cf575f80fd5b8301601f810185136100df575f80fd5b803567ffffffffffffffff8111156100f5575f80fd5b856020828401011115610106575f80fd5b602091909101959094509250505056fea26469706673582212209d6343a1dea4d476cfaebe6dd2a17f309334fa544b1563edf06688dac8203f4964736f6c634300081a0033")]
    contract RevertingPoLDistributor {
        function distributeFor(bytes calldata /*pubkey*/) public {
            revert("PoL distribution failed");
        }
    }
}

async fn setup_test_with_reverting_pol_contract()
-> eyre::Result<(TaskManager, Arc<BerachainChainSpec>)> {
    let tasks = TaskManager::current();

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let mut genesis: Genesis = parse_genesis(&genesis_json)?;

    let pol_address = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;
    let reverting_bytecode = Bytes::from_str(&RevertingPoLDistributor::BYTECODE.to_string())?;

    if let Some(account) = genesis.alloc.get_mut(&pol_address) {
        account.code = Some(reverting_bytecode);
        println!("Replaced PoL distributor contract with reverting version");
    } else {
        return Err(eyre::eyre!(
            "PoL distributor contract not found at {} in genesis file",
            POL_DISTRIBUTOR_ADDRESS
        ));
    }

    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));
    Ok((tasks, chain_spec))
}

#[tokio::test]
async fn test_pol_transaction_revert_still_included() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_with_reverting_pol_contract().await?;
    let executor = tasks.executor();

    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(executor.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;

    let initial_block = ctx.rpc.inner.eth_api().provider().best_block_number()?;

    // Mine a block - this will trigger PoL transaction creation
    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let transactions = &block.body().transactions;

    // Verify block was mined successfully despite PoL transaction reverting
    assert!(block.number > initial_block, "Block number should advance");
    assert!(!transactions.is_empty(), "Block should contain at least one PoL transaction");

    // Verify first transaction is a PoL transaction
    let first_tx = &transactions[0];
    assert!(
        matches!(first_tx, BerachainTxEnvelope::Berachain(_)),
        "First transaction should be a PoL transaction"
    );

    let tx_hash = *first_tx.hash();
    let receipt = ctx
        .rpc
        .inner
        .eth_api()
        .transaction_receipt(tx_hash)
        .await?
        .ok_or_else(|| eyre::eyre!("Receipt not found for PoL transaction"))?;

    // Key assertion: PoL transaction should be included but marked as failed
    assert!(!receipt.status(), "PoL transaction should be marked as failed (status: false)");

    // System call always used 0 gas
    assert_eq!(receipt.gas_used(), 0, "Reverted PoL transaction should consume 0 gas");

    println!("PoL transaction revert test passed");
    println!(
        "Block: {}, Hash: {tx_hash:#x}, Status: {}, Gas: {}",
        block.number,
        receipt.status(),
        receipt.gas_used()
    );

    Ok(())
}
