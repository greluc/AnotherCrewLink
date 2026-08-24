//! Socket.IO v5 packets and the session state they drive.
//!
//! A Socket.IO packet rides inside an [`crate::engineio::Packet::Message`]: one character
//! of type, an optional namespace, an optional acknowledgement id, then JSON.
//!
//! This client speaks the default namespace only and sends no binary attachments, which
//! removes types 5 and 6 and the whole attachment-counting prefix from the surface. Both
//! shipping clients already work that way.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engineio::Handshake;

/// A Socket.IO packet, in the subset this client uses.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// `0` — a connection to a namespace. From the server it carries the Socket.IO `sid`.
    Connect(Option<Value>),
    /// `1` — a namespace disconnect.
    Disconnect,
    /// `2` — an event: a name and its arguments, optionally expecting an ack.
    Event {
        /// The event name.
        name: String,
        /// Everything after the name.
        args: Vec<Value>,
        /// The id the peer should echo in its [`Packet::Ack`].
        ack_id: Option<u64>,
    },
    /// `3` — an acknowledgement of an event this end sent.
    Ack {
        /// The id from the event being answered.
        ack_id: u64,
        /// The arguments the peer answered with.
        args: Vec<Value>,
    },
    /// `4` — the server refused the connection.
    ///
    /// Distinct from a transport close on purpose: a refusal is an answer and should not
    /// drive the reconnect policy, while a close is silence and should.
    ConnectError(Option<Value>),
}

/// Why a Socket.IO packet could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// A packet with no type character.
    #[error("empty socket.io packet")]
    Empty,
    /// A type this client does not speak. Types 5 and 6 are binary and land here.
    #[error("unsupported socket.io packet type {0:?}")]
    UnsupportedType(char),
    /// The payload was not the JSON this packet type requires.
    #[error("malformed socket.io payload: {0}")]
    Malformed(String),
    /// An event with no name, which is not addressable.
    #[error("event packet with no name")]
    NamelessEvent,
}

impl Packet {
    /// Reads one Socket.IO packet out of an Engine.IO message body.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] for an empty packet, an unsupported type, or a payload
    /// that is not the JSON the type requires.
    pub fn decode(body: &str) -> Result<Self, DecodeError> {
        let mut chars = body.chars();
        let kind = chars.next().ok_or(DecodeError::Empty)?;
        let mut rest = chars.as_str();

        // A namespace, when present, runs to the comma. The default namespace is sent as
        // "/" or omitted; anything else is not ours, but skipping it is still the right
        // parse — the caller decides what to do about it.
        if rest.starts_with('/') {
            if let Some(comma) = rest.find(',') {
                rest = rest.get(comma + 1..).unwrap_or("");
            } else {
                rest = "";
            }
        }

        // Then an optional ack id: the digits before the JSON.
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (ack_id, payload) = if digits > 0 {
            let (head, tail) = rest.split_at(digits);
            (head.parse::<u64>().ok(), tail)
        } else {
            (None, rest)
        };

        match kind {
            '0' => Ok(Self::Connect(parse_optional(payload)?)),
            '1' => Ok(Self::Disconnect),
            '2' => {
                let values = parse_array(payload)?;
                let mut values = values.into_iter();
                let name = values
                    .next()
                    .and_then(|first| first.as_str().map(ToOwned::to_owned))
                    .ok_or(DecodeError::NamelessEvent)?;
                Ok(Self::Event {
                    name,
                    args: values.collect(),
                    ack_id,
                })
            }
            '3' => Ok(Self::Ack {
                // An ack with no id is not answerable; treating it as 0 would answer the
                // wrong call, so it is malformed.
                ack_id: ack_id
                    .ok_or_else(|| DecodeError::Malformed("ack with no id".to_owned()))?,
                args: parse_array(payload)?,
            }),
            '4' => Ok(Self::ConnectError(parse_optional(payload)?)),
            other => Err(DecodeError::UnsupportedType(other)),
        }
    }

    /// Writes one packet for the default namespace.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Connect(payload) => match payload {
                Some(value) => format!("0{value}"),
                None => "0".to_owned(),
            },
            Self::Disconnect => "1".to_owned(),
            Self::Event { name, args, ack_id } => {
                let mut values = vec![Value::String(name.clone())];
                values.extend(args.iter().cloned());
                let json = Value::Array(values);
                match ack_id {
                    Some(id) => format!("2{id}{json}"),
                    None => format!("2{json}"),
                }
            }
            Self::Ack { ack_id, args } => {
                let json = Value::Array(args.clone());
                format!("3{ack_id}{json}")
            }
            Self::ConnectError(payload) => match payload {
                Some(value) => format!("4{value}"),
                None => "4".to_owned(),
            },
        }
    }
}

