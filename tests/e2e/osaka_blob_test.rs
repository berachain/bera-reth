//! Verifies EIP-4844 blob transactions are accepted and included in blocks once Osaka is active.

use crate::e2e::{berachain_payload_attributes_generator, test_signer};
use bera_reth::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth::tasks::Runtime;
use reth_chainspec::EthChainSpec;
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::{node::NodeTestContext, transaction::TransactionTestContext};
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::sync::Arc;

#[tokio::test]
async fn test_eip4844_blob_tx_accepted_post_osaka() -> eyre::Result<()> {
    let runtime = Runtime::with_existing_handle(tokio::runtime::Handle::current())?;

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let genesis = parse_genesis(&genesis_json)?;
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));
    let chain_id = chain_spec.chain_id();

    let node_config = NodeConfig::new(chain_spec)
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(runtime.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;
    let signer = test_signer()?;

    let blob_tx_bytes = TransactionTestContext::tx_with_blobs_bytes(chain_id, signer).await?;

    let blob_tx_hash = ctx
        .rpc
        .inject_tx(blob_tx_bytes)
        .await
        .map_err(|err| eyre::eyre!("blob tx rejected by RPC under Osaka: {err}"))?;

    let payload = ctx.advance_block().await?;
    let block = payload.block();
    let included = block.body().transactions.iter().any(|tx| *tx.hash() == blob_tx_hash);

    assert!(
        included,
        "EIP-4844 blob tx {blob_tx_hash:?} should be included in block {} after Osaka activation",
        block.number
    );

    Ok(())
}
