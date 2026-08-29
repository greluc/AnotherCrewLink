//! The WebSocket end of the Socket.IO client.
//!
//! Thin on purpose. Everything that decides anything lives in [`crate::client`], which has
//! no socket in it; this moves text frames and reports a close honestly, which is the only
//! obligation the reconnect policy places on it.
//!
//! # What this does not do yet
//!
//! Certificate verification uses webpki's bundled roots, not the operating system's store,
//! and there is no proxy resolution at all. `docs/rust-port/04-implementation-plan.md`
//! §4.3 budgets `rustls-platform-verifier` and a system proxy resolver as named line items
//! for exactly this reason: Chromium supplied WPAD/PAC resolution and the Windows
//! certificate store for free, tokio-tungstenite supplies neither, and the symptom for a
//! user behind a TLS-inspecting corporate proxy is "won't connect at all". Both are P4's
//! to close, and until then this is honest about being incomplete rather than appearing
//! to work everywhere.
//!
//! **Measured against a deployed server on 2026-08-26**, and the incompleteness is
//! narrower than "does not work": webpki's roots accept an ordinary public certificate, so
//! `acl-core`'s `against_a_deployed_server` completes a `wss://` handshake with the
//! production server and is issued a relay credential. What is still open is exactly the
//! case named above — a machine whose trust anchor lives in the operating system's store
//! and not in webpki's, which is what TLS inspection and a private certificate authority
//! both look like. The proxy half is untouched by that measurement.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::client::{Action, Client};

/// How often [`Connection::next`] runs the client's timers when no frame arrives.
///
/// The heartbeat deadline is tens of seconds, so this only has to be small enough that a
/// missed heartbeat is noticed promptly rather than a whole interval late.
///
/// It is a *deadline*, not a sleep, and that is the whole of a fix made on 2026-08-29. It
/// used to be `sleep(TICK)` inside the `select!`, which restarts from zero every time
/// `next` is called -- and the client wraps every call in a fifty-millisecond timeout, so
/// the sleep was cancelled at fifty milliseconds and began again, for ever. `on_tick` was
/// never reached once in the life of a connection: acknowledgements never expired, and a
/// socket that had stopped answering was indistinguishable from a quiet one. The whole
/// heartbeat, which exists so that a dead connection is noticed, was dead itself.
///
/// Against an absolute instant the cancellation is harmless: each call sleeps for whatever
/// is left of the interval, and when nothing is left the timer fires immediately. It now
/// holds for any caller polling at any rate, which the sleep did not and could not.
const TICK: Duration = Duration::from_secs(1);

/// Why a connection ended or could not be made.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The URL was not one this client can connect to.
    #[error("bad server url: {0}")]
    Url(String),
    /// The WebSocket handshake or the socket itself failed.
    #[error("websocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
}

/// A live Socket.IO connection.
pub struct Connection {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    client: Client,
    /// When the client's timers are next due. See [`TICK`].
    next_tick: Instant,
}

impl Connection {
    /// Opens a Socket.IO connection to a server.
    ///
    /// `base` is the server as a user types it — `https://acl.example` or
    /// `http://127.0.0.1:9736`. The Engine.IO path and the transport query are added here,
    /// because getting either wrong produces a 400 that reads like a network problem.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the URL cannot be turned into a WebSocket request or
    /// the handshake fails.
    pub async fn connect(base: &str) -> Result<Self, TransportError> {
        let url = websocket_url(base)?;
        let request = url
            .as_str()
            .into_client_request()
            .map_err(TransportError::WebSocket)?;
        let (socket, _) = tokio_tungstenite::connect_async(request).await?;
        let now = Instant::now();
        Ok(Self {
            socket,
            client: Client::new(now),
            next_tick: now + TICK,
        })
    }

    /// The client state machine, for the session ids and the pending acknowledgements.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Waits for the next batch of actions.
    ///
    /// Returns an empty vector on a tick that produced nothing, and `None` once the
    /// session is over — which is the honest report the reconnect policy is built on.
    pub async fn next(&mut self) -> Option<Vec<Action>> {
        if self.client.is_ended() {
            return None;
        }

        let actions = tokio::select! {
            frame = self.socket.next() => match frame {
                Some(Ok(Message::Text(text))) => self.client.on_frame(&text, Instant::now()),
                // A binary frame cannot be Engine.IO on this transport; a ping or pong is
                // the WebSocket layer's own heartbeat and not Socket.IO's.
                Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => Vec::new(),
                Some(Ok(Message::Close(_))) | None => {
                    return Some(self.client.on_frame("1", Instant::now()));
                }
                Some(Err(_)) => return Some(self.client.on_frame("1", Instant::now())),
            },
            () = tokio::time::sleep_until(tokio::time::Instant::from_std(self.next_tick)) => {
                let now = Instant::now();
                self.next_tick = now + TICK;
                self.client.on_tick(now)
            }
        };

        // Anything the client wants sent goes out before the caller sees the rest, so a
        // pong is never delayed behind whatever the caller does with an event.
        for action in &actions {
            if let Action::Send(frame) = action
                && self
                    .socket
                    .send(Message::Text(frame.clone().into()))
                    .await
                    .is_err()
            {
                return Some(self.client.on_frame("1", Instant::now()));
            }
        }
        Some(actions)
    }

    /// Sends an event, once the session is live.
    ///
    /// Returns `false` before the server has issued a socket id, because an event sent
    /// then has no addressable sender.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the socket fails.
    pub async fn emit(
        &mut self,
        name: &str,
        args: Vec<serde_json::Value>,
        wants_ack: bool,
    ) -> Result<bool, TransportError> {
        let Some(frame) = self.client.emit(name, args, wants_ack, Instant::now()) else {
            return Ok(false);
        };
        self.socket.send(Message::Text(frame.into())).await?;
        Ok(true)
    }

    /// Closes the socket.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the close frame cannot be sent.
    pub async fn close(mut self) -> Result<(), TransportError> {
        self.socket.close(None).await?;
        Ok(())
    }
}

/// Turns a server address into the Engine.IO WebSocket URL.
///
/// `EIO=4` and `transport=websocket` are not optional: without the first the server
/// answers with v3 framing, and without the second it expects a polling handshake to
/// upgrade from.
fn websocket_url(base: &str) -> Result<String, TransportError> {
    let trimmed = base.trim().trim_end_matches('/');
    let rest = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_owned()
    } else {
        return Err(TransportError::Url(format!(
            "{base}: expected http, https, ws or wss"
        )));
    };
    Ok(format!("{rest}/socket.io/?EIO=4&transport=websocket"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn builds_the_engineio_url_a_v4_server_expects() {
        assert_eq!(
            websocket_url("http://127.0.0.1:9736").unwrap(),
            "ws://127.0.0.1:9736/socket.io/?EIO=4&transport=websocket"
        );
        assert_eq!(
            websocket_url("https://acl.example/").unwrap(),
            "wss://acl.example/socket.io/?EIO=4&transport=websocket"
        );
    }

    #[test]
    fn accepts_a_websocket_scheme_as_typed() {
        assert_eq!(
            websocket_url("wss://acl.example").unwrap(),
            "wss://acl.example/socket.io/?EIO=4&transport=websocket"
        );
    }

    #[test]
    fn refuses_a_scheme_it_cannot_connect_to() {
        assert!(websocket_url("ftp://acl.example").is_err());
        // A bare host is refused rather than guessed at: guessing http would silently
        // downgrade someone who meant https.
        assert!(websocket_url("acl.example").is_err());
    }
}
