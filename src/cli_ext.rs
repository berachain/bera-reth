//! Berachain-specific CLI extensions for `reth node`.

use clap::Args;

#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Berachain")]
pub struct BerachainExt {
    /// Enable Proof-of-Gossip: `beradmin_*` RPC, background probe watcher, and related state.
    ///
    /// Default is **off** so EL behaves like a standard node for sync and Engine API; pass this
    /// when running with sentinel / sidecar PoG workflows.
    #[arg(long = "bera.pog", default_value_t = false)]
    pub pog: bool,
}
