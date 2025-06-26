//! Berachain EVM executor using standard Ethereum execution with Berachain chain spec

use alloy_primitives::Bytes;
use reth_node_builder::PayloadBuilderConfig;

use crate::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth_evm::EthEvmFactory;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};
use reth_node_ethereum::EthEvmConfig;

/// Default extra data for Berachain blocks
fn default_extra_data() -> String {
    format!("bera-reth/v{}/{}", env!("CARGO_PKG_VERSION"), std::env::consts::OS)
}

/// Default extra data in bytes for Berachain blocks
fn default_extra_data_bytes() -> Bytes {
    Bytes::from(default_extra_data().as_bytes().to_vec())
}

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
        // Use custom Berachain extra_data if no custom extra_data is configured
        let extra_data = if ctx.payload_builder_config().extra_data_bytes().is_empty() {
            default_extra_data_bytes()
        } else {
            ctx.payload_builder_config().extra_data_bytes()
        };

        let evm_config =
            EthEvmConfig::new_with_evm_factory(ctx.chain_spec().clone(), EthEvmFactory::default())
                .with_extra_data(extra_data);
        Ok(evm_config)
    }
}
