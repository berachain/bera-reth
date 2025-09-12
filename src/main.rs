//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
};
use clap::Parser;
use reth::CliRunner;
use reth_cli_commands::node::NoArgs;
use reth_ethereum_cli::Cli;
use reth_node_builder::NodeHandle;
use reth_node_core::version::{RethCliVersionConsts, try_init_version_metadata};
use std::{borrow::Cow, sync::Arc};
use tracing::info;

fn main() {
    // Install signal handler for better crash reporting
    reth_cli_util::sigsegv_handler::install();

    // Initialize Bera-Reth version metadata with build.rs generated info
    let _ = try_init_version_metadata(RethCliVersionConsts {
        name_client: Cow::Borrowed("Bera-Reth"),
        cargo_pkg_version: Cow::Borrowed(env!("CARGO_PKG_VERSION")),
        vergen_git_sha_long: Cow::Borrowed(env!("VERGEN_GIT_SHA")),
        vergen_git_sha: Cow::Borrowed(env!("VERGEN_GIT_SHA_SHORT")),
        vergen_build_timestamp: Cow::Borrowed(env!("VERGEN_BUILD_TIMESTAMP")),
        vergen_cargo_target_triple: Cow::Borrowed(env!("VERGEN_CARGO_TARGET_TRIPLE")),
        vergen_cargo_features: Cow::Borrowed(env!("VERGEN_CARGO_FEATURES")),
        short_version: Cow::Borrowed(env!("BERA_RETH_SHORT_VERSION")),
        long_version: Cow::Owned(format!(
            "{}\n{}\n{}\n{}\n{}",
            env!("BERA_RETH_LONG_VERSION_0"),
            env!("BERA_RETH_LONG_VERSION_1"),
            env!("BERA_RETH_LONG_VERSION_2"),
            env!("BERA_RETH_LONG_VERSION_3"),
            env!("BERA_RETH_LONG_VERSION_4"),
        )),
        build_profile_name: Cow::Borrowed(env!("BERA_RETH_BUILD_PROFILE")),
        p2p_client_version: Cow::Borrowed(env!("BERA_RETH_P2P_CLIENT_VERSION")),
        extra_data: Cow::Owned(format!(
            "bera-reth/v{}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        )),
    });

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    let cli_components_builder = |spec: Arc<BerachainChainSpec>| {
        (
            BerachainEvmConfig::new_with_evm_factory(spec.clone(), BerachainEvmFactory::default()),
            BerachainBeaconConsensus::new(spec),
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
