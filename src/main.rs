//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
    version::init_bera_version,
};
use clap::Parser;
use reth::CliRunner;
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::Cli;
use reth_node_builder::NodeHandle;
use std::sync::Arc;
use tracing::info;

/// Production engine defaults: persist canonical blocks immediately and do not
/// retain an extra in-memory window behind the head (`memory_block_buffer_target`).
/// Upstream reth uses a higher persistence threshold; Berachain's block times
/// favor minimal in-memory buffering.
const BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD: u64 = 0;
const BERACHAIN_DEFAULT_MEMORY_BLOCK_BUFFER_TARGET: u64 = 0;

fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Initialize Bera-Reth version metadata
    init_bera_version().expect("Failed to initialize Bera-Reth version metadata");

    reth_node_core::args::DefaultEngineValues::default()
        .with_persistence_threshold(BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD)
        .with_memory_block_buffer_target(BERACHAIN_DEFAULT_MEMORY_BLOCK_BUFFER_TARGET)
        .try_init()
        .expect("engine defaults must be set before CLI parsing");

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    let cli_components_builder = |spec: Arc<BerachainChainSpec>| {
        (
            BerachainEvmConfig::new_with_evm_factory(spec.clone(), BerachainEvmFactory::default()),
            Arc::new(BerachainBeaconConsensus::new(spec)),
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
