//! Berachain EVM executor using standard Ethereum execution with Berachain chain spec

mod assembler;
mod block_context;
pub mod config;
pub mod error;
pub mod executor;
pub mod receipt;

use crate::node::{BerachainNode, evm::config::BerachainEvmConfig};
use alloy_primitives::Bytes;
use reth_evm::EthEvmFactory;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};

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
    type EVM = BerachainEvmConfig;

    /// Builds standard Ethereum EVM config with Berachain chain spec
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config =
            BerachainEvmConfig::new_with_evm_factory(ctx.chain_spec(), EthEvmFactory::default())
                .with_extra_data(default_extra_data_bytes());
        Ok(evm_config)
    }
}
