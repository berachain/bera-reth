//! Berachain-specific CLI extensions for `reth node`.

use clap::Args;

#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Berachain")]
pub struct BerachainExt {
    /// Enable Proof-of-Gossip: `pog_*` IPC methods, send watcher, and `/24` metrics.
    #[arg(long = "bera.pog", default_value_t = false)]
    pub pog: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct Probe {
        #[command(flatten)]
        ext: BerachainExt,
    }

    #[test]
    fn pog_defaults_off() {
        let p = Probe::try_parse_from(["bera-reth"]).unwrap();
        assert!(!p.ext.pog);
    }

    #[test]
    fn pog_flag_parses() {
        let p = Probe::try_parse_from(["bera-reth", "--bera.pog"]).unwrap();
        assert!(p.ext.pog);
    }
}
