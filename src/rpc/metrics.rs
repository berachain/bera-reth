//! Metrics for the preconf RPC layer.

use metrics::counter;

/// Outcome of a `pending_flashblock()` lookup. Recorded once per call.
pub(crate) fn record_state_lookup(result: &'static str) {
    counter!("preconf_rpc_state_lookup_total", "result" => result).increment(1);
}

/// Recorded when a pending flashblock was found but its parent block is not yet
/// available in the historical state DB, forcing a fallback to canonical state.
/// Distinct from `state_lookup_total` to avoid double-counting on a single RPC.
pub(crate) fn record_state_fallback(reason: &'static str) {
    counter!("preconf_rpc_state_fallback_total", "reason" => reason).increment(1);
}
