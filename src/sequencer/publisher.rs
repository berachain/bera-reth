//! WebSocket publisher for broadcasting flashblocks to subscribers.

use crate::flashblocks::BerachainFlashblockPayload;
use futures_util::{SinkExt, StreamExt};
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::sequencer::{FlashblockSequencerMetrics, record_ws_connection};

/// Capacity for the flashblock broadcast channel.
const FLASHBLOCK_CHANNEL_CAPACITY: usize = 20;

/// Maximum concurrent WebSocket connections.
const MAX_CONNECTIONS: usize = 256;

/// Timeout for WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// WebSocket publisher that broadcasts flashblocks to all connected clients.
#[derive(Debug)]
pub struct WebSocketPublisher {
    sender: broadcast::Sender<String>,
    address: SocketAddr,
    subscriber_count: Arc<AtomicUsize>,
    metrics: FlashblockSequencerMetrics,
}

impl WebSocketPublisher {
    /// Create a new WebSocket publisher.
    pub fn new(address: SocketAddr) -> Self {
        let (sender, _) = broadcast::channel(FLASHBLOCK_CHANNEL_CAPACITY);
        Self {
            sender,
            address,
            subscriber_count: Arc::new(AtomicUsize::new(0)),
            metrics: FlashblockSequencerMetrics::default(),
        }
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::Relaxed)
    }

    /// Get a receiver for flashblock messages.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// Publish a flashblock to all subscribers.
    ///
    /// Returns `(subscribers, bytes)` where `subscribers` is the receiver count and
    /// `bytes` is the length of the serialized JSON payload. Exposing the size lets
    /// callers record the `payload_bytes` metric without re-serializing.
    pub fn publish(&self, payload: &BerachainFlashblockPayload) -> io::Result<(usize, usize)> {
        let json = serde_json::to_string(payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let bytes = json.len();

        match self.sender.send(json) {
            Ok(count) => {
                debug!(
                    target: "sequencer::publisher",
                    payload_id = %payload.payload_id,
                    index = payload.index,
                    subscribers = count,
                    "published flashblock"
                );
                Ok((count, bytes))
            }
            Err(_) => {
                // No subscribers - this is fine
                Ok((0, bytes))
            }
        }
    }

    /// Bind the WebSocket server to its configured address.
    pub async fn bind(&self) -> eyre::Result<TcpListener> {
        let listener = TcpListener::bind(self.address).await?;
        info!(
            target: "sequencer::publisher",
            address = %self.address,
            "flashblock WebSocket server started"
        );
        Ok(listener)
    }

    /// Run the WebSocket server until cancelled.
    pub async fn run(&self, listener: TcpListener, cancel: CancellationToken) -> eyre::Result<()> {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(target: "sequencer::publisher", "shutting down WebSocket server");
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            if self.subscriber_count.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                                record_ws_connection("rejected_limit");
                                warn!(target: "sequencer::publisher", %addr, "connection limit reached");
                                continue;
                            }
                            let rx = self.sender.subscribe();
                            let count = self.subscriber_count.clone();
                            let conn_cancel = cancel.clone();
                            let metrics = self.metrics.clone();
                            tokio::spawn(async move {
                                handle_connection(stream, addr, rx, count, conn_cancel, metrics).await;
                            });
                        }
                        Err(e) => {
                            record_ws_connection("accept_error");
                            error!(target: "sequencer::publisher", error = %e, "failed to accept connection");
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    mut rx: broadcast::Receiver<String>,
    subscriber_count: Arc<AtomicUsize>,
    cancel: CancellationToken,
    metrics: FlashblockSequencerMetrics,
) {
    let ws_stream = match tokio::time::timeout(HANDSHAKE_TIMEOUT, accept_async(stream)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            record_ws_connection("handshake_error");
            warn!(target: "sequencer::publisher", %addr, error = %e, "handshake failed");
            return;
        }
        Err(_) => {
            record_ws_connection("handshake_error");
            warn!(target: "sequencer::publisher", %addr, "handshake timeout");
            return;
        }
    };
    record_ws_connection("accepted");
    let (mut write, mut read) = ws_stream.split();

    subscriber_count.fetch_add(1, Ordering::Relaxed);
    info!(
        target: "sequencer::publisher",
        %addr,
        count = subscriber_count.load(Ordering::Relaxed),
        "client connected"
    );
    metrics.ws_subscribers.set(subscriber_count.load(Ordering::Relaxed) as f64);

    let mut session_error: Option<String> = None;

    // Handle incoming messages and broadcast outgoing flashblocks
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                // Send close frame before shutting down
                let _ = write.send(Message::Close(None)).await;
                break;
            }
            // Forward flashblocks to the client
            result = rx.recv() => {
                match result {
                    Ok(json) => {
                        if let Err(e) = write.send(Message::Text(json.into())).await {
                            session_error = Some(format!("send failed: {e}"));
                            debug!(target: "sequencer::publisher", %addr, error = %e, "failed to send message");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        metrics.ws_client_lagged_total.increment(n);
                        warn!(target: "sequencer::publisher", %addr, skipped = n, "client lagging");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            // Handle client messages (ping/pong, close)
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            session_error = Some(format!("pong failed: {e}"));
                            debug!(target: "sequencer::publisher", %addr, error = %e, "failed to send pong");
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    Some(Err(e)) => {
                        session_error = Some(format!("ws error: {e}"));
                        debug!(target: "sequencer::publisher", %addr, error = %e, "websocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(err) = session_error {
        record_ws_connection("closed_error");
        warn!(target: "sequencer::publisher", %addr, %err, "connection error");
    }

    subscriber_count.fetch_sub(1, Ordering::Relaxed);
    info!(
        target: "sequencer::publisher",
        %addr,
        count = subscriber_count.load(Ordering::Relaxed),
        "client disconnected"
    );
    metrics.ws_subscribers.set(subscriber_count.load(Ordering::Relaxed) as f64);
}
