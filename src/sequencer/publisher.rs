//! WebSocket publisher for broadcasting flashblocks to subscribers.

use crate::flashblocks::BerachainFlashblockPayload;
use futures_util::{SinkExt, StreamExt};
use std::{
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Capacity for the flashblock broadcast channel.
/// At ~200ms intervals, 64 messages allows ~12.8 seconds of buffering for slow clients.
const FLASHBLOCK_CHANNEL_CAPACITY: usize = 64;

/// WebSocket publisher that broadcasts flashblocks to all connected clients.
#[derive(Debug)]
pub struct WebSocketPublisher {
    sender: broadcast::Sender<String>,
    address: SocketAddr,
    subscriber_count: Arc<AtomicUsize>,
}

impl WebSocketPublisher {
    /// Create a new WebSocket publisher.
    pub fn new(address: SocketAddr) -> Self {
        let (sender, _) = broadcast::channel(FLASHBLOCK_CHANNEL_CAPACITY);
        Self { sender, address, subscriber_count: Arc::new(AtomicUsize::new(0)) }
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
    pub fn publish(&self, payload: &BerachainFlashblockPayload) -> io::Result<usize> {
        let json = serde_json::to_string(payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        match self.sender.send(json) {
            Ok(count) => {
                debug!(
                    target: "sequencer::publisher",
                    payload_id = %payload.payload_id,
                    index = payload.index,
                    subscribers = count,
                    "published flashblock"
                );
                Ok(count)
            }
            Err(_) => {
                // No subscribers - this is fine
                Ok(0)
            }
        }
    }

    /// Run the WebSocket server until cancelled.
    pub async fn run(&self, cancel: CancellationToken) -> eyre::Result<()> {
        let listener = TcpListener::bind(self.address).await?;
        info!(
            target: "sequencer::publisher",
            address = %self.address,
            "flashblock WebSocket server started"
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(target: "sequencer::publisher", "shutting down WebSocket server");
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let rx = self.sender.subscribe();
                            let count = self.subscriber_count.clone();
                            let conn_cancel = cancel.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, addr, rx, count, conn_cancel).await {
                                    warn!(target: "sequencer::publisher", %addr, error = %e, "connection error");
                                }
                            });
                        }
                        Err(e) => {
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
) -> eyre::Result<()> {
    let ws_stream = accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    subscriber_count.fetch_add(1, Ordering::Relaxed);
    info!(
        target: "sequencer::publisher",
        %addr,
        count = subscriber_count.load(Ordering::Relaxed),
        "client connected"
    );

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
                            debug!(target: "sequencer::publisher", %addr, error = %e, "failed to send message");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
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
                            debug!(target: "sequencer::publisher", %addr, error = %e, "failed to send pong");
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    Some(Err(e)) => {
                        debug!(target: "sequencer::publisher", %addr, error = %e, "websocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    subscriber_count.fetch_sub(1, Ordering::Relaxed);
    info!(
        target: "sequencer::publisher",
        %addr,
        count = subscriber_count.load(Ordering::Relaxed),
        "client disconnected"
    );

    Ok(())
}
