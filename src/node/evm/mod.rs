//! # Berachain EVM Execution Environment
//!
//! This module provides the execution environment configuration for Berachain nodes.
//! It implements the [`ExecutorBuilder`] trait to create EVM instances that are
//! compatible with Berachain's custom hardforks and consensus mechanisms.
//!
//! The executor handles:
//! - Transaction execution with Berachain-specific rules
//! - State transitions according to custom hardforks
//! - Integration with Reth's modular architecture

use reth_node_builder::PayloadBuilderConfig;

use crate::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth_evm::EthEvmFactory;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};
use reth_node_ethereum::EthEvmConfig;

/// Builder for creating Berachain-specific EVM execution environments.
///
/// This builder creates EVM configurations that support Berachain's custom
/// hardforks and consensus rules. It integrates with Reth's node builder
/// pattern to provide modular execution capabilities.
///
/// The executor builder configures:
/// - Custom chain specification handling
/// - Berachain-specific hardfork logic
/// - Payload building with custom extra data
///
/// # Example
///
/// ```no_run
/// use bera_reth::node::evm::BerachainExecutorBuilder;
/// use reth_node_builder::components::ExecutorBuilder;
///
/// let builder = BerachainExecutorBuilder::default();
/// // Use with node builder...
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct BerachainExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for BerachainExecutorBuilder
where
    Node: FullNodeTypes<Types = BerachainNode>,
{
    /// The EVM configuration type that will be built
    type EVM = EthEvmConfig<BerachainChainSpec, EthEvmFactory>;

    /// Builds the EVM configuration for Berachain execution.
    ///
    /// This method creates an EVM configuration that:
    /// - Uses the Berachain chain specification for custom hardfork logic
    /// - Integrates with the standard Ethereum EVM factory
    /// - Configures payload building with custom extra data
    ///
    /// # Arguments
    ///
    /// * `ctx` - The builder context containing chain spec and configuration
    ///
    /// # Returns
    ///
    /// A configured [`EthEvmConfig`] ready for transaction execution
    ///
    /// # Errors
    ///
    /// Returns an error if the EVM configuration cannot be created
    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config =
            EthEvmConfig::new_with_evm_factory(ctx.chain_spec().clone(), EthEvmFactory::default())
                .with_extra_data(ctx.payload_builder_config().extra_data_bytes());
        Ok(evm_config)
    }
}
