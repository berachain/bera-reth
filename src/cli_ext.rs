//! Berachain-specific CLI extensions for `reth node`.

use bera_reth::pog::{
    DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT, DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES,
    DEFAULT_SEALED_FACT_RETENTION_HOURS,
};
use clap::Args;

#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "Berachain")]
pub struct BerachainExt {
    /// Enable Proof-of-Gossip: `beradmin_*` RPC, background probe watcher, and related state.
    ///
    /// Default is **off** so EL behaves like a standard node for sync and Engine API; pass this
    /// when running with sentinel / sidecar PoG workflows.
    #[arg(long = "bera.pog", default_value_t = false)]
    pub pog: bool,

    /// Hours of sealed-tx-fact retention in the durable PoG SQLite store.
    ///
    /// Retention is applied inline with every seal-flush transaction. Range: 1..=8760.
    #[arg(
        long = "sealed-fact-retention-hours",
        default_value_t = DEFAULT_SEALED_FACT_RETENTION_HOURS,
        value_parser = clap::value_parser!(u64).range(1_u64..=8760_u64),
    )]
    pub sealed_fact_retention_hours: u64,

    /// Hard cap on the in-memory `InflightTransactions` map.
    ///
    /// When the cap is reached, an inline TTL sweep runs; if still at cap, new first-hear
    /// inserts are refused and `reth_pog_inflight_tx_cap_rejections_total` is incremented.
    /// Range: 1000..=10_000_000.
    #[arg(
        long = "sealed-fact-max-inflight-entries",
        default_value_t = DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES as u64,
        value_parser = clap::value_parser!(u64).range(1_000_u64..=10_000_000_u64),
    )]
    pub sealed_fact_max_inflight_entries: u64,

    /// Server-side clamp on `beradmin_exportSealedTxFacts` `limit`. Values above this cap
    /// are rejected with a clear error (not silently clamped). Range: 10..=100_000.
    #[arg(
        long = "sealed-fact-export-max-limit",
        default_value_t = DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT,
        value_parser = clap::value_parser!(u32).range(10_i64..=100_000_i64),
    )]
    pub sealed_fact_export_max_limit: u32,
}

impl Default for BerachainExt {
    fn default() -> Self {
        Self {
            pog: false,
            sealed_fact_retention_hours: DEFAULT_SEALED_FACT_RETENTION_HOURS,
            sealed_fact_max_inflight_entries: DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES as u64,
            sealed_fact_export_max_limit: DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT,
        }
    }
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
    fn defaults_match_constants() {
        let p = Probe::try_parse_from(["bera-reth"]).unwrap();
        assert_eq!(p.ext.sealed_fact_retention_hours, DEFAULT_SEALED_FACT_RETENTION_HOURS);
        assert_eq!(
            p.ext.sealed_fact_max_inflight_entries,
            DEFAULT_SEALED_FACT_MAX_INFLIGHT_ENTRIES as u64
        );
        assert_eq!(p.ext.sealed_fact_export_max_limit, DEFAULT_SEALED_FACT_EXPORT_MAX_LIMIT);
    }

    #[test]
    fn retention_zero_is_rejected_at_parse() {
        let err =
            Probe::try_parse_from(["bera-reth", "--sealed-fact-retention-hours", "0"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn retention_above_year_is_rejected_at_parse() {
        let err = Probe::try_parse_from(["bera-reth", "--sealed-fact-retention-hours", "8761"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn inflight_cap_below_floor_is_rejected() {
        let err = Probe::try_parse_from(["bera-reth", "--sealed-fact-max-inflight-entries", "999"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn inflight_cap_above_ceiling_is_rejected() {
        let err =
            Probe::try_parse_from(["bera-reth", "--sealed-fact-max-inflight-entries", "10000001"])
                .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn export_max_limit_below_floor_is_rejected() {
        let err = Probe::try_parse_from(["bera-reth", "--sealed-fact-export-max-limit", "9"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn export_max_limit_above_ceiling_is_rejected() {
        let err = Probe::try_parse_from(["bera-reth", "--sealed-fact-export-max-limit", "100001"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn ranges_parse_at_boundaries() {
        let p = Probe::try_parse_from([
            "bera-reth",
            "--sealed-fact-retention-hours",
            "1",
            "--sealed-fact-max-inflight-entries",
            "1000",
            "--sealed-fact-export-max-limit",
            "10",
        ])
        .unwrap();
        assert_eq!(p.ext.sealed_fact_retention_hours, 1);
        assert_eq!(p.ext.sealed_fact_max_inflight_entries, 1_000);
        assert_eq!(p.ext.sealed_fact_export_max_limit, 10);
    }
}
