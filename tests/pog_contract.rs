//! Source-level contract for sentry-free PoG.

use std::fs;

#[test]
fn pog_merged_on_ipc_only() {
    let src = fs::read_to_string("src/rpc/mod.rs").expect("rpc/mod.rs");
    assert!(src.contains("merge_ipc"), "pog methods must be installed with merge_ipc");
    let pog_region = src.split("PogApiServer").nth(1).unwrap_or(&src);
    assert!(
        !pog_region.contains("merge_configured"),
        "pog must not use merge_configured (all transports)"
    );
}

#[test]
fn no_sentry_surfaces() {
    for path in ["src/rpc/pog/mod.rs", "src/pog/mod.rs", "src/cli_ext.rs", "src/main.rs"] {
        let src = fs::read_to_string(path).unwrap_or_default();
        for forbidden in [
            "prepareCanary",
            "submitCanary",
            "exportSealedTxFacts",
            "probePeer",
            "pog-signer-key",
            "proof_of_gossip.db",
            "seenTxCacheSize",
        ] {
            assert!(!src.contains(forbidden), "{path} must not contain {forbidden}");
        }
    }
}

#[test]
fn prometheus_metric_names_have_no_peer_id_label_in_source() {
    let src = fs::read_to_string("src/pog/mod.rs").expect("pog/mod.rs");
    assert!(src.contains("pog_peers"));
    assert!(src.contains("pog_sends_total"));
    assert!(src.contains("pog_sends_inflight"));
    assert!(!src.contains("\"peer_id\""));
    assert!(!src.contains("\"peerId\""));
    assert!(!src.contains("pog_peer_sessions_total"));
    assert!(!src.contains("pog_seen_tx_cache"));
    assert!(!src.contains("pog_send_latency"));
}
