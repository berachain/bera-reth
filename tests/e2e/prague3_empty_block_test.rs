use crate::e2e::berachain_payload_attributes_generator;
use bera_reth::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth::tasks::Runtime;
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::sync::Arc;

#[tokio::test]
async fn test_prague3_builds_empty_block() -> eyre::Result<()> {
    let runtime = Runtime::with_existing_handle(tokio::runtime::Handle::current())?;

    let genesis_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis-prague3.json");
    let genesis_json = std::fs::read_to_string(genesis_path)?;
    let genesis = parse_genesis(&genesis_json)?;
    let chain_spec = Arc::new(BerachainChainSpec::from(genesis));

    let node_config = NodeConfig::new(chain_spec)
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(runtime.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;

    let payload = ctx.advance_block().await?;
    let block = payload.block();

    assert_eq!(block.number, 1, "should have advanced to block 1");
    assert!(
        block.body().transactions.is_empty(),
        "Prague3 block must contain zero transactions, found {}",
        block.body().transactions.len()
    );

    Ok(())
}
