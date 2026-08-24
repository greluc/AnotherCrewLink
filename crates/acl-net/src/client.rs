//! The Socket.IO client as a state machine, with no transport in it.
//!
//! Everything that can go wrong in a hand-written v4 client goes wrong here rather than
//! in the socket: the heartbeat direction, where the session parameters come from, which
//! of the two session ids is the addressable one, what happens to an acknowledgement the
//! server never sends, and whether a refusal is distinguishable from silence.
//!
//! Keeping the socket out means all five are testable by feeding frames in and reading
//! actions out, with no timing and no network. The transport's whole job is to move
//! strings and to report a close honestly.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engineio::{self, Handshake};
use crate::socketio::{self, Session};

/// What the client wants done after reading a frame or a tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send this text frame.
    Send(String),
    /// The session is up; this is the socket id the server will address it by.
    Connected(String),
    /// An event arrived.
    Event {
        /// The event name.
        name: String,
        /// Everything after the name.
        args: Vec<Value>,
    },
    /// An event this end sent was acknowledged.
    Acked {
        /// The event that was answered.
        event: String,
        /// What the peer answered with.
        args: Vec<Value>,
    },
    /// An acknowledgement was given up on. See [`socketio::ACK_TIMEOUT`].
    AckExpired {
        /// The event that was never answered.
        event: String,
    },
    /// Close the transport and do not reconnect: the server answered and said no.
    Refused {
        /// What the server said, when it said anything.
        reason: Option<Value>,
    },
    /// Close the transport. Reconnecting is the caller's decision.
    Closed {
        /// Why.
        reason: CloseReason,
    },
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The server sent an Engine.IO close or a Socket.IO disconnect.
    ServerClosed,
    /// No heartbeat arrived within the deadline the handshake set.
    HeartbeatMissed,
    /// A frame this client could not read. Continuing past one is guesswork.
    ProtocolError,
}

/// Where a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for the Engine.IO handshake.
    AwaitingOpen,
    /// Handshake seen, CONNECT sent, waiting for the Socket.IO session id.
    AwaitingConnect,
    /// Both ids in hand.
    Live,
    /// Finished, for any reason.
    Ended,
}

/// A Socket.IO v5 client over a WebSocket transport.
#[derive(Debug)]
pub struct Client {
    state: State,
    session: Option<Session>,
    last_heard: Instant,
}

