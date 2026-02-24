use clap::Args;

const DEFAULT_POG_TIMEOUT_SECS: u64 = 120;
const DEFAULT_POG_REPUTATION_PENALTY: i32 = -25600;

#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Proof of Gossip")]
pub struct BerachainArgs {
    #[arg(long = "pog.private-key")]
    pub pog_private_key: Option<String>,

    #[arg(long = "pog.timeout", default_value_t = DEFAULT_POG_TIMEOUT_SECS)]
    pub pog_timeout: u64,

    #[arg(
        long = "pog.reputation-penalty",
        default_value_t = DEFAULT_POG_REPUTATION_PENALTY,
        allow_hyphen_values = true
    )]
    pub pog_reputation_penalty: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: BerachainArgs,
    }

    fn test_pog_key_hex() -> String {
        format!("0x{:064x}", 1u64)
    }

    #[test]
    fn test_defaults() {
        let cli = TestCli::parse_from(["test"]);
        assert_eq!(cli.args.pog_private_key, None);
        assert_eq!(cli.args.pog_timeout, DEFAULT_POG_TIMEOUT_SECS);
        assert_eq!(cli.args.pog_reputation_penalty, DEFAULT_POG_REPUTATION_PENALTY);
    }

    #[test]
    fn test_with_private_key() {
        let key = test_pog_key_hex();
        let cli = TestCli::parse_from(["test", "--pog.private-key", &key]);
        assert_eq!(cli.args.pog_private_key.as_deref(), Some(key.as_str()));
        assert_eq!(cli.args.pog_timeout, DEFAULT_POG_TIMEOUT_SECS);
    }

    #[test]
    fn test_with_all_flags() {
        let key = test_pog_key_hex();
        let cli = TestCli::parse_from([
            "test",
            "--pog.private-key",
            &key,
            "--pog.timeout",
            "300",
            "--pog.reputation-penalty",
            "-50000",
        ]);
        assert_eq!(cli.args.pog_private_key.as_deref(), Some(key.as_str()));
        assert_eq!(cli.args.pog_timeout, 300);
        assert_eq!(cli.args.pog_reputation_penalty, -50000);
    }

    #[test]
    fn test_with_explicit_reputation_penalty() {
        let cli = TestCli::parse_from(["test", "--pog.reputation-penalty", "-10000"]);
        assert_eq!(cli.args.pog_reputation_penalty, -10000);
    }

    #[test]
    fn test_feature_off_when_flag_absent() {
        let cli = TestCli::parse_from(["test", "--pog.timeout", "60"]);
        assert_eq!(cli.args.pog_private_key, None);
        assert_eq!(cli.args.pog_timeout, 60);
    }
}
