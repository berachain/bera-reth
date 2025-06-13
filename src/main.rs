#![allow(missing_docs)]
#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::chainspec::BerachainChainSpecParser;
use bera_reth::node::BerachainNode;
use bera_reth::node::cli::Cli;
use clap::Parser;
use reth::args::RessArgs;
use reth::ress::install_ress_subprotocol;
use reth_node_builder::NodeHandle;
use tracing::info;

fn main() {
    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    if let Err(err) =
        Cli::<BerachainChainSpecParser, RessArgs>::parse().run(async move |builder, ress_args| {
            info!(target: "reth::cli", "Launching node (rez v1)");
            let NodeHandle { node, node_exit_future } =
                builder.node(BerachainNode::default()).launch_with_debug_capabilities().await?;

            // Install ress subprotocol.
            if ress_args.enabled {
                install_ress_subprotocol(
                    ress_args,
                    node.provider,
                    node.evm_config,
                    node.network,
                    node.task_executor,
                    node.add_ons_handle.engine_events.new_listener(),
                )?;
            }

            node_exit_future.await
        })
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
