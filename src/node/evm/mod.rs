//! # Berachain EVM Execution Environment
//!
//! This module provides the execution environment configuration for Berachain nodes.
//! It implements the [`ExecutorBuilder`] trait to create EVM instances that are
//! identical to Ethereum's EVM execution.
//!
//! The executor handles:
//! - Standard Ethereum transaction execution
//! - State transitions according to Ethereum hardforks (plus Berachain's Prague1)
//! - Integration with Reth's modular architecture

use reth_node_builder::PayloadBuilderConfig;

use crate::{chainspec::BerachainChainSpec, node::BerachainNode};
use reth_evm::EthEvmFactory;
use reth_node_builder::{BuilderContext, FullNodeTypes, components::ExecutorBuilder};
use reth_node_ethereum::EthEvmConfig;

/// Builder for creating Berachain EVM execution environments.
///
/// This builder creates EVM configurations that are identical to Ethereum's
/// standard EVM execution, with the exception that it uses Berachain's
/// chain specification which includes the Prague1 hardfork for minimum
/// base fee enforcement.
///
/// The executor builder configures:
/// - Standard Ethereum EVM execution using `EthEvmConfig`
/// - Berachain chain specification (which extends Ethereum hardforks)
/// - Payload building with standard extra data handling
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
    /// This method creates an EVM configuration that is identical to Ethereum's
    /// standard EVM setup, using:
    /// - Berachain's chain specification (which extends Ethereum with Prague1)
    /// - Standard Ethereum EVM factory (`EthEvmFactory`)
    /// - Standard payload building with extra data configuration
    ///
    /// The resulting EVM executes transactions using standard Ethereum rules,
    /// with the addition of Berachain's Prague1 hardfork for minimum base fee.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The builder context containing chain spec and configuration
    ///
    /// # Returns
    ///
    /// A configured [`EthEvmConfig`] ready for standard Ethereum transaction execution
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
