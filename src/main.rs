//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    args::BerachainArgs,
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
    proof_of_gossip::new_pog_service,
    version::init_bera_version,
};
use clap::Parser;
use reth::{
    CliRunner,
    chainspec::{ChainSpecProvider, EthChainSpec},
};
use reth_ethereum_cli::Cli;
use reth_node_builder::NodeHandle;
use std::sync::Arc;
use tracing::info;

fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Initialize Bera-Reth version metadata
    init_bera_version().expect("Failed to initialize Bera-Reth version metadata");

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

    if let Err(err) = Cli::<BerachainChainSpecParser, BerachainArgs>::parse()
        .with_runner_and_components::<BerachainNode>(
            CliRunner::try_default_runtime().expect("Failed to create default runtime"),
            cli_components_builder,
            async move |builder, args| {
                info!(target: "reth::cli", "Launching Berachain node");
                let NodeHandle { node, node_exit_future } =
                    builder.node(BerachainNode::default()).launch_with_debug_capabilities().await?;

                if let Some(service) = new_pog_service(
                    node.network.clone(),
                    node.provider.clone(),
                    node.provider.chain_spec().chain().id(),
                    node.config.datadir().data_dir().to_path_buf(),
                    &args,
                )
                .await?
                {
                    node.task_executor.spawn(Box::pin(service.run()));
                }

                node_exit_future.await
            },
        )
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
