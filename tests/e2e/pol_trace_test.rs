//! Regression test for tracing the PoL system transaction over the debug API.
//!
//! The PoL distribution runs once per block, in `BerachainBlockExecutor`'s pre-execution hook,
//! and also appears as the type-0x7e transaction at index 0 of the block. reth's tracing helpers
//! replay every block transaction through `Evm::transact`, which executes the PoL envelope as a
//! system call, so if the trace path also ran the executor's pre-execution hook the distribution
//! would be applied twice and traces would observe storage one distribution ahead of what the
//! block actually committed. This pins the trace output to the canonical state.

use crate::e2e::{
    POL_DISTRIBUTOR_ADDRESS, berachain_payload_attributes_generator, setup_test_boilerplate,
};
use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types_eth::BlockId;
use alloy_rpc_types_trace::geth::{
    AccountState, GethDebugTracingOptions, GethTrace, PreStateConfig, PreStateFrame, TraceResult,
};
use alloy_serde::JsonStorageKey;
use bera_reth::{node::BerachainNode, transaction::BerachainTxEnvelope};
use reth::rpc::api::EthApiServer;
use reth_e2e_test_utils::node::NodeTestContext;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::{args::RpcServerArgs, node_config::NodeConfig};
use reth_payload_primitives::BuiltPayload;
use std::{collections::BTreeMap, str::FromStr};

/// Storage slot 0 of the PoL distributor holds the number of distributions performed so far,
/// incremented by exactly one on every block's PoL transaction.
const DISTRIBUTION_COUNTER_SLOT: B256 = B256::ZERO;

#[tokio::test]
async fn test_debug_trace_pol_tx_matches_committed_state() -> eyre::Result<()> {
    let (runtime, chain_spec) = setup_test_boilerplate().await?;

    let node_config = NodeConfig::new(chain_spec)
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http());

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config)
        .testing_node(runtime.clone())
        .node(BerachainNode::default())
        .launch()
        .await?;

    let mut ctx = NodeTestContext::new(node, berachain_payload_attributes_generator).await?;
    let distributor = Address::from_str(POL_DISTRIBUTOR_ADDRESS)?;

    // Mine a few blocks so the counter is well away from zero, then trace the last one.
    let mut payload = ctx.advance_block().await?;
    for _ in 0..2 {
        payload = ctx.advance_block().await?;
    }
    let block = payload.block();
    let block_number = block.number;
    let pol_tx = &block.body().transactions[0];
    assert!(
        matches!(pol_tx, BerachainTxEnvelope::Berachain(_)),
        "first transaction of every block must be the PoL system transaction"
    );

    // Ground truth from committed state: the counter before and after this block.
    let eth_api = ctx.rpc.inner.eth_api();
    let counter_at = |number: u64| async move {
        eth_api
            .storage_at(
                distributor,
                JsonStorageKey::Hash(DISTRIBUTION_COUNTER_SLOT),
                Some(BlockId::number(number)),
            )
            .await
            .map(|v| U256::from_be_bytes(v.0))
    };
    let committed_pre = counter_at(block_number - 1).await?;
    let committed_post = counter_at(block_number).await?;
    assert_eq!(
        committed_post,
        committed_pre + U256::from(1),
        "each block's PoL transaction must advance the distribution counter by exactly one"
    );

    let opts = GethDebugTracingOptions::prestate_tracer(PreStateConfig {
        diff_mode: Some(true),
        ..Default::default()
    });
    let debug_api = ctx.rpc.inner.debug_api();

    let counter_diff = |trace: GethTrace| -> eyre::Result<(U256, U256)> {
        let GethTrace::PreStateTracer(PreStateFrame::Diff(diff)) = trace else {
            return Err(eyre::eyre!("expected a prestate diff frame, got {trace:?}"));
        };
        let slot = |state: &BTreeMap<Address, AccountState>| -> eyre::Result<U256> {
            let account = state
                .get(&distributor)
                .ok_or_else(|| eyre::eyre!("distributor missing from prestate diff"))?;
            let value = account.storage.get(&DISTRIBUTION_COUNTER_SLOT).ok_or_else(|| {
                eyre::eyre!("distribution counter slot missing from prestate diff")
            })?;
            Ok(U256::from_be_bytes(value.0))
        };
        Ok((slot(&diff.pre)?, slot(&diff.post)?))
    };

    // debug_traceTransaction replays the block up to the PoL tx, then traces it.
    let tx_trace = debug_api.debug_trace_transaction(*pol_tx.hash(), opts.clone()).await?;
    assert_eq!(
        counter_diff(tx_trace)?,
        (committed_pre, committed_post),
        "debug_traceTransaction of the PoL tx must see the committed pre/post counter"
    );

    // debug_traceBlockByNumber traces every transaction of the block from parent state.
    let mut block_traces = debug_api.debug_trace_block(BlockId::number(block_number), opts).await?;
    let pol_trace = match block_traces.remove(0) {
        TraceResult::Success { result, .. } => result,
        TraceResult::Error { error, .. } => {
            return Err(eyre::eyre!("PoL tx trace failed: {error}"));
        }
    };
    assert_eq!(
        counter_diff(pol_trace)?,
        (committed_pre, committed_post),
        "debug_traceBlockByNumber must see the committed pre/post counter for the PoL tx"
    );

    Ok(())
}
