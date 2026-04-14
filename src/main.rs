//! Bera-Reth main entry point

mod cli_ext;

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
use cli_ext::BerachainExt;
use reth::CliRunner;
use reth_ethereum_cli::Cli;
use reth_node_builder::NodeHandle;
use std::sync::Arc;
use tracing::info;

/// Persist every canonical block to disk immediately rather than buffering.
/// Upstream reth defaults to 2, but Berachain's faster block times benefit from
/// eager persistence to keep the in-memory block window minimal.
const BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD: u64 = 0;

fn main() {
    reth_cli_util::sigsegv_handler::install();

    init_bera_version().expect("Failed to initialize Bera-Reth version metadata");

    reth_node_core::args::DefaultEngineValues::default()
        .with_persistence_threshold(BERACHAIN_DEFAULT_PERSISTENCE_THRESHOLD)
        .try_init()
        .expect("engine defaults must be set before CLI parsing");

    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    let cli_components_builder = |spec: Arc<BerachainChainSpec>| {
        (
            BerachainEvmConfig::new_with_evm_factory(spec.clone(), BerachainEvmFactory::default()),
            Arc::new(BerachainBeaconConsensus::new(spec)),
        )
    };

    let cli = Cli::<BerachainChainSpecParser, BerachainExt>::parse();

    if let Err(err) = cli.with_runner_and_components::<BerachainNode>(
        CliRunner::try_default_runtime().expect("Failed to create default runtime"),
        cli_components_builder,
        async move |builder, args: BerachainExt| {
            bera_reth::pog::set_pog_cli_enabled(args.pog);

            info!(target: "reth::cli", "Launching Berachain node");
            let launch_result =
                builder.node(BerachainNode::default()).launch_with_debug_capabilities().await;
            let NodeHandle { node, node_exit_future } = launch_result?;

            let _node_guard = node;
            node_exit_future.await
        },
    ) {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