impl Client {
    /// A client that has connected its transport and is waiting for the handshake.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            state: State::AwaitingOpen,
            session: None,
            last_heard: now,
        }
    }

    /// The session, once the handshake has been read.
    #[must_use]
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Whether the session is finished.
    #[must_use]
    pub fn is_ended(&self) -> bool {
        self.state == State::Ended
    }

    /// Reads one text frame from the transport.
    pub fn on_frame(&mut self, frame: &str, now: Instant) -> Vec<Action> {
        if self.state == State::Ended {
            return Vec::new();
        }
        self.last_heard = now;

        let Ok(packet) = engineio::Packet::decode(frame) else {
            return self.end(CloseReason::ProtocolError);
        };

        match packet {
            engineio::Packet::Open(body) => self.on_open(&body),
            // The heartbeat runs server-to-client in v4 and client-to-server in v3. A
            // client that has this backwards sends nothing, looks healthy for
            // `pingTimeout` milliseconds, and is then dropped for silence.
            engineio::Packet::Ping => vec![Action::Send(engineio::Packet::Pong.encode())],
            engineio::Packet::Pong | engineio::Packet::Ignored => Vec::new(),
            engineio::Packet::Close => self.end(CloseReason::ServerClosed),
            engineio::Packet::Message(body) => self.on_message(&body, now),
        }
    }

    /// Called on a timer. Enforces the heartbeat deadline and expires acknowledgements.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Action> {
        if self.state == State::Ended {
            return Vec::new();
        }

        let mut actions = Vec::new();
        if let Some(session) = self.session.as_mut() {
            for (_, event) in session.expire_acks(now) {
                actions.push(Action::AckExpired { event });
            }
        }

        if now.duration_since(self.last_heard) >= self.deadline() {
            actions.extend(self.end(CloseReason::HeartbeatMissed));
        }
        actions
    }

    /// Builds an event frame, claiming an acknowledgement id when one is wanted.
    ///
    /// Returns `None` before the session is live: sending then would be addressed by a
    /// socket id the server has not issued yet.
    pub fn emit(
        &mut self,
        name: &str,
        args: Vec<Value>,
        wants_ack: bool,
        now: Instant,
    ) -> Option<String> {
        if self.state != State::Live {
            return None;
        }
        let session = self.session.as_mut()?;
        let ack_id = wants_ack.then(|| session.claim_ack(name, now));
        let packet = socketio::Packet::Event {
            name: name.to_owned(),
            args,
            ack_id,
        };
        Some(engineio::Packet::Message(packet.encode()).encode())
    }

    fn deadline(&self) -> Duration {
        self.session.as_ref().map_or_else(
            // Before the handshake there is no promise to hold the server to, so this is
            // only a bound on how long a silent socket is kept.
            || Duration::from_secs(30),
            |session| session.handshake().heartbeat_deadline(),
        )
    }

    fn on_open(&mut self, body: &str) -> Vec<Action> {
        let Ok(handshake) = Handshake::parse(body) else {
            return self.end(CloseReason::ProtocolError);
        };
        self.session = Some(Session::new(handshake));
        self.state = State::AwaitingConnect;
        // CONNECT to the default namespace. Nothing may be sent before this is answered.
        vec![Action::Send(
            engineio::Packet::Message(socketio::Packet::Connect(None).encode()).encode(),
        )]
    }

    fn on_message(&mut self, body: &str, now: Instant) -> Vec<Action> {
        let Ok(packet) = socketio::Packet::decode(body) else {
            return self.end(CloseReason::ProtocolError);
        };

        match packet {
            socketio::Packet::Connect(payload) => {
                let Some(session) = self.session.as_mut() else {
                    return self.end(CloseReason::ProtocolError);
                };
                if !session.accept_connect(payload.as_ref()) {
                    return self.end(CloseReason::ProtocolError);
                }
                self.state = State::Live;
                session
                    .socket_sid()
                    .map(|sid| vec![Action::Connected(sid.to_owned())])
                    .unwrap_or_default()
            }
            // An answer, not silence. The caller must not treat this as a dropped
            // connection and retry into a rejection loop.
            socketio::Packet::ConnectError(reason) => {
                self.state = State::Ended;
                vec![Action::Refused { reason }]
            }
            socketio::Packet::Disconnect => self.end(CloseReason::ServerClosed),
            socketio::Packet::Event { name, args, ack_id } => {
                let mut actions = vec![Action::Event { name, args }];
                if let Some(id) = ack_id {
                    // The server asked to be told this arrived. Not answering leaks an
                    // entry on its side, which is the same bug in the other direction.
                    let ack = socketio::Packet::Ack {
                        ack_id: id,
                        args: Vec::new(),
                    };
                    actions.push(Action::Send(
                        engineio::Packet::Message(ack.encode()).encode(),
                    ));
                }
                actions
            }
            socketio::Packet::Ack { ack_id, args } => {
                let _ = now;
                self.session
                    .as_mut()
                    .and_then(|session| session.resolve_ack(ack_id))
                    .map(|event| vec![Action::Acked { event, args }])
                    .unwrap_or_default()
            }
        }
    }

    fn end(&mut self, reason: CloseReason) -> Vec<Action> {
        self.state = State::Ended;
        vec![Action::Closed { reason }]
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use serde_json::json;

    const OPEN: &str =
        r#"0{"sid":"engine-side","upgrades":[],"pingInterval":9000,"pingTimeout":4000}"#;
    const CONNECT: &str = r#"40{"sid":"socket-side"}"#;

    fn live() -> (Client, Instant) {
        let now = Instant::now();
        let mut client = Client::new(now);
        client.on_frame(OPEN, now);
        client.on_frame(CONNECT, now);
        (client, now)
    }

    // Failure mode 1: in v4 the server pings and the client pongs. v3 was the other way
    // round, and a client with it backwards is dropped for silence after pingTimeout.
    #[test]
    fn answers_the_servers_ping_with_a_pong() {
        let (mut client, now) = live();
        let actions = client.on_frame("2", now);
        assert_eq!(actions, vec![Action::Send("3".to_owned())]);
    }

    #[test]
    fn never_sends_a_ping_of_its_own() {
        let (mut client, now) = live();
        // A tick well inside the deadline produces nothing to send. A v3-shaped client
        // would emit a ping here, which a v4 server answers with nothing.
        assert!(client.on_tick(now + Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn the_handshake_drives_the_connect_and_the_socket_id() {
        let now = Instant::now();
        let mut client = Client::new(now);

        let actions = client.on_frame(OPEN, now);
        assert_eq!(actions, vec![Action::Send("40".to_owned())]);
        assert_eq!(client.session().unwrap().engine_sid(), "engine-side");

        let actions = client.on_frame(CONNECT, now);
        assert_eq!(actions, vec![Action::Connected("socket-side".to_owned())]);
    }

    #[test]
    fn refuses_to_emit_before_the_session_is_live() {
        let now = Instant::now();
        let mut client = Client::new(now);
        assert!(
            client
                .emit("join", vec![json!("ABCDEF")], false, now)
                .is_none()
        );

        client.on_frame(OPEN, now);
        // The handshake alone is not enough: the socket id has not been issued.
        assert!(
            client
                .emit("join", vec![json!("ABCDEF")], false, now)
                .is_none()
        );

        client.on_frame(CONNECT, now);
        assert_eq!(
            client.emit("join", vec![json!("ABCDEF")], false, now),
            Some(r#"42["join","ABCDEF"]"#.to_owned())
        );
    }

    #[test]
    fn acknowledges_an_event_that_asks_to_be_acknowledged() {
        let (mut client, now) = live();
        let actions = client.on_frame(r#"427["setHost",3]"#, now);
        assert_eq!(
            actions,
            vec![
                Action::Event {
                    name: "setHost".to_owned(),
                    args: vec![json!(3)],
                },
                Action::Send("437[]".to_owned()),
            ]
        );
    }

    // Failure mode 4, end to end: the server never answers join_lobby.
    #[test]
    fn gives_up_on_an_acknowledgement_the_server_never_sends() {
        let (mut client, now) = live();
        client.emit("join_lobby", vec![json!(1)], true, now);
        assert_eq!(client.session().unwrap().pending_acks(), 1);

        // Heartbeats keep arriving, so the session is healthy — the ack is not.
        let later = now + socketio::ACK_TIMEOUT;
        client.on_frame("2", later);
        let actions = client.on_tick(later);

        assert_eq!(
            actions,
            vec![Action::AckExpired {
                event: "join_lobby".to_owned()
            }]
        );
        assert_eq!(client.session().unwrap().pending_acks(), 0);
        assert!(!client.is_ended());
    }

    #[test]
    fn an_answered_event_reports_what_it_was() {
        let (mut client, now) = live();
        client.emit("join_lobby", vec![json!(1)], true, now);
        let actions = client.on_frame(r#"430[0,"ABCDEF"]"#, now);
        assert_eq!(
            actions,
            vec![Action::Acked {
                event: "join_lobby".to_owned(),
                args: vec![json!(0), json!("ABCDEF")],
            }]
        );
    }

    // Failure mode 5: an auth rejection must not drive the reconnect policy.
    #[test]
    fn tells_a_refusal_apart_from_a_close() {
        let now = Instant::now();
        let mut client = Client::new(now);
        client.on_frame(OPEN, now);

        let actions = client.on_frame(r#"44{"message":"Not authorized"}"#, now);
        assert_eq!(
            actions,
            vec![Action::Refused {
                reason: Some(json!({ "message": "Not authorized" })),
            }]
        );
        assert!(client.is_ended());

        // The same transport, a different meaning.
        let mut other = Client::new(now);
        other.on_frame(OPEN, now);
        assert_eq!(
            other.on_frame("1", now),
            vec![Action::Closed {
                reason: CloseReason::ServerClosed
            }]
        );
    }

    #[test]
    fn ends_the_session_when_the_heartbeat_stops() {
        let (mut client, now) = live();
        // pingInterval 9s + pingTimeout 4s, from the handshake rather than a constant.
        let deadline = Duration::from_secs(13);
        assert!(client.on_tick(now + Duration::from_secs(12)).is_empty());
        assert_eq!(
            client.on_tick(now + deadline),
            vec![Action::Closed {
                reason: CloseReason::HeartbeatMissed
            }]
        );
    }

    #[test]
    fn a_frame_it_cannot_read_ends_the_session_rather_than_being_skipped() {
        let (mut client, now) = live();
        assert_eq!(
            client.on_frame("nonsense", now),
            vec![Action::Closed {
                reason: CloseReason::ProtocolError
            }]
        );
        // And nothing is processed afterwards.
        assert!(client.on_frame("2", now).is_empty());
    }
}
