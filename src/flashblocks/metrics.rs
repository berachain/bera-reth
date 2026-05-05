//! Metrics for the flashblock RPC consumer.
//!
//! Unlabeled metrics live in `FlashBlockServiceMetrics`. Labeled counters are
//! emitted via the `record_*` helpers so call sites stay symbolic.

use metrics::{Counter, Gauge, Histogram, counter};
use reth_metrics::Metrics;

#[derive(Metrics)]
#[metrics(scope = "flashblock_service")]
pub(crate) struct FlashBlockServiceMetrics {
    /// Number of flashblocks in the last completed sequence.
    pub(crate) last_flashblock_length: Histogram,
    /// Duration of the last flashblock execution in seconds.
    pub(crate) execution_duration: Histogram,
    /// Current block height being processed.
    pub(crate) current_block_height: Gauge,
    /// Current flashblock index within the sequence.
    pub(crate) current_index: Gauge,
    /// 1 = receiving flashblocks from upstream sequencer, 0 = stream error/ended.
    pub(crate) upstream_connected: Gauge,
    /// Number of upstream WebSocket reconnect attempts (incremented on stream error backoff).
    pub(crate) ws_reconnects_total: Counter,
}

pub(crate) fn record_insert(result: &'static str) {
    counter!("flashblock_service_insert_total", "result" => result).increment(1);
}

pub(crate) fn record_build_skip(reason: &'static str) {
    counter!("flashblock_service_build_skip_total", "reason" => reason).increment(1);
}
