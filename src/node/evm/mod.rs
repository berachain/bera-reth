//! Berachain EVM executor using standard Ethereum execution with Berachain chain spec

use alloy_primitives::Bytes;

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
        // Always use Berachain-specific extra_data
        let evm_config =
            EthEvmConfig::new_with_evm_factory(ctx.chain_spec().clone(), EthEvmFactory::default())
                .with_extra_data(default_extra_data_bytes());
        Ok(evm_config)
    }
}
