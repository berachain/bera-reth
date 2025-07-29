//! Transaction integration tests for RPC injection, mempool handling, and PoL transactions

use crate::e2e::{berachain_payload_attributes, setup_test_boilerplate, test_signer};
use alloy_consensus::BlockHeader;
use alloy_eips::eip7002::SYSTEM_ADDRESS;
use alloy_primitives::{Address, ChainId};
use alloy_sol_macro::sol;
use alloy_sol_types::SolCall;
use bera_reth::{
    node::BerachainNode,
    primitives::header::BlsPublicKey,
    transaction::{BerachainTxEnvelope, PoLTx},
};
use reth::{providers::BlockNumReader, transaction_pool::TransactionPool};
use reth_chainspec::EthChainSpec;
use reth_e2e_test_utils::{node::NodeTestContext, transaction::TransactionTestContext};
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::str::FromStr;

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

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes).await?;
    let signer = test_signer()?;
    let chain_id = chain_spec.chain_id();

    let tx_bytes = TransactionTestContext::transfer_tx_bytes(chain_id, signer).await;
    let res = ctx.rpc.inject_tx(tx_bytes).await;

    assert!(res.is_ok(), "EIP1559 transaction should be accepted via RPC");
    let tx_hash = res.unwrap();
    println!("EIP1559 transaction accepted with hash: {tx_hash:?}");

    // Get initial block number before mining
    let initial_block = ctx.rpc.inner.eth_api().provider().best_block_number()?;

    // Mine a block to include the transaction
    let payload = ctx.advance_block().await?;
    let block = payload.block;
    let transactions = &block.body().transactions;

    // Verify the block was mined
    assert!(block.number > initial_block, "Block number should advance");

    // Verify the block contains exactly two transactions: PoL tx (first) + EIP1559 tx (second)
    assert_eq!(
        transactions.len(),
        2,
        "Block should contain exactly 2 transactions: PoL tx + EIP1559 tx, found: {}",
        transactions.len()
    );

    // Check transaction order: PoL transaction first, EIP1559 transaction second
    let tx_hashes: Vec<_> = transactions.iter().map(|tx| *tx.hash()).collect();

    // The second transaction should be our EIP1559 transaction
    assert_eq!(
        tx_hashes[1], tx_hash,
        "Second transaction should be our EIP1559 transaction. Expected: {tx_hash:?}, Found: {:?}",
        tx_hashes[1]
    );

    println!(
        "✅ Block {} contains PoL tx + EIP1559 tx in correct order: {:?}",
        block.number, tx_hashes
    );

    Ok(())
}

/// Creates a fake PoL transaction for testing rejection behavior
fn create_fake_pol_tx(chain_id: u64) -> PoLTx {
    use alloy_primitives::{Address, Bytes};

    PoLTx {
        chain_id,
        from: Default::default(),
        to: Address::random(),
        nonce: 0,
        gas_limit: 30_000_000,
        gas_price: 1_000_000_000u128, // 1 gwei
        input: Bytes::from(b"fake_pol_data"),
    }
}

#[tokio::test]
async fn test_pol_transaction_rpc_injection_fails() -> eyre::Result<()> {
    use alloy_eips::eip2718::Encodable2718;
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
    let fake_pol_tx = create_fake_pol_tx(chain_id);

    // Encode PoL transaction for RPC submission
    let mut buf = Vec::with_capacity(fake_pol_tx.encode_2718_len());
    fake_pol_tx.encode_2718(&mut buf);

    // Expected behavior: RPC should reject manually submitted PoL transactions
    // 1. Type inference causes recover_raw_transaction to be called with EthereumTxEnvelope
    // 2. EthereumTxEnvelope doesn't recognize PoL transaction type (0x7E/126)
    // 3. decode_2718 fails with RlpError(Custom("unexpected tx type"))
    // 4. This gets mapped to FailedToDecodeSignedTransaction for RPC response
    let rpc_result = ctx.rpc.inject_tx(buf.into()).await;
    let rpc_error = rpc_result.expect_err("PoL transaction should be rejected via RPC");

    assert!(
        matches!(rpc_error, EthApiError::FailedToDecodeSignedTransaction),
        "Expected FailedToDecodeSignedTransaction, got: {rpc_error}"
    );

    Ok(())
}