fn parse_array(payload: &str) -> Result<Vec<Value>, DecodeError> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(payload) {
        Ok(Value::Array(values)) => Ok(values),
        Ok(_) => Err(DecodeError::Malformed("expected an array".to_owned())),
        Err(error) => Err(DecodeError::Malformed(error.to_string())),
    }
}

fn parse_optional(payload: &str) -> Result<Option<Value>, DecodeError> {
    if payload.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(payload)
        .map(Some)
        .map_err(|error| DecodeError::Malformed(error.to_string()))
}

/// How long an unanswered acknowledgement is kept before it is given up on.
///
/// The plan names leaking ack ids as one of the five ways a hand-written v4 client fails,
/// and the concrete case is `join_lobby`: a server that never acks it leaves an entry in
/// this map for every join, for as long as the process runs.
pub const ACK_TIMEOUT: Duration = Duration::from_secs(30);

/// One second short of [`ACK_TIMEOUT`], for the test that checks the boundary.
#[cfg(test)]
const ALMOST_TIMED_OUT: Duration = Duration::from_secs(29);

/// The two session ids, and the outstanding acknowledgements.
///
/// Engine.IO and Socket.IO each hand out a session id and they are **different values**.
/// The Engine.IO one identifies the transport; the Socket.IO one, delivered in the CONNECT
/// packet, is what the server means by "socket id" everywhere else — including in every
/// `signal` envelope this application sends. Using the wrong one addresses a socket that
/// does not exist, and the symptom is that voice never connects while everything else
/// looks healthy.
#[derive(Debug)]
pub struct Session {
    engine_sid: String,
    socket_sid: Option<String>,
    handshake: Handshake,
    next_ack: u64,
    pending: HashMap<u64, (String, Instant)>,
}

impl Session {
    /// Starts a session from the Engine.IO handshake. The Socket.IO id arrives later.
    #[must_use]
    pub fn new(handshake: Handshake) -> Self {
        Self {
            engine_sid: handshake.sid.clone(),
            socket_sid: None,
            handshake,
            next_ack: 0,
            pending: HashMap::new(),
        }
    }

    /// The transport's session id.
    #[must_use]
    pub fn engine_sid(&self) -> &str {
        &self.engine_sid
    }

    /// The socket id the server uses to address this client, once CONNECT has arrived.
    #[must_use]
    pub fn socket_sid(&self) -> Option<&str> {
        self.socket_sid.as_deref()
    }

    /// The session parameters from the handshake.
    #[must_use]
    pub fn handshake(&self) -> &Handshake {
        &self.handshake
    }

    /// Records the Socket.IO id from a CONNECT packet.
    ///
    /// Returns `false` if the packet carried no `sid`, which is a server that has not
    /// finished connecting rather than one that has.
    pub fn accept_connect(&mut self, payload: Option<&Value>) -> bool {
        let Some(sid) = payload
            .and_then(|value| value.get("sid"))
            .and_then(Value::as_str)
        else {
            return false;
        };
        self.socket_sid = Some(sid.to_owned());
        true
    }

    /// Claims the next acknowledgement id for an event, recording what it belongs to.
    pub fn claim_ack(&mut self, event: &str, now: Instant) -> u64 {
        let id = self.next_ack;
        self.next_ack = self.next_ack.wrapping_add(1);
        self.pending.insert(id, (event.to_owned(), now));
        id
    }

    /// Retires an acknowledgement, returning the event it belonged to.
    pub fn resolve_ack(&mut self, ack_id: u64) -> Option<String> {
        self.pending.remove(&ack_id).map(|(event, _)| event)
    }

