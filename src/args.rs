use clap::Args;
use std::path::PathBuf;

const DEFAULT_POG_TIMEOUT_SECS: u64 = 120;
const DEFAULT_POG_REPUTATION_PENALTY: i32 = -25600;

#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Proof of Gossip")]
pub struct BerachainArgs {
    #[arg(long = "pog.private-key-file")]
    pub pog_private_key_file: Option<PathBuf>,

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
    use clap::error::ErrorKind;
    use clap::Parser;
    use std::path::PathBuf;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: BerachainArgs,
    }

    #[test]
    fn test_defaults() {
        let cli = TestCli::parse_from(["test"]);
        assert_eq!(cli.args.pog_private_key_file, None);
        assert_eq!(cli.args.pog_timeout, DEFAULT_POG_TIMEOUT_SECS);
        assert_eq!(cli.args.pog_reputation_penalty, DEFAULT_POG_REPUTATION_PENALTY);
    }

    #[test]
    fn test_with_private_key_file() {
        let key_file = "/tmp/pog.key";
        let cli = TestCli::parse_from(["test", "--pog.private-key-file", key_file]);
        assert_eq!(
            cli.args.pog_private_key_file.as_deref(),
            Some(PathBuf::from(key_file).as_path())
        );
        assert_eq!(cli.args.pog_timeout, DEFAULT_POG_TIMEOUT_SECS);
    }

    #[test]
    fn test_with_all_flags() {
        let key_file = "/tmp/pog.key";
        let cli = TestCli::parse_from([
            "test",
            "--pog.private-key-file",
            key_file,
            "--pog.timeout",
            "300",
            "--pog.reputation-penalty",
            "-50000",
        ]);
        assert_eq!(
            cli.args.pog_private_key_file.as_deref(),
            Some(PathBuf::from(key_file).as_path())
        );
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
        assert_eq!(cli.args.pog_private_key_file, None);
        assert_eq!(cli.args.pog_timeout, 60);
    }

    #[test]
    fn test_rejects_old_private_key_flag() {
        let key = format!("0x{:064x}", 1u64);
        let parsed = TestCli::try_parse_from(["test", "--pog.private-key", &key]);
        assert!(parsed.is_err());
        let err = parsed.err().expect("old flag should be rejected");
        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    }
}
