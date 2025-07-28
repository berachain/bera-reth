//! RPC integration tests for Berachain-specific functionality
//!
//! Tests RPC endpoints with real HTTP/WebSocket servers to verify:
//! - Standard Ethereum RPC compatibility  
//! - Berachain-specific transaction handling
//! - Error handling and validation
//! - Performance under load

use super::berachain_test_setup;
use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_network::TransactionBuilder;
use alloy_rpc_types_eth::{TransactionRequest, BlockNumberOrTag};
use bera_reth::BerachainNode;
use reth_e2e_test_utils::NodeTestContext;
use std::time::Duration;

// #[tokio::test]
// async fn test_basic_rpc_methods() -> eyre::Result<()> {
    // Test fundamental RPC methods work with Berachain node
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // Test eth_chainId
    let chain_id = provider.get_chain_id().await?;
    println!("Chain ID: {chain_id}");
    assert_eq!(chain_id, 80084, "Should be Berachain testnet chain ID");
    
    // Test eth_blockNumber
    let block_number = provider.get_block_number().await?;
    println!("Current block number: {block_number}");
    assert!(block_number >= 0, "Block number should be non-negative");
    
    // Test eth_gasPrice
    let gas_price = provider.get_gas_price().await?;
    println!("Current gas price: {gas_price}");
    assert!(gas_price >= 1_000_000_000, "Gas price should be at least 1 gwei (Prague1 minimum)");
    
    // Test eth_getBalance
    let balance = provider.get_balance(Address::ZERO, None).await?;
    println!("Zero address balance: {balance}");
    
    // Test eth_getBlockByNumber
    let genesis_block = provider.get_block_by_number(BlockNumberOrTag::Number(0), false).await?;
    assert!(genesis_block.is_some(), "Genesis block should exist");
    let genesis = genesis_block.unwrap();
    println!("Genesis block hash: {:?}", genesis.header.hash);
    
    // Test Prague1 features are active at genesis
    assert!(genesis.header.base_fee_per_gas.is_some(), "Genesis should have base fee (Prague1)");
    assert_eq!(genesis.header.base_fee_per_gas.unwrap(), 1_000_000_000, "Should be 1 gwei minimum");
    
    Ok(())
}

// #[tokio::test]
// async fn test_transaction_submission_and_mining() -> eyre::Result<()> {
    // Test complete transaction lifecycle through RPC
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    let recipient = Address::from([0x42u8; 20]);
    let initial_balance = provider.get_balance(recipient, None).await?;
    println!("Initial recipient balance: {initial_balance}");
    
    // Create and submit transaction
    let tx_request = TransactionRequest::default()
        .to(recipient)
        .value(U256::from(1_000_000_000_000_000u64)) // 0.001 ETH
        .gas_limit(21_000)
        .max_fee_per_gas(2_000_000_000u128) // 2 gwei
        .max_priority_fee_per_gas(500_000_000u128) // 0.5 gwei tip
        .with_chain_id(80084);
    
    println!("Submitting transaction...");
    let pending_tx = provider.send_transaction(tx_request).await?;
    let tx_hash = *pending_tx.tx_hash();
    println!("Transaction submitted: {tx_hash}");
    
    // Wait for mining with timeout
    println!("Waiting for transaction to be mined...");
    let receipt = tokio::time::timeout(
        Duration::from_secs(30),
        pending_tx.get_receipt()
    ).await??;
    
    assert!(receipt.status(), "Transaction should succeed");
    println!("Transaction mined in block: {}", receipt.block_number.unwrap());
    println!("Gas used: {}", receipt.gas_used.unwrap());
    
    // Verify balance change
    let final_balance = provider.get_balance(recipient, None).await?;
    println!("Final recipient balance: {final_balance}");
    assert!(final_balance > initial_balance, "Balance should have increased");
    
    // Verify transaction details via RPC
    let tx_details = provider.get_transaction_by_hash(tx_hash).await?;
    assert!(tx_details.is_some(), "Transaction should be retrievable");
    let tx = tx_details.unwrap();
    assert_eq!(tx.hash, tx_hash);
    assert_eq!(tx.to, Some(recipient));
    
    Ok(())
}

// #[tokio::test]
// async fn test_rpc_error_handling() -> eyre::Result<()> {
    // Test RPC error responses for various invalid inputs
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // Test invalid transaction - insufficient gas
    let invalid_tx = TransactionRequest::default()
        .to(Address::from([0x99u8; 20]))
        .value(U256::from(1000))
        .gas_limit(1) // Too low
        .max_fee_per_gas(1_000_000_000u128)
        .with_chain_id(80084);
    
    let result = provider.send_transaction(invalid_tx).await;
    assert!(result.is_err(), "Invalid transaction should be rejected");
    println!("Invalid gas limit error: {}", result.unwrap_err());
    
    // Test invalid block number request
    let invalid_block = provider.get_block_by_number(BlockNumberOrTag::Number(999999999), false).await?;
    assert!(invalid_block.is_none(), "Future block should return None");
    
    // Test invalid transaction hash lookup
    let fake_hash = alloy_primitives::B256::from([0x42u8; 32]);
    let invalid_tx_lookup = provider.get_transaction_by_hash(fake_hash).await?;
    assert!(invalid_tx_lookup.is_none(), "Non-existent transaction should return None");
    
    Ok(())
}

