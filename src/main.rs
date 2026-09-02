//! Bera-Reth main entry point

mod download_manifest_url;

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig, init_engine_defaults},
    version::init_bera_version,
};
use clap::Parser;
use download_manifest_url::with_resolved_manifest_url;
use reth::CliRunner;
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::Cli;
use reth_node_builder::NodeHandle;
use std::sync::Arc;
use tracing::info;

fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Initialize Bera-Reth version metadata
    init_bera_version().expect("Failed to initialize Bera-Reth version metadata");

    init_engine_defaults();

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

    // For `download --chain <mainnet|bepolia>` with no explicit manifest
    // source, construct Berachain's fixed per-chain manifest URL before clap
    // ever sees the args. Upstream reth's own snapshot auto-discovery only
    // ever works for Ethereum mainnet (chain ID 1), which Berachain's chain
    // IDs (80094, 80069) never match, so without this every restore requires
    // an operator to hand-copy a manifest URL first. `args_os` rather than
    // `args`: the latter panics on any non-UTF-8 argument, which would
    // regress valid Unix paths (datadir, custom genesis) clap otherwise
    // accepts.
    let argv = with_resolved_manifest_url(std::env::args_os().collect());

    if let Err(err) = Cli::<BerachainChainSpecParser, NoArgs>::parse_from(argv)
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
