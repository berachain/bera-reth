use crate::{
    chainspec::{BerachainChainSpec, BerachainChainSpecParser},
    node::BerachainNode,
};
use clap::Parser;
use reth::{
    CliRunner,
    args::LogArgs,
    beacon_consensus::EthBeaconConsensus,
    network::EthNetworkPrimitives,
    prometheus_exporter::install_prometheus_recorder,
    version::{LONG_VERSION, SHORT_VERSION},
};
use reth_chainspec::EthChainSpec;
use reth_cli::chainspec::ChainSpecParser;
use reth_cli_commands::{launcher::FnLauncher, node::NoArgs};
use reth_db::DatabaseEnv;
use reth_ethereum_cli::interface::Commands;
use reth_evm::EthEvmFactory;
use reth_evm_ethereum::EthEvmConfig;
use reth_node_builder::{NodeBuilder, WithLaunchContext};
use reth_tracing::FileWorkerGuard;
use std::{fmt, future::Future, sync::Arc};
use tracing::info;

/// The main bera-reth cli interface.
///
/// This is the entrypoint to the executable.
#[derive(Debug, Parser)]
#[command(author, version = SHORT_VERSION, long_version = LONG_VERSION, about = "Reth", long_about = None)]
pub struct Cli<C: ChainSpecParser = BerachainChainSpecParser, Ext: clap::Args + fmt::Debug = NoArgs>
{
    /// The command to run
    #[command(subcommand)]
    pub command: Commands<C, Ext>,

    /// The logging configuration for the CLI.
    #[command(flatten)]
    pub logs: LogArgs,
}

impl<C, Ext> Cli<C, Ext>
where
    C: ChainSpecParser<ChainSpec = BerachainChainSpec>,
    Ext: clap::Args + fmt::Debug,
{
    /// Execute the configured cli command.
    ///
    /// This accepts a closure that is used to launch the node via the
    /// [`NodeCommand`](reth_cli_commands::node::NodeCommand).
    pub fn run<L, Fut>(self, launcher: L) -> eyre::Result<()>
    where
        L: FnOnce(WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, C::ChainSpec>>, Ext) -> Fut,
        Fut: Future<Output = eyre::Result<()>>,
    {
        self.with_runner(CliRunner::try_default_runtime()?, launcher)
    }

    pub fn with_runner<L, Fut>(mut self, runner: CliRunner, launcher: L) -> eyre::Result<()>
    where
        L: FnOnce(WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, C::ChainSpec>>, Ext) -> Fut,
        Fut: Future<Output = eyre::Result<()>>,
    {
        // Add network name if available to the logs dir
        if let Some(chain_spec) = self.command.chain_spec() {
            self.logs.log_file_directory =
                self.logs.log_file_directory.join(chain_spec.chain().to_string());
        }
        let _guard = self.init_tracing()?;
        info!(target: "reth::cli", "Initialized tracing, debug log directory: {}", self.logs.log_file_directory);

        // Install the prometheus recorder to be sure to record all metrics
        let _ = install_prometheus_recorder();

        let components = |spec: Arc<C::ChainSpec>| {
            (
                EthEvmConfig::new_with_evm_factory(spec.clone(), EthEvmFactory::default()),
                EthBeaconConsensus::new(spec),
            )
        };
        match self.command {
            Commands::Node(command) => runner.run_command_until_exit(|ctx| {
                command.execute(ctx, FnLauncher::new::<C, Ext>(launcher))
            }),
            Commands::Init(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode>())
            }
            Commands::InitState(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode>())
            }
            Commands::Import(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode, _, _>(components))
            }
            Commands::ImportEra(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode>())
            }
            Commands::DumpGenesis(command) => runner.run_blocking_until_ctrl_c(command.execute()),
            Commands::Db(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode>())
            }
            Commands::Download(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<BerachainNode>())
            }
            Commands::Stage(command) => runner.run_command_until_exit(|ctx| {
                command.execute::<BerachainNode, _, _, EthNetworkPrimitives>(ctx, components)
            }),
            Commands::P2P(command) => {
                runner.run_until_ctrl_c(command.execute::<EthNetworkPrimitives>())
            }
            Commands::Config(command) => runner.run_until_ctrl_c(command.execute()),
            Commands::Debug(_command) => {
                // Debug commands require special handling and are typically used for
                // development and troubleshooting. Currently not implemented for Berachain.
                runner.run_blocking_until_ctrl_c(async {
                    tracing::warn!("Debug commands are not yet implemented for Berachain nodes");
                    Err(eyre::eyre!("Debug commands not available for Berachain nodes. This feature is planned for future releases."))
                })
            }
            Commands::Recover(command) => {
                runner.run_command_until_exit(|ctx| command.execute::<BerachainNode>(ctx))
            }
            Commands::Prune(command) => runner.run_until_ctrl_c(command.execute::<BerachainNode>()),
        }
    }
    /// Initializes tracing with the configured options.
    ///
    /// If file logging is enabled, this function returns a guard that must be kept alive to ensure
    /// that all logs are flushed to disk.
    pub fn init_tracing(&self) -> eyre::Result<Option<FileWorkerGuard>> {
        let guard = self.logs.init_tracing()?;
        Ok(guard)
    }
}
