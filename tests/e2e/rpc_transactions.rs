//! RPC transaction integration tests

use crate::e2e::{berachain_payload_attributes, setup_test_boilerplate, test_signer};
use bera_reth::node::BerachainNode;
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
