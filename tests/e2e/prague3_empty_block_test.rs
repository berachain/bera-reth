//! Pins the Prague3 empty-block builder behavior that ran on Berachain mainnet
//! during the closed historical window `[1762164459, 1762963200)`. The genesis
//! fixture lives under `tests/fixtures/historical/` (see the README there) to
//! signal that it models a non-live fork window. The consensus rules being
//! exercised are documented in `src/consensus/mod.rs`.

use crate::e2e::berachain_payload_attributes_generator;
use bera_reth::{
    chainspec::BerachainChainSpec, node::BerachainNode, transaction::BerachainTxEnvelope,
};
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
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/historical/eth-genesis-prague3.json");
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
    let txs = &block.body().transactions;

    assert_eq!(block.number, 1, "should have advanced to block 1");
    assert_eq!(
        txs.len(),
        1,
        "Prague3 block must contain only the PoL system transaction, found {}",
        txs.len()
    );
    assert!(
        matches!(txs[0], BerachainTxEnvelope::Berachain(_)),
        "sole transaction must be the PoL system transaction"
    );

    Ok(())
}
