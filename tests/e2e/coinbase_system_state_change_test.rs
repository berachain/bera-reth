//! Coinbase and system state change test for PoL transactions
//!
//! This test verifies that system calls can modify multiple accounts during execution.
//! Before https://github.com/alloy-rs/evm/pull/121, alloy-evm system calls would only
//! retain state changes for the target contract, not for other addresses modified during
//! the call. This test ensures that transfers to system address and coinbase persist.

use crate::e2e::{POL_DISTRIBUTOR_ADDRESS, berachain_payload_attributes_generator};
use alloy_eips::eip7002::SYSTEM_ADDRESS;
use alloy_genesis::Genesis;
use alloy_network::ReceiptResponse;
use alloy_primitives::{Address, Bytes, U256};
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

sol! {
    #[sol(bytecode = "0x608060405234801561000f575f80fd5b5060043610610029575f3560e01c806360644a6b1461002d575b5f80fd5b61004061003b366004610272565b610042565b005b6040515f9033906001908381818185875af1925050503d805f8114610082576040519150601f19603f3d011682016040523d82523d5f602084013e610087565b606091505b50509050806100f7576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152601960248201527f5472616e7366657220746f2073656e646572206661696c65640000000000000060448201526064015b60405180910390fd5b6040515f9073fffffffffffffffffffffffffffffffffffffffe906001908381818185875af1925050503d805f811461014b576040519150601f19603f3d011682016040523d82523d5f602084013e610150565b606091505b50509050806101bb576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152601960248201527f5472616e7366657220746f2073797374656d206661696c65640000000000000060448201526064016100ee565b6040515f9041906001908381818185875af1925050503d805f81146101fb576040519150601f19603f3d011682016040523d82523d5f602084013e610200565b606091505b505090508061026b576040517f08c379a000000000000000000000000000000000000000000000000000000000815260206004820152601b60248201527f5472616e7366657220746f20636f696e62617365206661696c6564000000000060448201526064016100ee565b5050505050565b5f8060208385031215610283575f80fd5b823567ffffffffffffffff811115610299575f80fd5b8301601f810185136102a9575f80fd5b803567ffffffffffffffff8111156102bf575f80fd5b8560208284010111156102d0575f80fd5b602091909101959094509250505056fea26469706673582212206d4054d287528565948e764c459a234aa9af67786283d912fc16bbb9c91e0b8b64736f6c634300081a0033")]
    contract StateChangeDistributor {
        function distributeFor(bytes calldata /*pubkey*/) public {
            // Transfer 1 wei to msg.sender (which is system address 0xfff...fe in PoL txs)
            (bool success,) = msg.sender.call{value: 1}("");
            require(success, "Transfer to sender failed");

            // Transfer 1 wei to system address 0xfff...fe (same as msg.sender in PoL txs)
            (bool success2,) = address(0xfffffffffffffffffffffffffffffffffffffffe).call{value: 1}("");
            require(success2, "Transfer to system failed");

            // Transfer 1 wei to coinbase (different address)
            (bool success3,) = block.coinbase.call{value: 1}("");
            require(success3, "Transfer to coinbase failed");
        }
    }
}

const CONTRACT_INITIAL_BALANCE: u64 = 100;

async fn setup_test_with_state_change_contract()
-> eyre::Result<(TaskManager, Arc<BerachainChainSpec>)> {
    let tasks = TaskManager::current();

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let mut genesis: Genesis = parse_genesis(&genesis_json)?;

    let pol_address = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;
    let new_bytecode = Bytes::from_str(&StateChangeDistributor::BYTECODE.to_string())?;

    if let Some(account) = genesis.alloc.get_mut(&pol_address) {
        account.code = Some(new_bytecode);
        account.balance = U256::from(CONTRACT_INITIAL_BALANCE);
        println!("Replaced PoL distributor contract with state change version");
        println!("Set contract balance to {CONTRACT_INITIAL_BALANCE} wei");
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
async fn test_pol_coinbase_system_state_changes() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_with_state_change_contract().await?;
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

    let pol_address = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;
    let system_address = SYSTEM_ADDRESS;

    let initial_contract_balance = ctx.rpc.inner.eth_api().balance(pol_address, None).await?;
    let initial_system_balance = ctx.rpc.inner.eth_api().balance(system_address, None).await?;

    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let transactions = &block.body().transactions;
    let coinbase_address = block.header().beneficiary;

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
        return Err(eyre::eyre!("PoL transaction reverted: {tx_hash:#x}"));
    }

    let final_contract_balance = ctx.rpc.inner.eth_api().balance(pol_address, None).await?;
    let final_system_balance = ctx.rpc.inner.eth_api().balance(system_address, None).await?;
    let final_coinbase_balance = ctx.rpc.inner.eth_api().balance(coinbase_address, None).await?;

    let contract_balance_change = initial_contract_balance - final_contract_balance;
    let system_balance_change = final_system_balance - initial_system_balance;

    assert_eq!(
        contract_balance_change,
        U256::from(3),
        "Contract should have transferred 3 wei total"
    );
    assert_eq!(
        system_balance_change,
        U256::from(2),
        "System address should have received 2 wei (1 from msg.sender + 1 from explicit transfer)"
    );
    assert_eq!(final_coinbase_balance, U256::from(1), "Coinbase should have received 1 wei");

    println!(
        "State change test passed: Contract({final_contract_balance}), System({final_system_balance}), Coinbase({final_coinbase_balance})"
    );

    Ok(())
}
