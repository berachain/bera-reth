//! Berachain EVM executor using standard Ethereum execution with Berachain chain spec

use reth_node_builder::PayloadBuilderConfig;

use crate::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth_evm::EthEvmFactory;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};
use reth_node_ethereum::EthEvmConfig;

/// Creates standard Ethereum EVM with Berachain chain spec
#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for BerachainExecutorBuilder
where
    Node: FullNodeTypes<Types = BerachainNode>,
{
    /// The EVM configuration type that will be built
    type EVM = EthEvmConfig<BerachainChainSpec, EthEvmFactory>;

    /// Builds standard Ethereum EVM config with Berachain chain spec
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config =
            EthEvmConfig::new_with_evm_factory(ctx.chain_spec().clone(), EthEvmFactory::default())
                .with_extra_data(ctx.payload_builder_config().extra_data_bytes());
        Ok(evm_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_builder() {
        let builder = BerachainExecutorBuilder;

        // Test Debug implementation
        let debug_str = format!("{builder:?}");
        assert!(debug_str.contains("BerachainExecutorBuilder"));
    }

    #[test]
    fn test_executor_builder_copy() {
        let builder = BerachainExecutorBuilder;
        let copied = builder; // Copy due to Copy trait

        // Both should be usable and identical
        assert_eq!(format!("{builder:?}"), format!("{copied:?}"));

        let _builder1 = builder;
        let _builder2 = copied;
    }
}
