# End-to-End Testing Guide for Bera-Reth

This guide demonstrates how to adapt Reth's comprehensive e2e testing patterns for Bera-Reth, enabling robust integration testing of RPC endpoints, transaction handling, and Berachain-specific functionality.

## Table of Contents

- [Overview](#overview)
- [Testing Architecture](#testing-architecture)
- [Setting Up E2E Tests](#setting-up-e2e-tests)
- [RPC Integration Testing](#rpc-integration-testing)
- [Transaction Testing Patterns](#transaction-testing-patterns)
- [Berachain-Specific Testing](#berachain-specific-testing)
- [Advanced Testing Scenarios](#advanced-testing-scenarios)
- [Best Practices](#best-practices)
- [Examples](#examples)

## Overview

Reth's e2e testing framework provides a comprehensive approach to integration testing that:

- **Launches real nodes** with full blockchain state
- **Tests actual RPC endpoints** with HTTP/WebSocket clients
- **Validates transaction flow** from submission to inclusion
- **Supports custom chain specifications** for different networks
- **Provides utilities** for common testing scenarios

Bera-Reth can leverage this infrastructure while adding Berachain-specific testing capabilities.

## Testing Architecture

### Core Components

```rust
// Key dependencies for bera-reth e2e tests
[dev-dependencies]
reth-e2e-test-utils = { workspace = true }
reth-rpc-builder = { workspace = true }
reth-testing-utils = { workspace = true }
alloy-rpc-client = { workspace = true }
alloy-provider = { workspace = true }
tokio = { version = "1.0", features = ["full"] }
```

### Test Infrastructure Layers

1. **Node Setup Layer**: Configures Berachain node with custom specs
2. **RPC Layer**: HTTP/WS client for actual endpoint testing  
3. **Transaction Layer**: PoL and Ethereum transaction utilities
4. **Validation Layer**: Chain state and consensus verification

## Setting Up E2E Tests

### Basic Test Structure

Create integration tests in `tests/` directory following this pattern:

```rust
// tests/integration/mod.rs
use reth_e2e_test_utils::{
    setup::{Setup, NetworkSetup},
    TestBuilder, NodeTestContext
};
use bera_reth::{
    BerachainNode,
    chainspec::BerachainChainSpec,
    transaction::{BerachainTxEnvelope, PoLTx, POL_TX_TYPE}
};

/// Berachain-specific test setup
fn berachain_test_setup() -> Setup {
    Setup::default()
        .with_chain_spec(berachain_testnet_spec())
        .with_network(NetworkSetup::single_node())
        .with_genesis_block_interval(Duration::from_secs(1))
}

/// Create Berachain testnet specification
fn berachain_testnet_spec() -> Arc<BerachainChainSpec> {
    // Configure with Prague1 hardfork at genesis
    BerachainChainSpec::builder()
        .chain(Chain::berachain_testnet())
        .genesis_block(create_berachain_genesis())
        .prague_fork_activated_at_genesis() // Key difference from Ethereum
        .build()
}
```

### Node Context Setup

```rust
#[tokio::test]
async fn test_berachain_node_startup() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    
    // Verify node is running with Berachain configuration
    let client_version = ctx.rpc_client().client_version().await?;
    assert!(client_version.contains("bera-reth"));
    
    // Verify Prague1 hardfork is active at genesis
    let genesis_block = ctx.rpc_client().get_block_by_number(0.into(), false).await?;
    assert!(genesis_block.unwrap().base_fee_per_gas.is_some()); // Prague1 feature
    
    Ok(())
}
```

## RPC Integration Testing

### HTTP RPC Testing

```rust
use reth_rpc_builder::{RethRpcModule, RpcServerHandle};
use alloy_rpc_client::ClientBuilder;
use alloy_provider::{Provider, ProviderBuilder};

async fn launch_berachain_http_rpc() -> eyre::Result<RpcServerHandle> {
    let setup = berachain_test_setup();
    let modules = vec![
        RethRpcModule::Eth,
        RethRpcModule::Net, 
        RethRpcModule::Web3,
        // Add Berachain-specific modules if any
    ];
    
    let handle = reth_e2e_test_utils::launch_http_rpc(
        setup,
        modules,
        BerachainNode::default()
    ).await?;
    
    Ok(handle)
}

#[tokio::test]
async fn test_rpc_eth_methods() -> eyre::Result<()> {
    let handle = launch_berachain_http_rpc().await?;
    let provider = ProviderBuilder::new()
        .on_http(handle.http_url())
        .await?;
    
    // Test basic RPC functionality
    let chain_id = provider.get_chain_id().await?;
    assert_eq!(chain_id, 80084); // Berachain testnet ID
    
    let block_number = provider.get_block_number().await?;
    assert!(block_number >= 0);
    
    // Test gas price includes base fee (Prague1 feature)
    let gas_price = provider.get_gas_price().await?;
    assert!(gas_price >= 1_000_000_000); // Minimum 1 gwei base fee
    
    Ok(())
}
```

### WebSocket RPC Testing

```rust
#[tokio::test]
async fn test_websocket_subscriptions() -> eyre::Result<()> {
    let handle = launch_berachain_ws_rpc().await?;
    let provider = ProviderBuilder::new()
        .on_ws(handle.ws_url())
        .await?;
    
    // Subscribe to new blocks
    let subscription = provider.subscribe_blocks().await?;
    let mut stream = subscription.into_stream();
    
    // Trigger block production
    let _tx_hash = send_test_transaction(&provider).await?;
    
    // Verify we receive the new block
    if let Some(block) = stream.next().await {
        let block = block?;
        assert!(block.transactions.len() > 0);
        
        // Verify PoL transaction is first transaction (Berachain-specific)
        if let Some(first_tx) = block.transactions.first() {
            // PoL transactions should be first in each block after Prague1
            // (actual verification would depend on block structure)
        }
    }
    
    Ok(())
}
```

## Transaction Testing Patterns

### PoL Transaction Testing

```rust
/// Test PoL transaction rejection at RPC level
#[tokio::test]
async fn test_pol_transaction_rejection() -> eyre::Result<()> {
    let handle = launch_berachain_http_rpc().await?;
    let provider = ProviderBuilder::new()
        .on_http(handle.http_url())
        .await?;
    
    // Create a PoL transaction that should be rejected
    let pol_tx = create_test_pol_transaction();
    let raw_tx = encode_pol_transaction(pol_tx);
    
    // Attempt to submit via RPC - should be rejected
    let result = provider.send_raw_transaction(&raw_tx).await;
    
    assert!(result.is_err());
    let error = result.unwrap_err();
    
    // Verify specific error for PoL rejection
    assert!(error.to_string().contains("PoL transactions cannot be submitted via RPC"));
    
    Ok(())
}

fn create_test_pol_transaction() -> PoLTx {
    PoLTx {
        chain_id: ChainId::from(80084u64),
        from: Address::ZERO, // System address for PoL
        to: pol_distributor_address(),
        nonce: 42,
        gas_limit: 0, // PoL transactions have zero gas limit
        gas_price: 1_000_000_000, // Base fee
        input: create_pol_distribute_call_data(),
    }
}

fn encode_pol_transaction(pol_tx: PoLTx) -> Bytes {
    let sealed = Sealed::new(pol_tx);
    let envelope = BerachainTxEnvelope::Berachain(sealed);
    envelope.encoded_2718().into()
}
```

### Ethereum Transaction Testing

```rust
#[tokio::test]
async fn test_ethereum_transaction_acceptance() -> eyre::Result<()> {
    let handle = launch_berachain_http_rpc().await?;
    let provider = ProviderBuilder::new()
        .on_http(handle.http_url())
        .await?;
    
    // Create standard Ethereum transaction
    let tx_request = TransactionRequest::default()
        .to(Address::random())
        .value(U256::from(1000))
        .gas_limit(21000)
        .max_fee_per_gas(2_000_000_000u128) // 2 gwei
        .max_priority_fee_per_gas(1_000_000_000u128) // 1 gwei
        .chain_id(80084);
    
    // Submit transaction - should be accepted
    let pending_tx = provider.send_transaction(tx_request).await?;
    let tx_hash = *pending_tx.tx_hash();
    
    // Wait for inclusion
    let receipt = pending_tx.get_receipt().await?;
    assert!(receipt.status());
    assert_eq!(receipt.transaction_hash, tx_hash);
    
    Ok(())
}
```

### Transaction Pool Testing

```rust
#[tokio::test]
async fn test_transaction_pool_behavior() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    
    // Test pool capacity and replacement logic
    let provider = ctx.provider();
    let pool = ctx.node().pool();
    
    // Fill pool with transactions
    let mut tx_hashes = Vec::new();
    for nonce in 0..10 {
        let tx = create_test_ethereum_tx(nonce);
        let hash = provider.send_raw_transaction(&tx).await?;
        tx_hashes.push(hash);
    }
    
    // Verify pool state
    let pool_status = pool.pool_size();
    assert_eq!(pool_status.pending, 10);
    
    // Test transaction replacement with higher gas
    let replacement_tx = create_higher_gas_tx(0); // Same nonce, higher gas
    let new_hash = provider.send_raw_transaction(&replacement_tx).await?;
    
    // Verify replacement occurred
    assert_ne!(new_hash, tx_hashes[0]);
    
    Ok(())
}
```

## Berachain-Specific Testing

### Genesis Configuration Testing

```rust
#[tokio::test]
async fn test_berachain_genesis_configuration() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // Verify Prague1 hardfork is active at genesis (time: 0)
    let genesis_block = provider.get_block_by_number(0.into(), true).await?.unwrap();
    
    // Prague1 features should be active
    assert!(genesis_block.base_fee_per_gas.is_some());
    assert_eq!(genesis_block.base_fee_per_gas.unwrap(), 1_000_000_000); // 1 gwei minimum
    
    // Verify PoL transaction is present in genesis+1 block
    let block_1 = provider.get_block_by_number(1.into(), true).await?.unwrap();
    if let Some(first_tx) = block_1.transactions.first() {
        // Verify first transaction is PoL type
        if let Some(tx_bytes) = &first_tx.input {
            assert_eq!(tx_bytes[0], POL_TX_TYPE); // Type 126 (0x7E)
        }
    }
    
    Ok(())
}
```

### Block Production Testing

```rust
#[tokio::test]
async fn test_block_progression_with_pol() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    let initial_block = provider.get_block_number().await?;
    
    // Send a regular transaction to trigger block production
    let tx_request = create_test_ethereum_tx_request();
    let pending_tx = provider.send_transaction(tx_request).await?;
    
    // Wait for block to be mined
    let receipt = pending_tx.get_receipt().await?;
    let block_number = receipt.block_number.unwrap();
    
    assert!(block_number > initial_block);
    
    // Verify the block contains both PoL and user transaction
    let block = provider.get_block_by_number(block_number.into(), true).await?.unwrap();
    
    // Should have at least 2 transactions: PoL + user transaction
    assert!(block.transactions.len() >= 2);
    
    // First transaction should be PoL
    let first_tx_bytes = &block.transactions[0].input;
    assert_eq!(first_tx_bytes[0], POL_TX_TYPE);
    
    // User transaction should be present
    assert_eq!(block.transactions[1].hash, receipt.transaction_hash);
    
    Ok(())
}
```

### Chain Specification Testing

```rust
#[tokio::test]
async fn test_berachain_chain_spec_compatibility() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    
    // Test EIP compatibility
    let chain_spec = ctx.node().chain_spec();
    
    // Verify Berachain-specific configuration
    assert_eq!(chain_spec.chain, Chain::berachain_testnet());
    assert!(chain_spec.is_prague_active_at_timestamp(0)); // Active at genesis
    
    // Verify Ethereum compatibility features are maintained
    assert!(chain_spec.is_london_active_at_block(0));
    assert!(chain_spec.is_cancun_active_at_timestamp(0));
    
    Ok(())
}
```

## Advanced Testing Scenarios

### Multi-Node Network Testing

```rust
#[tokio::test]
async fn test_multi_node_pol_consensus() -> eyre::Result<()> {
    let setup = Setup::default()
        .with_chain_spec(berachain_testnet_spec())
        .with_network(NetworkSetup::peer_to_peer(3)); // 3 nodes
    
    let network = TestNetwork::<BerachainNode>::new(setup).await?;
    
    // Send transaction to node 1
    let provider_1 = network.node(0).provider();
    let tx_hash = send_test_transaction(&provider_1).await?;
    
    // Verify transaction propagates to all nodes
    for i in 0..3 {
        let provider = network.node(i).provider();
        let receipt = provider.get_transaction_receipt(tx_hash).await?;
        assert!(receipt.is_some());
        
        // Verify all nodes see same PoL transaction in block
        let block_number = receipt.unwrap().block_number.unwrap();
        let block = provider.get_block_by_number(block_number.into(), true).await?.unwrap();
        assert_eq!(block.transactions[0].input[0], POL_TX_TYPE);
    }
    
    Ok(())
}
```

### Load Testing

```rust
#[tokio::test]
async fn test_high_transaction_throughput() -> eyre::Result<()> {
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // Send many transactions concurrently
    let mut handles = Vec::new();
    for nonce in 0..100 {
        let provider = provider.clone();
        let handle = tokio::spawn(async move {
            let tx = create_test_ethereum_tx_request_with_nonce(nonce);
            provider.send_transaction(tx).await
        });
        handles.push(handle);
    }
    
    // Wait for all transactions
    let mut successful_txs = 0;
    for handle in handles {
        if handle.await?.is_ok() {
            successful_txs += 1;
        }
    }
    
    // Verify high success rate
    assert!(successful_txs > 95); // >95% success rate
    
    // Verify blocks contain PoL transactions
    let latest_block = provider.get_block_number().await?;
    for block_num in 1..=latest_block {
        let block = provider.get_block_by_number(block_num.into(), true).await?.unwrap();
        if !block.transactions.is_empty() {
            assert_eq!(block.transactions[0].input[0], POL_TX_TYPE);
        }
    }
    
    Ok(())
}
```

## Best Practices

### 1. Test Organization

```rust
// Organize tests by functionality
mod rpc_tests {
    mod send_raw_transaction;
    mod transaction_pool;
    mod block_subscriptions;
}

mod berachain_tests {
    mod pol_transactions;
    mod genesis_config;
    mod hardfork_activation;
}

mod integration_tests {
    mod multi_node;
    mod load_testing;
    mod consensus;
}
```

### 2. Test Utilities

```rust
// Create reusable test utilities
pub mod test_utils {
    pub fn berachain_test_setup() -> Setup { /* ... */ }
    pub fn create_pol_transaction() -> PoLTx { /* ... */ }
    pub fn create_ethereum_tx() -> TransactionRequest { /* ... */ }
    pub async fn wait_for_block_production(provider: &Provider) -> eyre::Result<u64> { /* ... */ }
}
```

### 3. Environment Configuration

```rust
// Support different test environments
fn test_setup_from_env() -> Setup {
    match std::env::var("BERA_TEST_ENV").as_deref() {
        Ok("mainnet") => berachain_mainnet_test_setup(),
        Ok("testnet") => berachain_testnet_setup(),
        _ => berachain_local_dev_setup(),
    }
}
```

### 4. Resource Management

```rust
// Proper cleanup in tests
#[tokio::test]
async fn test_with_cleanup() -> eyre::Result<()> {
    let _guard = TestGuard::new(); // Cleanup on drop
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    
    // Test logic here
    
    // Resources cleaned up automatically
    Ok(())
}
```

## Examples

### Complete RPC Integration Test

```rust
// tests/rpc_integration.rs
use bera_reth_test_utils::*;

#[tokio::test]
async fn test_complete_transaction_flow() -> eyre::Result<()> {
    // Setup Berachain node
    let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
    let provider = ctx.provider();
    
    // 1. Verify initial state
    let initial_balance = provider.get_balance(test_address(), None).await?;
    let initial_block = provider.get_block_number().await?;
    
    // 2. Send transaction
    let tx_request = TransactionRequest::default()
        .to(Address::random())
        .value(U256::from(1_000_000))
        .gas_limit(21_000)
        .max_fee_per_gas(2_000_000_000u128);
    
    let pending_tx = provider.send_transaction(tx_request).await?;
    let tx_hash = *pending_tx.tx_hash();
    
    // 3. Wait for inclusion
    let receipt = pending_tx.get_receipt().await?;
    assert!(receipt.status());
    
    // 4. Verify block contains PoL transaction first
    let block = provider.get_block_by_number(receipt.block_number.unwrap().into(), true).await?.unwrap();
    assert!(block.transactions.len() >= 2);
    assert_eq!(block.transactions[0].input[0], POL_TX_TYPE); // PoL first
    assert_eq!(block.transactions[1].hash, tx_hash); // User tx second
    
    // 5. Verify state changes
    let final_balance = provider.get_balance(test_address(), None).await?;
    assert!(final_balance < initial_balance); // Gas consumed
    
    // 6. Verify chain progressed
    let final_block = provider.get_block_number().await?;
    assert!(final_block > initial_block);
    
    Ok(())
}
```

This guide provides a comprehensive foundation for implementing robust e2e testing in Bera-Reth using Reth's proven patterns while accommodating Berachain-specific requirements like PoL transactions and Prague1 hardfork activation at genesis.