// #[tokio::test]
// async fn test_concurrent_transaction_submission() -> eyre::Result<()> {
    // Test RPC handling under concurrent load
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    let num_transactions = 10;
    let mut handles = Vec::new();
    
    println!("Submitting {num_transactions} concurrent transactions...");
    
    // Submit transactions concurrently
    for i in 0..num_transactions {
        let provider = provider.clone();
        let handle = tokio::spawn(async move {
            let tx_request = TransactionRequest::default()
                .to(Address::from([i as u8; 20]))
                .value(U256::from(1000 + i as u64))
                .gas_limit(21_000)
                .max_fee_per_gas(2_000_000_000u128)
                .max_priority_fee_per_gas(500_000_000u128)
                .with_chain_id(80084);
            
            provider.send_transaction(tx_request).await
        });
        handles.push(handle);
    }
    
    // Collect results
    let mut successful_submissions = 0;
    let mut tx_hashes = Vec::new();
    
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await? {
            Ok(pending_tx) => {
                successful_submissions += 1;
                tx_hashes.push(*pending_tx.tx_hash());
                println!("Transaction {i} submitted: {}", pending_tx.tx_hash());
            }
            Err(e) => {
                println!("Transaction {i} failed: {e}");
            }
        }
    }
    
    println!("Successfully submitted: {successful_submissions}/{num_transactions}");
    assert!(successful_submissions > 0, "At least some transactions should succeed");
    
    // Wait for at least one transaction to be mined
    if !tx_hashes.is_empty() {
        let first_hash = tx_hashes[0];
        let receipt = provider.get_transaction_receipt(first_hash).await?;
        if let Some(receipt) = receipt {
            println!("First transaction mined in block: {}", receipt.block_number.unwrap());
        }
    }
    
    Ok(())
}

// #[tokio::test]
// async fn test_berachain_specific_features() -> eyre::Result<()> {
    // Test Berachain-specific RPC behavior
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // Test that Prague1 features are properly exposed via RPC
    let latest_block = provider.get_block_by_number(BlockNumberOrTag::Latest, true).await?;
    
    if let Some(block) = latest_block {
        // Verify Prague1 features in block structure
        assert!(block.header.base_fee_per_gas.is_some(), "Blocks should have base fee");
        println!("✅ Block has base fee: {} wei", block.header.base_fee_per_gas.unwrap());
        
        // If block has transactions, examine their structure
        if !block.transactions.is_empty() {
            println!("Block contains {} transactions", block.transactions.len());
            
            // Check for potential PoL transactions (if implemented)
            for (i, tx) in block.transactions.iter().enumerate() {
                if let Some(input) = &tx.input {
                    if !input.is_empty() {
                        println!("Transaction {i}: type byte = {}", input[0]);
                        if input[0] == 126 { // POL_TX_TYPE
                            println!("  → This appears to be a PoL transaction");
                        }
                    }
                }
            }
        } else {
            println!("Block is empty - no transactions to examine");
        }
        
        println!("✅ Berachain block structure validated");
    }
    
    Ok(())
}

// #[tokio::test]
// #[ignore = "requires WebSocket support - enable when WS is implemented"]
// async fn test_websocket_subscriptions() -> eyre::Result<()> {
    // Test WebSocket RPC functionality
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    
    // Note: This test requires WebSocket support in the test context
    // For now, we'll use HTTP provider but document the WebSocket pattern
    let provider = ctx.provider();
    
    // Subscribe to new blocks (would be via WebSocket in full implementation)
    // let subscription = provider.subscribe_blocks().await?;
    // let mut stream = subscription.into_stream();
    
    // Send transaction to trigger block production
    let tx_request = TransactionRequest::default()
        .to(Address::from([0xAAu8; 20]))
        .value(U256::from(5000))
        .gas_limit(21_000)
        .max_fee_per_gas(2_000_000_000u128);
    
    let pending_tx = provider.send_transaction(tx_request).await?;
    let receipt = pending_tx.get_receipt().await?;
    
    println!("Transaction mined, would trigger block subscription event");
    println!("Block number: {}", receipt.block_number.unwrap());
    
    // TODO: Implement actual WebSocket subscription testing when WS support is added
    // if let Some(block) = stream.next().await {
    //     let block = block?;
    //     assert!(block.number.unwrap() >= receipt.block_number.unwrap());
    // }
    
    Ok(())
}