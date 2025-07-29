//! PoL (Proof of Liquidity) transaction integration tests

use crate::e2e::berachain_payload_attributes;
use alloy_consensus::BlockHeader;
use alloy_eips::eip7002::SYSTEM_ADDRESS;
use alloy_primitives::{Address, ChainId};
use alloy_sol_macro::sol;
use alloy_sol_types::SolCall;
use bera_reth::{
    chainspec::BerachainChainSpec, node::BerachainNode, primitives::header::BlsPublicKey,
    transaction::BerachainTxEnvelope,
};
use reth::{providers::BlockNumReader, tasks::TaskManager};
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::{str::FromStr, sync::Arc};

#[tokio::test]
async fn test_pol_transaction_auto_inclusion() -> eyre::Result<()> {
    let tasks = TaskManager::current();
    let executor = tasks.executor();

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path).expect("Failed to read genesis file");
    let genesis = parse_genesis(&genesis_json).expect("Failed to parse genesis");
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));

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
