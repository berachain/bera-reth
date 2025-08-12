//! Gas limit regression tests for PoL transactions
//!
//! These tests verify that the 30M gas limit for system calls in REVM remains exactly 30,000,000.
//! Any deviation from this exact value will cause the test to fail.
//!
//! Context: REVM hardcodes a 30M gas limit for system calls in the handler:
//! https://github.com/bluealloy/revm/blob/f3c794b4df282d8053d60e67bca5c4a306031357/crates/handler/src/system_call.rs#L65

use crate::e2e::{POL_DISTRIBUTOR_ADDRESS, berachain_payload_attributes_generator};
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

// Strict gas limit validation contract for regression testing
// This contract expects exactly 29,999,646 gas remaining at execution time.
// This precise value corresponds to the 30M system call gas limit minus the gas
// consumed by transaction processing and function call overhead.
sol! {
    #[sol(bytecode = "0x608060405234801561000f575f80fd5b5060043610610029575f3560e01c806360644a6b1461002d575b5f80fd5b61004061003b366004610095565b610042565b005b5f5a9050806301c9c21e146100905760405162461bcd60e51b815260206004820152601060248201526f496e73756666696369656e742067617360801b604482015260640160405180910390fd5b505050565b5f80602083850312156100a6575f80fd5b823567ffffffffffffffff8111156100bc575f80fd5b8301601f810185136100cc575f80fd5b803567ffffffffffffffff8111156100e2575f80fd5b8560208284010111156100f3575f80fd5b602091909101959094509250505056fea2646970667358221220d5e6c7321d98a40b9a775d16eed1a1ef1182f64c7e8bf81f724feef606d28dde64736f6c634300081a0033")]
    contract SimplePoLDistributor {
            function distributeFor(bytes calldata /*pubkey*/) public {
              uint256 start_gas = gasleft();
              // Strict validation: exactly 29,999,646 gas must remain at this point
              // Any change to REVM's 30M system call limit will cause this to fail
              require(start_gas == 29_999_646, "Insufficient gas");
        }
    }
}

async fn setup_test_with_gas_boundary_pol_contract()
-> eyre::Result<(TaskManager, Arc<BerachainChainSpec>)> {
    let tasks = TaskManager::current();

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let mut genesis: Genesis = parse_genesis(&genesis_json)?;
    let pol_address = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;
    let new_bytecode = Bytes::from_str(&SimplePoLDistributor::BYTECODE.to_string())?;

    if let Some(account) = genesis.alloc.get_mut(&pol_address) {
        account.code = Some(new_bytecode);
        println!("Replaced PoL distributor contract with strict gas limit validator");
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
async fn test_pol_gas_limit_is_30_million() -> eyre::Result<()> {
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

    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let transactions = &block.body().transactions;

    assert!(!transactions.is_empty(), "Block should contain at least one PoL transaction");
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

    if !receipt.status() {
        return Err(eyre::eyre!(
            "PoL transaction reverted: {tx_hash:#x}. REVM's 30M system call gas limit may have changed."
        ));
    }

    println!("PoL gas limit validation passed");
    println!("Block: {}, Transactions: {}, Hash: {tx_hash:#x}", block.number, transactions.len());

    Ok(())
}
