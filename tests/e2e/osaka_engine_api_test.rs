//! Verifies Berachain serves `engine_getPayloadV4P11` post-Osaka.
//!
//! BeaconKit calls `getPayloadV4P11` for both Prague1 and Osaka payloads. Upstream reth's
//! V4 metered handler rejects retrievals once Osaka is active, so without the bera-reth
//! override the call returns `-38005 Unsupported fork`.

use crate::e2e::berachain_payload_attributes_generator;
use bera_reth::{chainspec::BerachainChainSpec, node::BerachainNode};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use reth::tasks::Runtime;
use reth_cli::chainspec::parse_genesis;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::PayloadBuilderAttributes;
use serde_json::Value;
use std::sync::Arc;

#[tokio::test]
async fn test_get_payload_v4_p11_works_post_osaka() -> eyre::Result<()> {
    let runtime = Runtime::with_existing_handle(tokio::runtime::Handle::current())?;

    let genesis_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eth-genesis.json");
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

    // Trigger a payload build via the local builder handle so we get a valid payload_id.
    let attrs = ctx.payload.new_payload().await?;
    let payload_id = attrs.payload_id();
    ctx.payload.wait_for_built_payload(payload_id).await;

    // Call engine_getPayloadV4P11 over the auth RPC. Pre-fix this returns
    // `-38005 Unsupported fork` because upstream's V4 metered handler rejects
    // retrievals when the payload's timestamp is at or after Osaka activation.
    let auth_client = ctx.inner.auth_server_handle().http_client();
    let envelope: Value = auth_client
        .request("engine_getPayloadV4P11", rpc_params![payload_id])
        .await
        .map_err(|err| eyre::eyre!("engine_getPayloadV4P11 failed post-Osaka: {err}"))?;

    assert!(
        envelope.get("executionPayload").is_some(),
        "expected executionPayload field in V4P11 response, got {envelope}"
    );

    Ok(())
}
