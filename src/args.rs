use clap::Args;

#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Proof of Gossip")]
pub struct BerachainArgs {
    #[arg(long = "pog.private-key")]
    pub pog_private_key: Option<String>,

    #[arg(long = "pog.timeout", default_value_t = 120)]
    pub pog_timeout: u64,
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

    #[test]
    fn test_defaults() {
        let cli = TestCli::parse_from(["test"]);
        assert_eq!(cli.args.pog_private_key, None);
        assert_eq!(cli.args.pog_timeout, 120);
    }

    #[test]
    fn test_with_private_key() {
        let cli = TestCli::parse_from([
            "test",
            "--pog.private-key",
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        ]);
        assert_eq!(
            cli.args.pog_private_key,
            Some("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string())
        );
        assert_eq!(cli.args.pog_timeout, 120);
    }

    #[test]
    fn test_with_both_flags() {
        let cli = TestCli::parse_from([
            "test",
            "--pog.private-key",
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "--pog.timeout",
            "300",
        ]);
        assert_eq!(
            cli.args.pog_private_key,
            Some("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string())
        );
        assert_eq!(cli.args.pog_timeout, 300);
    }

    #[test]
    fn test_feature_off_when_flag_absent() {
        let cli = TestCli::parse_from(["test", "--pog.timeout", "60"]);
        assert_eq!(cli.args.pog_private_key, None);
        assert_eq!(cli.args.pog_timeout, 60);
    }
}
