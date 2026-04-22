//! Metrics for the flashblock sequencer (producer side).
//!
//! Unlabeled metrics live in `FlashblockSequencerMetrics`. Labeled counters are
//! emitted via the `record_*` helpers so call sites stay symbolic.

use metrics::{Counter, Gauge, Histogram, counter};
use reth_metrics::Metrics;

#[derive(Clone, Metrics)]
#[metrics(scope = "flashblock_sequencer")]
pub(crate) struct FlashblockSequencerMetrics {
    /// Full duration of `build_flashblock_payload` in seconds.
    pub(crate) build_duration_seconds: Histogram,
    /// Observed delta between configured interval and actual emission gap, in seconds.
    /// Positive drift = emissions running behind schedule.
    pub(crate) interval_drift_seconds: Histogram,
    /// Transactions included in a single flashblock interval.
    pub(crate) transactions_per_flashblock: Histogram,
    /// Gas used in a single flashblock interval.
    pub(crate) gas_used_per_flashblock: Histogram,
    /// Size of serialized flashblock JSON payload in bytes.
    pub(crate) payload_bytes: Histogram,
    /// BLS signing duration in seconds.
    pub(crate) signing_duration_seconds: Histogram,
    /// Current WebSocket subscriber count (mirrors `WebSocketPublisher::subscriber_count`).
    pub(crate) ws_subscribers: Gauge,
    /// Total broadcast messages dropped by lagging WebSocket clients.
    pub(crate) ws_client_lagged_total: Counter,
}

pub(crate) fn record_build_exit(reason: &'static str) {
    counter!("flashblock_sequencer_build_exit_total", "reason" => reason).increment(1);
}

pub(crate) fn record_emitted(is_last: bool) {
    let is_last = if is_last { "true" } else { "false" };
    counter!("flashblock_sequencer_emitted_total", "is_last" => is_last).increment(1);
}

pub(crate) fn record_ws_connection(result: &'static str) {
    counter!("flashblock_sequencer_ws_connections_total", "result" => result).increment(1);
}

/// Recorded when `WebSocketPublisher::publish` fails (currently only on
/// serialization errors). Kept separate from `emitted_total` so the build path
/// can be tracked independently of the publish path.
pub(crate) fn record_publish_error(reason: &'static str) {
    counter!("flashblock_sequencer_publish_error_total", "reason" => reason).increment(1);
}
