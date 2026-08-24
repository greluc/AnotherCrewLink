//! Engine.IO v4 framing.
//!
//! One character of packet type, then the payload. That is the whole format on the
//! WebSocket transport — the length-prefixed batching and the base64 binary framing
//! belong to polling, which this client does not offer and the server does not accept.
//!
//! The direction of the heartbeat is the thing to keep in mind here: in v4 the **server**
//! sends `ping` and the client answers `pong`. It was the other way round in v3, and a
//! client that gets it backwards looks fine for `pingTimeout` milliseconds and is then
//! disconnected for silence.

use std::time::Duration;

use serde::Deserialize;

/// An Engine.IO packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// `0` — the handshake, sent by the server. Carries the session parameters.
    Open(String),
    /// `1` — the server is closing the session.
    Close,
    /// `2` — the server's heartbeat. The client answers with [`Packet::Pong`].
    Ping,
    /// `3` — the client's answer to a [`Packet::Ping`].
    Pong,
    /// `4` — a Socket.IO packet, as text.
    Message(String),
    /// `5` and `6`: transport upgrade and no-op. Neither can occur on a
    /// WebSocket-only connection, but a peer that sends one is not a protocol error worth
    /// dropping the session over.
    Ignored,
}

/// Why a frame could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// A frame with nothing in it.
    #[error("empty engine.io frame")]
    Empty,
    /// A first character that is not a packet type.
    #[error("unknown engine.io packet type {0:?}")]
    UnknownType(char),
}

impl Packet {
    /// Reads one frame.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] for an empty frame or an unknown type character.
    pub fn decode(frame: &str) -> Result<Self, DecodeError> {
        let mut chars = frame.chars();
        let kind = chars.next().ok_or(DecodeError::Empty)?;
        let rest = chars.as_str();
        match kind {
            '0' => Ok(Self::Open(rest.to_owned())),
            '1' => Ok(Self::Close),
            '2' => Ok(Self::Ping),
            '3' => Ok(Self::Pong),
            '4' => Ok(Self::Message(rest.to_owned())),
            '5' | '6' => Ok(Self::Ignored),
            other => Err(DecodeError::UnknownType(other)),
        }
    }

    /// Writes one frame.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Open(payload) => format!("0{payload}"),
            Self::Close => "1".to_owned(),
            Self::Ping => "2".to_owned(),
            Self::Pong => "3".to_owned(),
            Self::Message(payload) => format!("4{payload}"),
            Self::Ignored => "6".to_owned(),
        }
    }
}

/// The session parameters the server sends in its handshake.
///
/// These are read from the OPEN packet rather than hard-coded. A client that assumes the
/// defaults works against a server that keeps them and silently misbehaves against one
/// that does not — and the values are exactly the ones an operator tunes when a
/// deployment is behind a proxy that cuts idle connections early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// The Engine.IO session id. **Not** the Socket.IO session id — see
    /// [`crate::socketio::Session`].
    pub sid: String,
    /// How often the server sends [`Packet::Ping`].
    pub ping_interval: Duration,
    /// How long the server waits for the answering [`Packet::Pong`].
    pub ping_timeout: Duration,
    /// The largest payload the server will accept, when it says.
    pub max_payload: Option<u64>,
}

#[derive(Deserialize)]
struct RawHandshake {
    sid: String,
    #[serde(rename = "pingInterval")]
    ping_interval: u64,
    #[serde(rename = "pingTimeout")]
    ping_timeout: u64,
    #[serde(rename = "maxPayload")]
    max_payload: Option<u64>,
}

impl Handshake {
    /// Parses the body of an [`Packet::Open`].
    ///
    /// # Errors
    ///
    /// Returns the serde error if the body is not the expected object.
    pub fn parse(body: &str) -> Result<Self, serde_json::Error> {
        let raw: RawHandshake = serde_json::from_str(body)?;
        Ok(Self {
            sid: raw.sid,
            ping_interval: Duration::from_millis(raw.ping_interval),
            ping_timeout: Duration::from_millis(raw.ping_timeout),
            max_payload: raw.max_payload,
        })
    }

    /// How long to wait for a server heartbeat before treating the session as dead.
    ///
    /// Both values, not either: the server promises a ping every `ping_interval` and
    /// allows `ping_timeout` for the answer, so silence only means something once both
    /// have passed.
    #[must_use]
    pub fn heartbeat_deadline(&self) -> Duration {
        self.ping_interval + self.ping_timeout
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn reads_every_packet_type() {
        assert_eq!(Packet::decode("2"), Ok(Packet::Ping));
        assert_eq!(Packet::decode("3"), Ok(Packet::Pong));
        assert_eq!(Packet::decode("1"), Ok(Packet::Close));
        assert_eq!(Packet::decode("40"), Ok(Packet::Message("0".to_owned())));
        assert_eq!(Packet::decode("5"), Ok(Packet::Ignored));
    }

    #[test]
    fn refuses_a_frame_it_cannot_name() {
        assert_eq!(Packet::decode(""), Err(DecodeError::Empty));
        assert_eq!(Packet::decode("x"), Err(DecodeError::UnknownType('x')));
    }

    #[test]
    fn round_trips() {
        for packet in [
            Packet::Ping,
            Packet::Pong,
            Packet::Close,
            Packet::Message("2[\"hi\"]".to_owned()),
        ] {
            assert_eq!(Packet::decode(&packet.encode()), Ok(packet));
        }
    }

    // Failure mode 2 of the five the plan names: the session parameters come from the
    // OPEN packet rather than being hard-coded.
    #[test]
    fn takes_the_session_parameters_from_the_handshake() {
        let handshake = Handshake::parse(
            r#"{"sid":"engine-side","upgrades":[],"pingInterval":9000,"pingTimeout":4000,"maxPayload":65536}"#,
        )
        .expect("a well-formed handshake");
        assert_eq!(handshake.sid, "engine-side");
        assert_eq!(handshake.ping_interval, Duration::from_secs(9));
        assert_eq!(handshake.ping_timeout, Duration::from_secs(4));
        assert_eq!(handshake.max_payload, Some(65536));
        // Not either one alone: silence means something only once both have passed.
        assert_eq!(handshake.heartbeat_deadline(), Duration::from_secs(13));
    }

    #[test]
    fn tolerates_a_handshake_without_a_payload_limit() {
        // maxPayload is what socketioxide advertises and what the Node server does not.
        let handshake = Handshake::parse(
            r#"{"sid":"s","upgrades":[],"pingInterval":25000,"pingTimeout":20000}"#,
        )
        .expect("a handshake with no maxPayload");
        assert_eq!(handshake.max_payload, None);
    }
}
