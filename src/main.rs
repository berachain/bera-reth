//! Bera-Reth main entry point

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

use bera_reth::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    consensus::BerachainBeaconConsensus,
    evm::BerachainEvmFactory,
    node::{BerachainNode, evm::config::BerachainEvmConfig},
    sequencer::{FlashblockPayloadServiceBuilder, FlashblockSigner, SequencerConfig, WebSocketPublisher},
    version::init_bera_version,
};
use clap::Parser;
use reth::CliRunner;
use reth_chainspec::EthChainSpec;
use reth_ethereum_cli::Cli;
use reth_node_builder::{components::BasicPayloadServiceBuilder, Node, NodeHandle};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Sequencer-specific CLI arguments
#[derive(Debug, Clone, clap::Args)]
pub struct SequencerArgs {
    /// Enable sequencer mode for flashblock production
    #[arg(long, default_value = "false")]
    pub sequencer_enabled: bool,

    /// Flashblock emission interval in milliseconds
    #[arg(long, default_value = "200")]
    pub flashblock_interval_ms: u64,

    /// WebSocket address for flashblock publishing
    #[arg(long, default_value = "0.0.0.0:8548")]
    pub flashblock_ws_addr: SocketAddr,

    /// Path to BLS secret key file for signing flashblocks (hex-encoded 32-byte key)
    #[arg(long)]
    pub flashblock_signing_key: Option<PathBuf>,
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

    if let Err(err) = Cli::<BerachainChainSpecParser, SequencerArgs>::parse()
        .with_runner_and_components::<BerachainNode>(
            CliRunner::try_default_runtime().expect("Failed to create default runtime"),
            cli_components_builder,
            async move |builder, extra_args| {
                if extra_args.sequencer_enabled {
                    // Signing key is required in sequencer mode
                    let chain_id = builder.config().chain.chain().id();
                    let key_path = extra_args.flashblock_signing_key.ok_or_else(|| {
                        eyre::eyre!("--flashblock-signing-key is required in sequencer mode")
                    })?;
                    let signer = FlashblockSigner::from_file(&key_path, chain_id)
                        .map_err(|e| eyre::eyre!("failed to load signing key from {:?}: {}", key_path, e))?;

                    info!(
                        target: "reth::cli",
                        interval_ms = extra_args.flashblock_interval_ms,
                        ws_addr = %extra_args.flashblock_ws_addr,
                        pubkey = %hex::encode(signer.public_key_bytes()),
                        "Launching Berachain node in SEQUENCER mode"
                    );

                    let config = SequencerConfig::new(
                        extra_args.flashblock_interval_ms,
                        extra_args.flashblock_ws_addr,
                        signer,
                    );

                    let publisher = Arc::new(WebSocketPublisher::new(config.ws_addr));
                    let ws_cancel = CancellationToken::new();

                    // Spawn WebSocket server
                    let ws_publisher = publisher.clone();
                    let ws_cancel_token = ws_cancel.clone();
                    tokio::spawn(async move {
                        if let Err(e) = ws_publisher.run(ws_cancel_token).await {
                            tracing::error!(target: "sequencer::publisher", error = %e, "WebSocket server error");
                        }
                    });

                    // Build node with flashblock payload builder
                    let berachain_node = BerachainNode::default();
                    let flashblock_builder = FlashblockPayloadServiceBuilder::new(config, publisher);

                    let NodeHandle { node: _node, node_exit_future } = builder
                        .with_types::<BerachainNode>()
                        .with_components(
                            berachain_node
                                .components_builder()
                                .payload(BasicPayloadServiceBuilder::new(flashblock_builder)),
                        )
                        .with_add_ons(berachain_node.add_ons())
                        .launch_with_debug_capabilities()
                        .await?;

                    let result = node_exit_future.await;
                    ws_cancel.cancel();
                    result
                } else {
                    info!(target: "reth::cli", "Launching Berachain node");
                    let NodeHandle { node: _node, node_exit_future } =
                        builder.node(BerachainNode::default()).launch_with_debug_capabilities().await?;

                    node_exit_future.await
                }
            },
        )
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
