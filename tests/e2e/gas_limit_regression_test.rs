//! Gas limit regression tests for PoL transactions
//!
//! These tests verify that the 30M gas limit for system calls in REVM
//! remains compatible with PoL transaction execution.

use crate::e2e::berachain_payload_attributes_generator;
use alloy_genesis::Genesis;
use alloy_network::ReceiptResponse;
use alloy_primitives::{Address, Bytes};
use alloy_sol_macro::sol;
use bera_reth::{
    chainspec::BerachainChainSpec, node::BerachainNode, transaction::BerachainTxEnvelope,
};
use reth::{rpc::api::EthApiServer, tasks::TaskManager};
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::{str::FromStr, sync::Arc};

// Gas boundary testing PoL distributor contract for regression testing
sol! {
    #[sol(bytecode = "0x608060405234801561000f575f80fd5b5060043610610029575f3560e01c806360644a6b1461002d575b5f80fd5b61004061003b3660046100da565b610042565b005b5f5a90506301c900308110156100925760405162461bcd60e51b815260206004820152601060248201526f496e73756666696369656e742067617360801b60448201526064015b60405180910390fd5b6301c9c3808111156100d55760405162461bcd60e51b815260206004820152600c60248201526b546f6f206d7563682067617360a01b6044820152606401610089565b505050565b5f80602083850312156100eb575f80fd5b823567ffffffffffffffff811115610101575f80fd5b8301601f81018513610111575f80fd5b803567ffffffffffffffff811115610127575f80fd5b856020828401011115610138575f80fd5b602091909101959094509250505056fea2646970667358221220520bb1eea6ca1b15920f93b3c22dc56d139dc7bf299271a290231604bd3bc5b464736f6c634300081a0033")]
    contract SimplePoLDistributor {
        /// This contract validates the 30M system call gas limit boundary
        /// It requires exactly 29.95M-30M gas to ensure we're testing at the limit
        function distributeFor(bytes calldata /*pubkey*/) public {
            uint256 start_gas = gasleft();
            require(start_gas >= 29_950_000, "Insufficient gas");
            require(start_gas <= 30_000_000, "Too much gas");
        }
    }
}

/// PoL distributor contract address
const POL_DISTRIBUTOR_ADDRESS: &str = "0x4200000000000000000000000000000000000042";

/// Create a custom chainspec with the gas boundary validation PoL distributor contract
async fn setup_test_with_gas_boundary_pol_contract()
-> eyre::Result<(TaskManager, Arc<BerachainChainSpec>)> {
    let tasks = TaskManager::current();

    // Load the base genesis file
    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let mut genesis: Genesis = parse_genesis(&genesis_json)?;

    // Replace the PoL distributor contract with our gas-heavy version
    let pol_address = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;
    let new_bytecode = Bytes::from_str(&SimplePoLDistributor::BYTECODE.to_string())?;

    if let Some(account) = genesis.alloc.get_mut(&pol_address) {
        account.code = Some(new_bytecode);
        println!("✅ Replaced PoL distributor contract with gas boundary validator");
    } else {
        // If the PoL contract doesn't exist in genesis, this test cannot proceed
        return Err(eyre::eyre!(
            "PoL distributor contract not found at {} in genesis file. \
             This test requires the contract to exist for replacement.",
            POL_DISTRIBUTOR_ADDRESS
        ));
    }

    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));
    Ok((tasks, chain_spec))
}

#[tokio::test]
async fn test_pol_gas_limit_boundary_succeeds() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_with_gas_boundary_pol_contract().await?;
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

    println!("🚀 Testing PoL transaction with 29.9M gas contract...");

    // Advance a block - this should create and execute a PoL transaction
    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let transactions = &block.body().transactions;

    // Verify we have transactions (should include the PoL tx)
    assert!(!transactions.is_empty(), "Block should contain at least one PoL transaction");

    // Verify the first transaction is a PoL transaction and did not revert
    let first_tx = &transactions[0];
    assert!(
        matches!(first_tx, BerachainTxEnvelope::Berachain(_)),
        "First transaction should be a PoL transaction"
    );

    // Query the transaction receipt via RPC to verify it didn't revert
    let tx_hash = *first_tx.hash();
    let receipt = ctx
        .rpc
        .inner
        .eth_api()
        .transaction_receipt(tx_hash)
        .await?
        .ok_or_else(|| eyre::eyre!("Receipt not found for PoL transaction"))?;

    assert!(
        receipt.status(),
        "PoL transaction should not have reverted. This indicates the gas boundary validation failed."
    );

    println!("✅ PoL transaction with gas boundary validation executed successfully!");
    println!("   Block number: {}", block.number);
    println!("   Transaction count: {}", transactions.len());
    println!("   PoL transaction hash: {tx_hash:#x}");

    // Log all receipt fields
    println!("📋 Transaction Receipt Details:");
    println!("   transaction_hash: {:#x}", receipt.transaction_hash);
    println!("   transaction_index: {:?}", receipt.transaction_index);
    println!("   block_hash: {:?}", receipt.block_hash.map(|h| format!("{h:#x}")));
    println!("   block_number: {:?}", receipt.block_number);
    println!("   gas_used: {}", receipt.gas_used);
    println!("   cumulative_gas_used: {}", receipt.cumulative_gas_used());
    println!("   effective_gas_price: {}", receipt.effective_gas_price);
    println!("   from: {:#x}", receipt.from);
    println!("   to: {:?}", receipt.to.map(|addr| format!("{addr:#x}")));
    println!(
        "   contract_address: {:?}",
        receipt.contract_address.map(|addr| format!("{addr:#x}"))
    );
    println!("   status: {}", receipt.status());
    println!("   logs count: {}", receipt.logs().len());
    println!("   inner envelope: {:?}", receipt.inner);

    Ok(())
}
