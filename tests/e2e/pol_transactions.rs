//! PoL (Proof of Liquidity) transaction integration tests
//!
//! Tests the complete lifecycle of PoL transactions, including:
//! - RPC behavior when PoL transactions are submitted
//! - Automatic PoL transaction inclusion in blocks
//! - PoL transaction validation and consensus

use alloy_consensus::Sealed;
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, B256, Bytes, ChainId};
use bera_reth::{
    chainspec::BerachainChainSpec,
    engine::payload::{BerachainPayloadAttributes, BerachainPayloadBuilderAttributes},
    node::BerachainNode,
    primitives::header::BlsPublicKey,
    transaction::{BerachainTxEnvelope, PoLTx},
};
use reth::{providers::BlockNumReader, tasks::TaskManager};
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_node_ethereum::engine::EthPayloadAttributes;
use reth_payload_primitives::{BuiltPayload, PayloadBuilderAttributes};
use std::sync::Arc;

/// Create a test PoL transaction that would normally be system-generated
fn create_test_pol_transaction() -> PoLTx {
    PoLTx {
        chain_id: ChainId::from(80084u64), // Berachain testnet
        from: Address::ZERO,               // System address
        to: Address::from([0x42u8; 20]),   // Mock PoL distributor
        nonce: 42,
        gas_limit: 0,             // PoL transactions have zero gas limit
        gas_price: 1_000_000_000, // 1 gwei base fee
        input: Bytes::from(vec![0x01, 0x02, 0x03]), // Mock distributeFor() call
    }
}

/// Encode PoL transaction as raw bytes for RPC submission
fn encode_pol_transaction_bytes(pol_tx: PoLTx) -> Bytes {
    let sealed = Sealed::new(pol_tx);
    let envelope = BerachainTxEnvelope::Berachain(sealed);
    envelope.encoded_2718().into()
}

/// Create Berachain payload attributes for testing
fn berachain_payload_attributes(timestamp: u64) -> BerachainPayloadBuilderAttributes {
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

#[tokio::test]
async fn test_block_production_with_transactions() -> eyre::Result<()> {
    // Test that blocks are produced and examine their structure

    // Create TaskManager and keep it alive for the entire test
    let tasks = TaskManager::current();
    let executor = tasks.executor();

    // Load genesis from the actual BeaconKit genesis file
    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path).expect("Failed to read genesis file");
    let genesis = parse_genesis(&genesis_json).expect("Failed to parse genesis");

    // Create BerachainChainSpec from the genesis using the From trait
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));

    // Create node configuration with Berachain chain spec
    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    // Launch the Berachain node with proper executor
    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(executor.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    // Create test context
    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes).await?;

    // Get initial block number from the context
    let initial_block = ctx.rpc.inner.eth_api().provider().best_block_number()?;
    println!("Initial block number: {initial_block}");

    // Advance the block to test block production
    let payload = ctx.advance_block().await?;
    let block_number = payload.block().number;

    println!("Transaction mined in block: {block_number}");
    assert!(block_number > initial_block, "Block should have advanced");

    // Examine the block structure from the payload
    let block = payload.block();
    println!("Block contains {} transactions", block.body().transactions.len());
    //
    // // Check if first transaction is PoL (if PoL auto-inclusion is implemented)
    // if !block.body() {
    // let first_tx = &block.body()[0];
    // // Check transaction type from the transaction envelope
    // if let Ok(envelope) = BerachainTxEnvelope::decode_2718(&mut first_tx.encode().as_slice()) {
    //     match envelope {
    //         BerachainTxEnvelope::Berachain(_) => {
    //             println!("✅ First transaction is PoL type (126) - auto-inclusion working");
    //         }
    //         _ => {
    //             println!("ℹ️  First transaction is not PoL type");
    //             println!("   This indicates PoL auto-inclusion may not be implemented yet");
    //         }
    //     }
    // }

    //     println!("✅ Block produced successfully with transactions");
    // }

    // Keep tasks alive until the end of the test
    // drop(tasks);
    Ok(())
}

// #[tokio::test]
// async fn test_pol_transaction_current_behavior() -> eyre::Result<()> {
//     // Test current behavior - establishes baseline for PoL transaction handling

//     let ctx =
//         NodeTestContext::<BerachainNode, BerachainAddOns>::new(berachain_test_setup()).await?;
//     let provider = ctx.provider();

//     // Create PoL transaction bytes
//     let pol_tx = create_test_pol_transaction();
//     let pol_bytes = encode_pol_transaction_bytes(pol_tx);

//     // Verify it's encoded as type 126 (PoL)
//     assert_eq!(pol_bytes[0], POL_TX_TYPE, "PoL transaction should have type 126");
//     println!("✅ PoL transaction encoded correctly as type {}", pol_bytes[0]);

//     // Test current RPC behavior when submitting PoL transaction
//     let result = provider.send_raw_transaction(&pol_bytes).await;