    /// Drops acknowledgements the peer never answered, returning what was dropped.
    ///
    /// Without this the map grows for the life of the process. It is called on the
    /// heartbeat rather than on a timer of its own: the heartbeat is already the thing
    /// that runs at a known interval, and an ack that has waited longer than
    /// [`ACK_TIMEOUT`] is not going to arrive.
    pub fn expire_acks(&mut self, now: Instant) -> Vec<(u64, String)> {
        let expired: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, (_, sent))| now.duration_since(*sent) >= ACK_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        expired
            .into_iter()
            .filter_map(|id| self.pending.remove(&id).map(|(event, _)| (id, event)))
            .collect()
    }

    /// How many acknowledgements are still outstanding.
    #[must_use]
    pub fn pending_acks(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use serde_json::json;

    fn handshake() -> Handshake {
        Handshake::parse(
            r#"{"sid":"engine-side","upgrades":[],"pingInterval":25000,"pingTimeout":20000}"#,
        )
        .expect("a well-formed handshake")
    }

    #[test]
    fn reads_an_event() {
        let packet = Packet::decode(r#"2["join","ABCDEF",1,2]"#).expect("an event");
        assert_eq!(
            packet,
            Packet::Event {
                name: "join".to_owned(),
                args: vec![json!("ABCDEF"), json!(1), json!(2)],
                ack_id: None,
            }
        );
    }

    #[test]
    fn reads_an_event_that_wants_an_acknowledgement() {
        let packet = Packet::decode(r#"217["join_lobby","ABCDEF"]"#).expect("an event with an ack");
        let Packet::Event { ack_id, name, .. } = packet else {
            panic!("expected an event");
        };
        assert_eq!(name, "join_lobby");
        assert_eq!(ack_id, Some(17));
    }

    #[test]
    fn reads_an_event_in_an_explicit_namespace() {
        let packet = Packet::decode(r#"2/admin,5["ping"]"#).expect("a namespaced event");
        let Packet::Event { name, ack_id, .. } = packet else {
            panic!("expected an event");
        };
        // The namespace is skipped, not folded into the ack id — which is what a parser
        // that looks for digits before checking for a namespace would do.
        assert_eq!(name, "ping");
        assert_eq!(ack_id, Some(5));
    }

    #[test]
    fn round_trips_an_event() {
        let packet = Packet::Event {
            name: "signal".to_owned(),
            args: vec![json!({ "to": "abc", "data": 1 })],
            ack_id: Some(3),
        };
        assert_eq!(Packet::decode(&packet.encode()), Ok(packet));
    }

    #[test]
    fn refuses_a_binary_packet_rather_than_guessing() {
        // Types 5 and 6 carry attachments. This client sends none and the server sends
        // none, so one arriving is a protocol surprise and should say so.
        assert_eq!(Packet::decode("5"), Err(DecodeError::UnsupportedType('5')));
    }

    // Failure mode 3: the Socket.IO sid in the CONNECT ack is not the Engine.IO sid.
    #[test]
    fn keeps_the_two_session_ids_apart() {
        let mut session = Session::new(handshake());
        assert_eq!(session.engine_sid(), "engine-side");
        assert_eq!(session.socket_sid(), None);

        let connect = Packet::decode(r#"0{"sid":"socket-side"}"#).expect("a connect");
        let Packet::Connect(payload) = connect else {
            panic!("expected a connect");
        };
        assert!(session.accept_connect(payload.as_ref()));

        assert_eq!(session.socket_sid(), Some("socket-side"));
        // The one that matters for addressing is the Socket.IO one, and it is not the
        // transport's.
        assert_ne!(session.engine_sid(), session.socket_sid().unwrap_or(""));
    }

    #[test]
    fn a_connect_without_a_sid_does_not_complete_the_session() {
        let mut session = Session::new(handshake());
        assert!(!session.accept_connect(None));
        assert!(!session.accept_connect(Some(&json!({ "not_a_sid": 1 }))));
        assert_eq!(session.socket_sid(), None);
    }

    // Failure mode 5: a refusal is an answer, a close is silence, and they must not be
    // the same event to the caller.
    #[test]
    fn tells_a_refusal_apart_from_a_disconnect() {
        let refused = Packet::decode(r#"4{"message":"Not authorized"}"#).expect("a connect error");
        assert!(matches!(refused, Packet::ConnectError(Some(_))));
        assert_eq!(Packet::decode("1"), Ok(Packet::Disconnect));
        // Same transport, same frame shape, different meaning: one must not drive the
        // reconnect policy and the other must.
        assert_ne!(Packet::decode("4"), Packet::decode("1"));
    }

    // Failure mode 4: ack ids leak if the server never acks join_lobby.
    #[test]
    fn gives_up_on_acknowledgements_the_server_never_sends() {
        let mut session = Session::new(handshake());
        let start = Instant::now();

        let id = session.claim_ack("join_lobby", start);
        assert_eq!(session.pending_acks(), 1);

        // Still waiting, just before the deadline.
        assert!(session.expire_acks(start + ALMOST_TIMED_OUT).is_empty());
        assert_eq!(session.pending_acks(), 1);

        let expired = session.expire_acks(start + ACK_TIMEOUT);
        assert_eq!(expired, vec![(id, "join_lobby".to_owned())]);
        assert_eq!(session.pending_acks(), 0);
    }

    #[test]
    fn an_answered_acknowledgement_is_retired_rather_than_expiring() {
        let mut session = Session::new(handshake());
        let start = Instant::now();
        let id = session.claim_ack("join_lobby", start);

        assert_eq!(session.resolve_ack(id).as_deref(), Some("join_lobby"));
        assert_eq!(session.pending_acks(), 0);
        // And answering twice is not an error that removes someone else's entry.
        assert_eq!(session.resolve_ack(id), None);
    }

    #[test]
    fn ack_ids_do_not_repeat_while_one_is_outstanding() {
        let mut session = Session::new(handshake());
        let now = Instant::now();
        let first = session.claim_ack("join_lobby", now);
        let second = session.claim_ack("join_lobby", now);
        assert_ne!(first, second);
        assert_eq!(session.pending_acks(), 2);
    }
}
