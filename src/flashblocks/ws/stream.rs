use crate::flashblocks::ws::FlashBlockDecoder;
use futures_util::{
    FutureExt, Sink, Stream, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::{
    fmt::{Debug, Formatter},
    future::Future,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Bytes, Error, Message, protocol::CloseFrame},
};
use tracing::debug;
use url::Url;

pub struct WsFlashBlockStream<Stream, Sink, Connector, F> {
    ws_url: Url,
    state: State,
    connector: Connector,
    decoder: Box<dyn FlashBlockDecoder<F>>,
    connect: ConnectFuture<Sink, Stream>,
    stream: Option<Stream>,
    sink: Option<Sink>,
}

impl<F> WsFlashBlockStream<WsStream, WsSink, WsConnector, F>
where
    F: serde::de::DeserializeOwned,
{
    pub fn new(ws_url: Url) -> Self {
        Self {
            ws_url,
            state: State::default(),
            connector: WsConnector,
            decoder: Box::new(()),
            connect: Box::pin(async move { Err(Error::ConnectionClosed)? }),
            stream: None,
            sink: None,
        }
    }
}

impl<F> WsFlashBlockStream<WsStream, WsSink, WsConnector, F> {
    pub fn with_decoder(self, decoder: Box<dyn FlashBlockDecoder<F>>) -> Self {
        Self { decoder, ..self }
    }
}

impl<Stream, S, C, F> WsFlashBlockStream<Stream, S, C, F>
where
    F: serde::de::DeserializeOwned,
{
    pub fn with_connector(ws_url: Url, connector: C) -> Self {
        Self {
            ws_url,
            state: State::default(),
            decoder: Box::new(()),
            connector,
            connect: Box::pin(async move { Err(Error::ConnectionClosed)? }),
            stream: None,
            sink: None,
        }
    }
}

impl<Str, S, C, F> Stream for WsFlashBlockStream<Str, S, C, F>
where
    Str: Stream<Item = Result<Message, Error>> + Unpin,
    S: Sink<Message> + Send + Unpin,
    C: WsConnect<Stream = Str, Sink = S> + Clone + Send + 'static + Unpin,
    F: 'static,
{
    type Item = eyre::Result<F>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        'start: loop {
            if this.state == State::Initial {
                this.connect();
            }

            if this.state == State::Connect {
                match ready!(this.connect.poll_unpin(cx)) {
                    Ok((sink, stream)) => this.stream(sink, stream),
                    Err(err) => {
                        this.state = State::Initial;
                        return Poll::Ready(Some(Err(err)));
                    }
                }
            }

            while let State::Stream(msg) = &mut this.state {
                if msg.is_some() {
                    let mut sink = Pin::new(this.sink.as_mut().unwrap());
                    let _ = ready!(sink.as_mut().poll_ready(cx));
                    if let Some(pong) = msg.take() {
                        let _ = sink.as_mut().start_send(pong);
                    }
                    let _ = ready!(sink.as_mut().poll_flush(cx));
                }

                let Some(msg) = ready!(
                    this.stream
                        .as_mut()
                        .expect("Stream state should be unreachable without stream")
                        .poll_next_unpin(cx)
                ) else {
                    this.state = State::Initial;
                    continue 'start;
                };

                match msg {
                    Ok(Message::Binary(bytes)) => {
                        return Poll::Ready(Some(this.decoder.decode(bytes)));
                    }
                    Ok(Message::Text(bytes)) => {
                        return Poll::Ready(Some(this.decoder.decode(bytes.into())));
                    }
                    Ok(Message::Ping(bytes)) => this.ping(bytes),
                    Ok(Message::Close(frame)) => this.close(frame),
                    Ok(msg) => {
                        debug!(target: "flashblocks", "Received unexpected message: {:?}", msg)
                    }
                    Err(err) => return Poll::Ready(Some(Err(err.into()))),
                }
            }
        }
    }
}

impl<Stream, S, C, F> WsFlashBlockStream<Stream, S, C, F>
where
    C: WsConnect<Stream = Stream, Sink = S> + Clone + Send + 'static,
{
    fn connect(&mut self) {
        let ws_url = self.ws_url.clone();
        let mut connector = self.connector.clone();
        Pin::new(&mut self.connect).set(Box::pin(async move { connector.connect(ws_url).await }));
        self.state = State::Connect;
    }

    fn stream(&mut self, sink: S, stream: Stream) {
        self.sink.replace(sink);
        self.stream.replace(stream);
        self.state = State::Stream(None);
    }

    fn ping(&mut self, pong: Bytes) {
        if let State::Stream(current) = &mut self.state {
            current.replace(Message::Pong(pong));
        }
    }

    fn close(&mut self, frame: Option<CloseFrame>) {
        if let State::Stream(current) = &mut self.state {
            current.replace(Message::Close(frame));
        }
    }
}

impl<Stream: Debug, S: Debug, C: Debug, F> Debug for WsFlashBlockStream<Stream, S, C, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlashBlockStream")
            .field("ws_url", &self.ws_url)
            .field("state", &self.state)
            .field("connector", &self.connector)
            .field("connect", &"Pin<Box<dyn Future<..>>>")
            .field("stream", &self.stream)
            .finish()
    }
}

#[derive(Default, Debug, Eq, PartialEq)]
enum State {
    #[default]
    Initial,
    Connect,
    Stream(Option<Message>),
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsStream = SplitStream<Ws>;
type WsSink = SplitSink<Ws, Message>;
type ConnectFuture<Sink, Stream> =
    Pin<Box<dyn Future<Output = eyre::Result<(Sink, Stream)>> + Send + 'static>>;

pub trait WsConnect {
    type Stream;
    type Sink;

    fn connect(
        &mut self,
        ws_url: Url,
    ) -> impl Future<Output = eyre::Result<(Self::Sink, Self::Stream)>> + Send;
}

#[derive(Debug, Clone)]
pub struct WsConnector;

impl WsConnect for WsConnector {
    type Stream = WsStream;
    type Sink = WsSink;

    async fn connect(&mut self, ws_url: Url) -> eyre::Result<(WsSink, WsStream)> {
        let (stream, _response) = connect_async(ws_url.as_str()).await?;
        Ok(stream.split())
    }
}
