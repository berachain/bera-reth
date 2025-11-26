//! Berachain Sequencer Binary
//!
//! This binary runs bera-reth with a custom payload builder for sequencing.
//!
//! Usage: `cargo run --bin bera-sequencer -- node`

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    engine::{rpc::BerachainEngineApiBuilder, validator::BerachainEngineValidatorBuilder},
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
    platform::BerachainPlatform,
    rpc::{BerachainAddOns, BerachainEthApiBuilder},
    version::init_bera_version,
};
use clap::Parser;
use rblib::{
    pool::{AppendOrders, HostNodeInstaller, OrderPool},
    prelude::{Loop, Pipeline},
    steps::{OrderByPriorityFee, RemoveRevertedTransactions},
};
use reth::CliRunner;
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::Cli;
use reth_node_builder::{Node, NodeHandle};
use std::sync::Arc;
use tracing::info;

/// Example Berachain builder pipeline with Revert Protection
fn berachain_builder_pipeline(pool: &OrderPool<BerachainPlatform>) -> Pipeline<BerachainPlatform> {
    let pipeline = Pipeline::<BerachainPlatform>::named("classic").with_pipeline(
        Loop,
        (
            AppendOrders::from_pool(pool),
            OrderByPriorityFee::default(),
            RemoveRevertedTransactions::default(),
        ),
    );

    pool.attach_pipeline(&pipeline);
    pipeline
}

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

    if let Err(err) = Cli::<BerachainChainSpecParser, NoArgs>::parse()
        .with_runner_and_components::<BerachainNode>(
            CliRunner::try_default_runtime().expect("Failed to create default runtime"),
            cli_components_builder,
            async move |builder, _| {
                info!(target: "reth::cli", "Launching Berachain Sequencer node");
                let pool = OrderPool::<BerachainPlatform>::default();
                let pipeline = berachain_builder_pipeline(&pool);
                let berachain_node = BerachainNode::default();

                // Helps compiler
                let add_ons: BerachainAddOns<
                    _,
                    BerachainEthApiBuilder,
                    BerachainEngineValidatorBuilder,
                    BerachainEngineApiBuilder<BerachainEngineValidatorBuilder>,
                > = BerachainAddOns::default();

                let NodeHandle { node: _node, node_exit_future } = builder
                    .with_types::<BerachainNode>()
                    .with_components(
                        berachain_node
                            .components_builder()
                            .attach_pool(&pool)
                            .payload(pipeline.into_service()),
                    )
                    .with_add_ons(add_ons)
                    .launch_with_debug_capabilities()
                    .await?;

                node_exit_future.await
            },
        )
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
