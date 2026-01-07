//! E2E tests for flashblock integration.
//!
//! These tests verify the complete flow: flashblock stream → FlashBlockService →
//! pending block construction → RPC receipt availability.

use crate::e2e::{setup_test_boilerplate, test_signer};
use alloy_consensus::BlockHeader;
use alloy_eips::{BlockId, eip2718::Encodable2718};
use alloy_primitives::{Address, B256, Bloom, Bytes, TxKind, U256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::TransactionRequest;
use bera_reth::{
    engine::validator::BerachainEngineValidatorBuilder,
    flashblocks::{
        BerachainFlashblockPayload, BerachainFlashblockPayloadBase, BerachainFlashblockPayloadDiff,
        BerachainFlashblockPayloadMetadata,
    },
    node::BerachainNode,
    primitives::{BerachainPrimitives, header::BlsPublicKey},
    rpc::{BerachainAddOns, BerachainEthApiBuilder},
};
use reth::{providers::BlockReaderIdExt, rpc::types::engine::PayloadId};
use reth_chainspec::EthChainSpec;
use reth_e2e_test_utils::transaction::TransactionTestContext;
use reth_node_builder::{Node, NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_optimism_flashblocks::{
    FlashBlockCompleteSequence, FlashBlockService, FlashblocksListeners,
};
use std::{pin::Pin, sync::Arc, task::Poll, time::Duration};
use tokio::sync::{broadcast, watch};

fn create_test_flashblock(
    index: u64,
    block_number: u64,
    payload_id: PayloadId,
    parent_hash: B256,
    timestamp: u64,
) -> BerachainFlashblockPayload {
    let base = if index == 0 {
        Some(BerachainFlashblockPayloadBase {
            parent_beacon_block_root: B256::random(),
            parent_hash,
            fee_recipient: Address::random(),
            prev_randao: B256::random(),
            block_number,
            gas_limit: 30_000_000,
            timestamp,
            extra_data: Bytes::default(),
            base_fee_per_gas: U256::from(1_000_000_000u64),
            prev_proposer_pubkey: Some(BlsPublicKey::random()),
        })
    } else {
        None
    };

    BerachainFlashblockPayload {
        payload_id,
        index,
        base,
        diff: BerachainFlashblockPayloadDiff {
            state_root: B256::random(),
            receipts_root: B256::random(),
            logs_bloom: Bloom::default(),
            gas_used: 21000,
            block_hash: B256::random(),
            transactions: vec![],
            withdrawals: vec![],
            withdrawals_root: B256::ZERO,
            blob_gas_used: None,
        },
        metadata: BerachainFlashblockPayloadMetadata { block_number },
    }
}

struct MockFlashblockStream {
    rx: tokio::sync::mpsc::Receiver<BerachainFlashblockPayload>,
}

impl futures_util::Stream for MockFlashblockStream {
    type Item = eyre::Result<BerachainFlashblockPayload>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.rx).poll_recv(cx) {
            Poll::Ready(Some(fb)) => Poll::Ready(Some(Ok(fb))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Tests the complete flashblock → pending block → RPC receipt flow.
///
/// # Data Flow
///
/// This test wires together two separate systems that need to communicate:
///
/// ```text
///                    TEST HARNESS                          NODE (RPC)
///                    ────────────                          ──────────
///
///   fb_tx ──────► MockFlashblockStream
///                        │
///                        ▼
///                 FlashBlockService
///                   │         │
///                   │         ▼
///                   │    service.subscribe_in_progress()
///                   │              │
///                   ▼              │ (test manually bridges)
///              pending_tx ─────────┼──────────────────► pending_rx ──► RPC EthApi
///                                  │                                   (reads pending block)
///                                  ▼
///                           in_progress_tx ─────────► in_progress_rx ──► RPC EthApi
///                                                                        (knows build is active)
/// ```
///
/// The key insight is that `FlashblocksListeners` holds the rx ends of channels, which are
/// passed into the node at build time. The test harness holds the tx ends and must:
/// 1. Pass `pending_tx` to `service.run()` so the service can publish pending blocks
/// 2. Manually forward `in_progress` state from service → node (the service has its own internal
///    channel, so we subscribe and forward to the node's channel)
///
/// When an RPC call like `eth_getTransactionReceipt` comes in, the EthApi checks
/// `pending_rx` for a flashblock-derived pending block containing that transaction.
#[tokio::test]
async fn test_rpc_returns_flashblock_pending_receipt() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_boilerplate().await?;
    let executor = tasks.executor();

    // Channels that connect FlashBlockService (producer) to RPC EthApi (consumer).
    // The service writes to pending_tx, the RPC reads from pending_rx.
    let (pending_tx, pending_rx) = watch::channel(None);

    // in_progress channel tells RPC layer that a flashblock build is active.
    // We manually bridge from service's internal channel to this one (see below).
    let (in_progress_tx, in_progress_rx) = watch::channel(None);

    // These broadcast channels are for external listeners (e.g., websocket subscribers).
    // Not used in this test but required by FlashblocksListeners constructor.
    let (unused_sequence_tx, _) =
        broadcast::channel::<FlashBlockCompleteSequence<BerachainFlashblockPayload>>(1);
    let (unused_received_tx, _) = broadcast::channel::<Arc<BerachainFlashblockPayload>>(1);

    // FlashblocksListeners bundles the rx ends for the node's RPC layer.
    let listeners: FlashblocksListeners<BerachainPrimitives, BerachainFlashblockPayload> =
        FlashblocksListeners::new(
            pending_rx,
            unused_sequence_tx,
            in_progress_rx,
            unused_received_tx,
        );

    let eth_api_builder = BerachainEthApiBuilder::default().with_flashblocks_listeners(listeners);
    let add_ons = BerachainAddOns::<_, _, BerachainEngineValidatorBuilder>::new(eth_api_builder);

    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(executor.clone())
        .with_types::<BerachainNode>()
        .with_components(BerachainNode::default().components_builder())
        .with_add_ons(add_ons)
        .launch()
        .await?;

    // Create the flashblock input channel. Test sends flashblocks via fb_tx.
    let (fb_tx, fb_rx) = tokio::sync::mpsc::channel::<BerachainFlashblockPayload>(128);
    let stream = MockFlashblockStream { rx: fb_rx };

    let service = FlashBlockService::new(
        stream,
        node.evm_config.clone(),
        node.provider().clone(),
        executor.clone(),
        false,
    );

    // Subscribe to service's internal in_progress channel so we can forward to node's channel.
    let mut service_in_progress_rx = service.subscribe_in_progress();

    // Start the service, passing pending_tx so it can publish pending blocks to the RPC layer.
    executor.spawn_critical(
        "flashblock-service",
        Box::pin(async move {
            service.run(pending_tx).await;
        }),
    );

    let latest = node.provider().latest_header()?.expect("should have genesis");
    let latest_hash = latest.hash();
    let next_block = latest.number() + 1;
    let next_timestamp = latest.timestamp() + 2;

    let signer = test_signer()?;
    let chain_id = chain_spec.chain_id();
    let tx = TransactionTestContext::transfer_tx(chain_id, signer).await;
    let tx_bytes = Bytes::from(tx.encoded_2718());
    let tx_hash = *tx.tx_hash();

    let payload_id = PayloadId::new([1u8; 8]);
    let mut fb0 = create_test_flashblock(0, next_block, payload_id, latest_hash, next_timestamp);
    fb0.diff.transactions = vec![tx_bytes];
    fb0.diff.gas_used = 21000;

    // Inject the flashblock into the service via our mock stream.
    fb_tx.send(fb0).await?;

    // Wait for service to signal it's building, then forward that state to the node's channel.
    tokio::time::timeout(Duration::from_millis(500), service_in_progress_rx.changed()).await??;
    in_progress_tx.send(*service_in_progress_rx.borrow())?;

    // Give the service time to build and publish the pending block.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Query the RPC - should find the transaction in the flashblock-derived pending block.
    let rpc_url =
        format!("http://127.0.0.1:{}", node.rpc_server_handle().http_local_addr().unwrap().port());
    let rpc_client = alloy_provider::ProviderBuilder::new().connect(&rpc_url).await?;

    let receipt = rpc_client.get_transaction_receipt(tx_hash).await?;

    assert!(
        receipt.is_some(),
        "Transaction receipt should be available from flashblock pending state"
    );
    assert_eq!(receipt.unwrap().transaction_hash, tx_hash);

    Ok(())
}

/// Tests that state-reading RPC methods return flashblock pending state.
///
/// This test verifies that when a flashblock contains a transfer transaction:
/// - `eth_getBalance` returns the recipient's updated balance
/// - `eth_getTransactionCount` returns the sender's incremented nonce
#[tokio::test]
async fn test_rpc_returns_flashblock_pending_state() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_boilerplate().await?;
    let executor = tasks.executor();

    let (pending_tx, pending_rx) = watch::channel(None);
    let (in_progress_tx, in_progress_rx) = watch::channel(None);
    let (unused_sequence_tx, _) =
        broadcast::channel::<FlashBlockCompleteSequence<BerachainFlashblockPayload>>(1);
    let (unused_received_tx, _) = broadcast::channel::<Arc<BerachainFlashblockPayload>>(1);

    let listeners: FlashblocksListeners<BerachainPrimitives, BerachainFlashblockPayload> =
        FlashblocksListeners::new(
            pending_rx,
            unused_sequence_tx,
            in_progress_rx,
            unused_received_tx,
        );

    let eth_api_builder = BerachainEthApiBuilder::default().with_flashblocks_listeners(listeners);
    let add_ons = BerachainAddOns::<_, _, BerachainEngineValidatorBuilder>::new(eth_api_builder);

    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(executor.clone())
        .with_types::<BerachainNode>()
        .with_components(BerachainNode::default().components_builder())
        .with_add_ons(add_ons)
        .launch()
        .await?;

    let (fb_tx, fb_rx) = tokio::sync::mpsc::channel::<BerachainFlashblockPayload>(128);
    let stream = MockFlashblockStream { rx: fb_rx };

    let service = FlashBlockService::new(
        stream,
        node.evm_config.clone(),
        node.provider().clone(),
        executor.clone(),
        false,
    );

    let mut service_in_progress_rx = service.subscribe_in_progress();

    executor.spawn_critical(
        "flashblock-service",
        Box::pin(async move {
            service.run(pending_tx).await;
        }),
    );

    let latest = node.provider().latest_header()?.expect("should have genesis");
    let latest_hash = latest.hash();
    let next_block = latest.number() + 1;
    let next_timestamp = latest.timestamp() + 2;

    // Create a transfer transaction to a fresh recipient address
    let recipient = Address::random();
    let transfer_value = U256::from(100);

    let signer = test_signer()?;
    let sender = signer.address();
    let chain_id = chain_spec.chain_id();

    // Build a transfer transaction with a specific recipient
    let tx_request = TransactionRequest {
        nonce: Some(0),
        value: Some(transfer_value),
        to: Some(TxKind::Call(recipient)),
        gas: Some(21000),
        max_fee_per_gas: Some(20e9 as u128),
        max_priority_fee_per_gas: Some(20e9 as u128),
        chain_id: Some(chain_id),
        ..Default::default()
    };
    let tx = TransactionTestContext::sign_tx(signer, tx_request).await;
    let tx_bytes = Bytes::from(tx.encoded_2718());

    let payload_id = PayloadId::new([2u8; 8]);
    let mut fb0 = create_test_flashblock(0, next_block, payload_id, latest_hash, next_timestamp);
    fb0.diff.transactions = vec![tx_bytes];
    fb0.diff.gas_used = 21000;

    // Inject the flashblock
    fb_tx.send(fb0).await?;

    // Wait for service to signal it's building
    tokio::time::timeout(Duration::from_millis(500), service_in_progress_rx.changed()).await??;
    in_progress_tx.send(*service_in_progress_rx.borrow())?;

    // Give the service time to build and publish the pending block
    tokio::time::sleep(Duration::from_millis(100)).await;

    let rpc_url =
        format!("http://127.0.0.1:{}", node.rpc_server_handle().http_local_addr().unwrap().port());
    let client = alloy_provider::ProviderBuilder::new().connect(&rpc_url).await?;

    // Check recipient balance with "pending" block tag - should reflect the pending transfer
    let balance = client.get_balance(recipient).block_id(BlockId::pending()).await?;
    assert_eq!(balance, transfer_value);

    // Verify that the nonce is 1 more in flashblock state
    let nonce_latest = client.get_transaction_count(sender).block_id(BlockId::latest()).await?;
    let nonce_pending = client.get_transaction_count(sender).block_id(BlockId::pending()).await?;
    assert_eq!(nonce_pending, nonce_latest + 1);

    Ok(())
}

/// Tests that flashblocks with invalid parent hashes are rejected.
#[tokio::test]
async fn test_flashblock_rejects_invalid_parent_hash() -> eyre::Result<()> {
    let (tasks, chain_spec) = setup_test_boilerplate().await?;
    let executor = tasks.executor();

    let node_config = NodeConfig::new(chain_spec.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(executor.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let (fb_tx, fb_rx) = tokio::sync::mpsc::channel::<BerachainFlashblockPayload>(128);
    let stream = MockFlashblockStream { rx: fb_rx };

    let service = FlashBlockService::new(
        stream,
        node.evm_config.clone(),
        node.provider().clone(),
        executor.clone(),
        false,
    );

    let (pending_tx, mut pending_rx) = watch::channel(None);
    executor.spawn_critical(
        "flashblock-service",
        Box::pin(async move {
            service.run(pending_tx).await;
        }),
    );

    let latest = node.provider().latest_header()?.expect("should have genesis");
    let next_block = latest.number() + 1;
    let next_timestamp = latest.timestamp() + 2;

    let wrong_parent_hash = B256::random();
    let payload_id = PayloadId::new([1u8; 8]);
    let fb = create_test_flashblock(0, next_block, payload_id, wrong_parent_hash, next_timestamp);

    fb_tx.send(fb).await?;

    let result = tokio::time::timeout(Duration::from_millis(200), pending_rx.changed()).await;
    assert!(result.is_err(), "Should not build pending block with wrong parent hash");

    Ok(())
}
