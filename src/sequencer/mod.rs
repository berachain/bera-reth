//! Berachain sequencer module for flashblock production.
//!
//! This module provides the sequencer functionality that produces flashblocks
//! at regular intervals (~200ms) and publishes them via WebSocket.

mod builder;
mod publisher;
pub mod signing;

pub use builder::{FlashblockPayloadBuilder, FlashblockPayloadServiceBuilder};
pub use publisher::WebSocketPublisher;
pub use signing::{BlsPublicKeyBytes, BlsSignature, FlashblockSigner, SigningError};
pub use tokio_util::sync::CancellationToken;

use std::{net::SocketAddr, sync::Arc, time::Duration};

/// Configuration for the sequencer.
#[derive(Debug, Clone)]
pub struct SequencerConfig {
    /// Flashblock emission interval.
    pub interval: Duration,
    /// WebSocket address for flashblock publishing.
    pub ws_addr: SocketAddr,
    /// BLS signer for flashblock signing.
    pub signer: Arc<FlashblockSigner>,
}

impl SequencerConfig {
    /// Create a new sequencer config with the required signer.
    pub fn new(interval_ms: u64, ws_addr: SocketAddr, signer: FlashblockSigner) -> Self {
        Self { interval: Duration::from_millis(interval_ms), ws_addr, signer: Arc::new(signer) }
    }
}
