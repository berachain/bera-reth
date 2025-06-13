use reth_node_builder::PayloadBuilderConfig;

use crate::node::BerachainNode;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};
use reth_node_ethereum::EthEvmConfig;
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for BerachainExecutorBuilder
where
    Node: FullNodeTypes<Types = BerachainNode>,
{
    type EVM = EthEvmConfig;
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config = EthEvmConfig::new(Arc::from(ctx.chain_spec().inner()))
            .with_extra_data(ctx.payload_builder_config().extra_data_bytes());
        Ok(evm_config)
    }
}
