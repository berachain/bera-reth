//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    berachain_cli::BerachainSubcommands,
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
    version::init_bera_version,
};
use clap::Parser;
use reth::CliRunner;
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::interface::{Cli, Commands};
use reth_node_builder::NodeHandle;
use reth_rpc_server_types::DefaultRpcModuleValidator;
use std::{marker::PhantomData, sync::Arc};
use tracing::info;

/// Persist every canonical block to disk immediately rather than buffering.
/// Upstream reth defaults to 2, but Berachain's faster block times benefit from
/// eager persistence to keep the in-memory block window minimal.
const BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD: u64 = 0;

fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Initialize Bera-Reth version metadata
    init_bera_version().expect("Failed to initialize Bera-Reth version metadata");

    reth_node_core::args::DefaultEngineValues::default()
        .with_persistence_threshold(BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD)
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

    let cli = Cli::<
        BerachainChainSpecParser,
        NoArgs,
        DefaultRpcModuleValidator,
        BerachainSubcommands,
    >::parse();

    let reth_ethereum_cli::interface::Cli { command, logs, traces, _phantom } = cli;

    match command {
        Commands::Ext(BerachainSubcommands::Console(console_cmd)) => {
            let runner =
                CliRunner::try_default_runtime().expect("Failed to create default runtime");
            if let Err(err) = console_cmd.run(runner) {
                eprintln!("Error: {err:?}");
                std::process::exit(1);
            }
        }
        other => {
            let cli = Cli {
                command: other,
                logs,
                traces,
                _phantom: PhantomData::<DefaultRpcModuleValidator>,
            };
            if let Err(err) = cli.with_runner_and_components::<BerachainNode>(
                CliRunner::try_default_runtime().expect("Failed to create default runtime"),
                cli_components_builder,
                async move |builder, _| {
                    info!(target: "reth::cli", "Launching Berachain node");
                    let NodeHandle { node: _node, node_exit_future } = builder
                        .node(BerachainNode::default())
                        .launch_with_debug_capabilities()
                        .await?;

                    node_exit_future.await
                },
            ) {
                eprintln!("Error: {err:?}");
                std::process::exit(1);
            }
        }
    }
}
