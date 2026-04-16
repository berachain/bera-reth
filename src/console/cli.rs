use clap::Args;
use reth_cli_runner::CliRunner;

/// JSON-RPC console (IPC, HTTP, or WebSocket).
#[derive(Debug, Clone, Args)]
pub struct ConsoleCommand {
    /// IPC path, or `http(s)://…`, or `ws(s)://…`. If omitted, uses the platform default
    /// datadir with `reth.ipc`.
    #[arg(value_name = "ENDPOINT")]
    pub endpoint: Option<String>,

    /// Run a single command and print raw JSON (implies raw output; no prompts).
    #[arg(long = "exec")]
    pub exec: Option<String>,

    /// In REPL mode, print raw JSON instead of tables and annotations.
    #[arg(long)]
    pub raw: bool,
}

impl ConsoleCommand {
    pub fn run(self, runner: CliRunner) -> eyre::Result<()> {
        runner.block_on(super::run::run_console(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    #[command(name = "bera-reth")]
    struct Top {
        #[command(subcommand)]
        sub: Sub,
    }

    #[derive(clap::Subcommand)]
    enum Sub {
        Console(ConsoleCommand),
    }

    #[test]
    fn parses_exec_and_raw() {
        let Top { sub: Sub::Console(c) } =
            Top::try_parse_from(["bera-reth", "console", "--exec", "eth.blockNumber", "--raw"])
                .unwrap();
        assert_eq!(c.exec.as_deref(), Some("eth.blockNumber"));
        assert!(c.raw);
        assert!(c.endpoint.is_none());
    }

    #[test]
    fn parses_positional_endpoint() {
        let Top { sub: Sub::Console(c) } =
            Top::try_parse_from(["bera-reth", "console", "/tmp/reth.ipc"]).unwrap();
        assert_eq!(c.endpoint.as_deref(), Some("/tmp/reth.ipc"));
        assert!(!c.raw);
    }
}