#[tokio::test]
async fn test_pol_transaction_mempool_insertion_fails() -> eyre::Result<()> {
    use alloy_primitives::Sealed;
    use bera_reth::transaction::BerachainTxEnvelope;
    use reth_primitives_traits::SignedTransaction;
    use reth_transaction_pool::TransactionOrigin;

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
    let fake_pol_tx = create_fake_pol_tx(chain_id);

    // Create a properly sealed PoL transaction envelope for consensus layer
    let pol_tx_sealed = Sealed::new(fake_pol_tx);
    let pol_tx_envelope = BerachainTxEnvelope::Berachain(pol_tx_sealed);
    let recovered_pol_tx = pol_tx_envelope
        .try_into_recovered()
        .expect("PoL transaction should be recoverable as consensus transaction");

    // Expected behavior: PoolTransaction::try_from_consensus() calls try_into() which
    // triggers TxConversionError::UnsupportedBerachainTransaction for PoL transactions
    let pool_result = ctx
        .rpc
        .inner
        .pool()
        .add_consensus_transaction(recovered_pol_tx, TransactionOrigin::External)
        .await;

    let pool_error = pool_result.expect_err("PoL transaction should be rejected by mempool");
    let error_msg = pool_error.to_string();

    // Verify the error contains the expected UnsupportedBerachainTransaction message
    assert!(
        error_msg.contains("Cannot convert Berachain POL transaction to Ethereum format"),
        "Expected UnsupportedBerachainTransaction error, got: {error_msg}"
    );

    Ok(())
}

#[tokio::test]
async fn test_pol_transaction_auto_inclusion() -> eyre::Result<()> {
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

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes).await?;
    let initial_block = ctx.rpc.inner.eth_api().provider().best_block_number()?;

    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let transactions = &block.body().transactions;

    assert!(!transactions.is_empty(), "Block should contain at least one PoL transaction");
    assert!(block.number > initial_block, "Block number should advance");

    assert!(
        matches!(&transactions[0], BerachainTxEnvelope::Berachain(_)),
        "First transaction should be PoL type"
    );
    let BerachainTxEnvelope::Berachain(pol_tx_sealed) = &transactions[0] else { unreachable!() };

    let pol_tx = pol_tx_sealed.as_ref();
    let block_base_fee = block.header().base_fee_per_gas().expect("Block should have base fee");
    let expected_pol_contract = Address::from_str("0x4200000000000000000000000000000000000042")
        .expect("Valid PoL contract address");

    // Validate all PoL transaction fields
    assert_eq!(pol_tx.chain_id, ChainId::from(80087u64));
    assert_eq!(pol_tx.from, SYSTEM_ADDRESS);
    assert_eq!(pol_tx.to, expected_pol_contract);
    assert_eq!(pol_tx.nonce, 0);
    assert_eq!(pol_tx.gas_limit, 30_000_000);
    assert_eq!(pol_tx.gas_price, block_base_fee as u128);
    assert!(!pol_tx.input.is_empty());

    // Validate input is valid distributeFor call
    sol! {
        interface PoLDistributor {
            function distributeFor(bytes calldata pubkey) external;
        }
    }

    let decoded_call = PoLDistributor::distributeForCall::abi_decode(&pol_tx.input)
        .expect("Should decode as distributeFor call");
    assert_eq!(decoded_call.pubkey.len(), 48, "BLS public key should be 48 bytes");

    // Validate that the pubkey in the PoL transaction matches the header's prev_proposer_pubkey
    let header_pubkey = block
        .header()
        .prev_proposer_pubkey
        .expect("Block header should contain prev_proposer_pubkey");
    let pol_pubkey = BlsPublicKey::from_slice(&decoded_call.pubkey);
    assert_eq!(
        pol_pubkey, header_pubkey,
        "PoL transaction pubkey should match header's prev_proposer_pubkey"
    );

    Ok(())
}
