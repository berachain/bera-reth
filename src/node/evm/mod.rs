use reth_node_builder::PayloadBuilderConfig;

use crate::chainspec::BerachainChainSpec;
use crate::node::BerachainNode;
use reth_evm::EthEvmFactory;
use reth_node_builder::{components::ExecutorBuilder, BuilderContext, FullNodeTypes};
use reth_node_ethereum::EthEvmConfig;

#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for BerachainExecutorBuilder
where
    Node: FullNodeTypes<Types=BerachainNode>,
{
    type EVM = EthEvmConfig<BerachainChainSpec, EthEvmFactory>;
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config = EthEvmConfig::new_with_evm_factory(ctx.chain_spec().clone(), EthEvmFactory::default())
            .with_extra_data(ctx.payload_builder_config().extra_data_bytes());
        Ok(evm_config)
    }
}
