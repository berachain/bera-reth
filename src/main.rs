//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{chainspec::BerachainChainSpecParser, node::BerachainNode};
use clap::Parser;
use reth::{CliRunner, beacon_consensus::EthBeaconConsensus};
use std::sync::Arc;
// Removed RessArgs since ress subprotocol is disabled
use bera_reth::{chainspec::BerachainChainSpec, node::evm::config::BerachainEvmConfig};
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::Cli;
use reth_evm::EthEvmFactory;
use reth_node_builder::NodeHandle;
use tracing::info;

/// Main entry point. Sets up runtime, signal handlers, and launches Berachain node.
fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    // This helps with debugging issues during development and operation.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    let cli_components_builder = |spec: Arc<BerachainChainSpec>| {
        (
            BerachainEvmConfig::new_with_evm_factory(spec.clone(), EthEvmFactory::default()),
            EthBeaconConsensus::new(spec),
        )
    };

    if let Err(err) = Cli::<BerachainChainSpecParser, NoArgs>::parse()
        .with_runner_and_components::<BerachainNode>(
            CliRunner::try_default_runtime().expect("Failed to create default runtime"),
            cli_components_builder,
            async move |builder, _| {
                info!(target: "reth::cli", "Launching Berachain node");
                let NodeHandle { node: _node, node_exit_future } =
                    builder.node(BerachainNode::default()).launch_with_debug_capabilities().await?;

                node_exit_future.await
            },
        )
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