//     // Document current behavior
//     match result {
//         Ok(tx_hash) => {
//             println!("✅ PoL transaction was accepted with hash: {tx_hash}");
//             println!("   This indicates PoL rejection is not currently implemented");

//             // If accepted, wait to see if it gets mined
//             if let Ok(receipt) = tokio::time::timeout(
//                 Duration::from_secs(10),
//                 provider.get_transaction_receipt(tx_hash),
//             )
//             .await
//             {
//                 match receipt {
//                     Ok(Some(receipt)) => {
//                         println!(
//                             "   PoL transaction was mined in block {}",
//                             receipt.block_number.unwrap()
//                         );
//                     }
//                     Ok(None) => {
//                         println!("   PoL transaction is pending");
//                     }
//                     Err(e) => {
//                         println!("   Error getting receipt: {e}");
//                     }
//                 }
//             }
//         }
//         Err(e) => {
//             println!("❌ PoL transaction was rejected with error: {e}");

//             // Check if it's the expected PoL rejection error
//             let error_msg = e.to_string();
//             if error_msg.contains("PoL transactions cannot be submitted via RPC") {
//                 println!("   ✅ This is the expected PoL rejection behavior");
//             } else if error_msg.contains("failed to decode") || error_msg.contains("invalid") {
//                 println!("   ⚠️  This appears to be a decode/validation error");
//             } else {
//                 println!("   ⚠️  This is an unexpected error type");
//             }
//         }
//     }

//     Ok(())
// }

// #[tokio::test]
// async fn test_ethereum_transaction_acceptance() -> eyre::Result<()> {
//     // Verify that regular Ethereum transactions work properly
//     let ctx = NodeTestContext::<BerachainNode>::new(berachain_test_setup()).await?;
//     let provider = ctx.provider();

//     // Create a simple Ethereum transaction
//     let tx_request = alloy_rpc_types_eth::TransactionRequest::default()
//         .to(Address::from([0x11u8; 20]))
//         .value(1000u64.into())
//         .gas_limit(21000)
//         .max_fee_per_gas(2_000_000_000u128) // 2 gwei
//         .max_priority_fee_per_gas(1_000_000_000u128) // 1 gwei tip
//         .with_chain_id(80084);

//     // This should work regardless of PoL implementation
//     let pending_tx = provider.send_transaction(tx_request).await?;
//     let tx_hash = *pending_tx.tx_hash();

//     println!("✅ Ethereum transaction submitted successfully: {tx_hash}");

//     // Wait for inclusion (with timeout)
//     let receipt = tokio::time::timeout(Duration::from_secs(10),
// pending_tx.get_receipt()).await??;

//     assert!(receipt.status(), "Transaction should be successful");
//     println!(
//         "✅ Ethereum transaction mined successfully in block {}",
//         receipt.block_number.unwrap()
//     );

//     Ok(())
// }

// #[tokio::test]
// #[ignore = "requires multi-node setup - enable when network testing is needed"]
// async fn test_pol_consensus_across_nodes() -> eyre::Result<()> {
//     // Test PoL transaction consensus across multiple nodes
//     let network = TestNetwork::<BerachainNode>::new(berachain_multi_node_setup(3)).await?;

//     // Send transaction to first node
//     let provider_0 = network.node(0).provider();
//     let tx_request = alloy_rpc_types_eth::TransactionRequest::default()
//         .to(Address::from([0x33u8; 20]))
//         .value(3000u64.into())
//         .gas_limit(21000)
//         .max_fee_per_gas(2_000_000_000u128);

//     let pending_tx = provider_0.send_transaction(tx_request).await?;
//     let receipt = pending_tx.get_receipt().await?;
//     let block_number = receipt.block_number.unwrap();

//     // Verify all nodes see the same block
//     for i in 0..3 {
//         let provider = network.node(i).provider();
//         let block = provider
//             .get_block_by_number(block_number.into(), true)
//             .await?
//             .expect("All nodes should have the block");

//         println!("Node {i}: Block {block_number} has {} transactions", block.transactions.len());

//         // Verify transaction hash is consistent across nodes
//         let tx_in_block = block
//             .transactions
//             .iter()
//             .find(|tx| tx.hash == receipt.transaction_hash)
//             .expect("Transaction should be in block on all nodes");

//         assert_eq!(tx_in_block.hash, receipt.transaction_hash);

//         // Check for PoL transaction consistency across nodes
//         if !block.transactions.is_empty() {
//             let first_tx = &block.transactions[0];
//             if let Some(input) = &first_tx.input {
//                 if !input.is_empty() && input[0] == POL_TX_TYPE {
//                     println!("Node {i}: First transaction is PoL type - consensus maintained");
//                 }
//             }
//         }
//     }

//     println!("✅ Transaction consensus verified across all nodes");
//     Ok(())
// }
