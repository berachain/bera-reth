//! RPC transaction integration tests

use crate::e2e::{berachain_payload_attributes, setup_test_boilerplate, test_signer};
use bera_reth::node::BerachainNode;
use reth::transaction_pool::TransactionPool;
use reth_chainspec::EthChainSpec;
use reth_e2e_test_utils::{node::NodeTestContext, transaction::TransactionTestContext};
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};

#[tokio::test]
async fn test_eip1559_transaction_via_rpc_is_accepted() -> eyre::Result<()> {
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

    let ctx = NodeTestContext::new(node, berachain_payload_attributes).await?;
    let signer = test_signer()?;
    let chain_id = chain_spec.chain_id();

    let tx_bytes = TransactionTestContext::transfer_tx_bytes(chain_id, signer).await;
    let res = ctx.rpc.inject_tx(tx_bytes).await;

    assert!(res.is_ok(), "EIP1559 transaction should be accepted via RPC");
    println!("EIP1559 transaction accepted with hash: {:?}", res.unwrap());

    Ok(())
}

#[tokio::test]
async fn test_pol_transaction_via_rpc_is_rejected() -> eyre::Result<()> {
    use alloy_eips::eip2718::Encodable2718;
    use alloy_primitives::{Address, Bytes};
    use bera_reth::transaction::PoLTx;
    use reth_rpc_eth_types::EthApiError;

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

    let ctx = NodeTestContext::new(node, berachain_payload_attributes).await?;
    let chain_id = chain_spec.chain_id();

    // Create a manually crafted PoL transaction (type 0x7E/126)
    // Expected behavior: PoL transactions should ONLY be generated automatically by the
    // consensus layer during block building, never submitted manually via RPC
    let fake_pol_tx = PoLTx {
        chain_id,
        from: Default::default(),
        to: Address::random(),
        nonce: 0,
        gas_limit: 30_000_000,
        gas_price: 1_000_000_000u128, // 1 gwei
        input: Bytes::from(b"fake_pol_data"),
    };

    let mut buf = Vec::with_capacity(fake_pol_tx.encode_2718_len());
    fake_pol_tx.encode_2718(&mut buf);

    // Attempt to inject the PoL transaction via RPC - this should be rejected
    let res = ctx.rpc.inject_tx(buf.into()).await;

    // Expected behavior: RPC should reject manually submitted PoL transactions
    // 1. Type inference causes recover_raw_transaction to be called with EthereumTxEnvelope
    // 2. EthereumTxEnvelope doesn't recognize PoL transaction type (0x7E/126)
    // 3. decode_2718 fails with RlpError(Custom("unexpected tx type"))
    // 4. This gets mapped to FailedToDecodeSignedTransaction for RPC response
    assert!(res.is_err(), "PoL transaction should be rejected via RPC");

    let error = res.unwrap_err();
    assert!(
        matches!(error, EthApiError::FailedToDecodeSignedTransaction),
        "Expected FailedToDecodeSignedTransaction, got: {:?}",
        error
    );

    // Test 2: Attempt to add PoL transaction directly to mempool as consensus transaction
    // This tests whether the mempool accepts properly formed PoL transactions from consensus layer
    use alloy_primitives::Sealed;
    use bera_reth::transaction::BerachainTxEnvelope;
    use reth_primitives_traits::SignedTransaction;
    use reth_transaction_pool::TransactionOrigin;

    // Create a properly sealed PoL transaction envelope
    let pol_tx_sealed = Sealed::new(fake_pol_tx);
    let pol_tx_envelope = BerachainTxEnvelope::Berachain(pol_tx_sealed);

    // Convert to recovered transaction (consensus transactions are pre-validated)
    let recovered_pol_tx = pol_tx_envelope
        .try_into_recovered()
        .expect("PoL transaction should be recoverable as consensus transaction");

    // Test: Add as consensus transaction - this reveals mempool validation behavior
    // Expected: Even consensus transactions must be convertible to mempool format
    let pool_result = ctx
        .rpc
        .inner
        .pool()
        .add_consensus_transaction(recovered_pol_tx, TransactionOrigin::External)
        .await;

    // Expected behavior: PoL transactions cannot be converted to Ethereum mempool format
    // The mempool requires all transactions to be in a format compatible with Ethereum
    // transaction pool, but PoL transactions are Berachain-specific and not convertible
    assert!(pool_result.is_err(), "PoL transaction should be rejected by mempool");

    let pool_error = pool_result.unwrap_err();
    let error_msg = format!("{:?}", pool_error);
    assert!(
        error_msg.contains("Cannot convert Berachain POL transaction to Ethereum format"),
        "Expected conversion error, got: {}",
        error_msg
    );

    println!("✅ Mempool correctly rejects PoL transactions: {}", error_msg);

    Ok(())
}
