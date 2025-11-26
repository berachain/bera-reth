//! Berachain EVM executor using standard Ethereum execution with Berachain chain spec

mod assembler;
pub mod block_context;
pub mod config;
pub mod error;
pub mod executor;
pub mod receipt;

pub use config::BerachainEvmConfig;

use crate::evm;
use alloy_primitives::Bytes;
use reth_node_api::NodeTypes;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};

/// Default extra data for Berachain blocks
fn default_extra_data() -> String {
    format!("bera-reth/v{}/{}", env!("CARGO_PKG_VERSION"), std::env::consts::OS)
}

/// Default extra data in bytes for Berachain blocks
pub fn default_extra_data_bytes() -> Bytes {
    Bytes::from(default_extra_data().as_bytes().to_vec())
}

/// Creates standard Ethereum EVM with Berachain chain spec
#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for BerachainExecutorBuilder
where
    Node: FullNodeTypes<
        Types: NodeTypes<
            ChainSpec = crate::chainspec::BerachainChainSpec,
            Primitives = crate::primitives::BerachainPrimitives,
        >,
    >,
{
    /// The EVM configuration type that will be built
    type EVM = BerachainEvmConfig;

    /// Builds standard Ethereum EVM config with Berachain chain spec
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config = BerachainEvmConfig::new_with_evm_factory(
            ctx.chain_spec(),
            evm::BerachainEvmFactory::default(),
        )
        .with_extra_data(default_extra_data_bytes());
        Ok(evm_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_extra_data_format() {
        let expected = format!("bera-reth/v{}/{}", env!("CARGO_PKG_VERSION"), std::env::consts::OS);
        assert_eq!(default_extra_data(), expected);
    }
}
