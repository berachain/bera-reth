//! Berachain-specific CLI subcommands (extension to reth Ethereum CLI).

use clap::Subcommand;
use reth_cli_runner::CliRunner;
use reth_ethereum_cli::app::ExtendedCommand;

#[derive(Debug, Subcommand)]
pub enum BerachainSubcommands {
    /// JSON-RPC console over IPC, HTTP, or WebSocket.
    Console(crate::console::ConsoleCommand),
}

impl ExtendedCommand for BerachainSubcommands {
    fn execute(self, runner: CliRunner) -> eyre::Result<()> {
        match self {
            Self::Console(cmd) => cmd.run(runner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chainspec::BerachainChainSpecParser;
    use clap::Parser;
    use reth_cli_commands::node::NoArgs;
    use reth_ethereum_cli::interface::{Cli, Commands};
    use reth_rpc_server_types::DefaultRpcModuleValidator;

    #[test]
    fn parses_console_subcommand() {
        let err = Cli::<
            BerachainChainSpecParser,
            NoArgs,
            DefaultRpcModuleValidator,
            BerachainSubcommands,
        >::try_parse_from(["bera-reth", "console", "--help"])
        .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn ext_console_variant_reachable() {
        let cli = Cli::<
            BerachainChainSpecParser,
            NoArgs,
            DefaultRpcModuleValidator,
            BerachainSubcommands,
        >::try_parse_from(["bera-reth", "console", "--exec", "eth_blockNumber"])
        .unwrap();
        match cli.command {
            Commands::Ext(BerachainSubcommands::Console(ref c)) => {
                assert_eq!(c.exec.as_deref(), Some("eth_blockNumber"));
            }
            _ => panic!("expected console ext"),
        }
    }
}
