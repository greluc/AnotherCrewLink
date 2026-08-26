//! The blobs two peers exchange through the server, and what they mean.
//!
//! This is a wire format shared with the client it replaces, so it is ported rather than
//! designed. `src/renderer/peer.ts` declares it in four lines:
//!
//! ```text
//! | { type: 'offer'; sdp: string; renegotiation?: true }
//! | { type: 'answer'; sdp: string }
//! | { candidate: RTCIceCandidateInit }
//! | { renegotiate: true }
//! ```
//!
//! During §4.10's rollout a 2.x client and a 1.x client are in the same lobby, so a shape
//! invented here is a player who cannot hear half the game. The server does not help:
//! `signal` carries the payload as an opaque `RawValue` and never looks inside it.
//!
//! # The one that is easy to drop
//!
//! A trickled candidate has no `type` field. The shipped client forwarded only signals
//! that had one, so every trickled candidate was thrown away and connections depended on
//! whatever happened to be in the initial SDP — the comment in `Voice.tsx` records it.
//! [`Payload::from_value`] therefore recognises a candidate by the key it does have,
//! rather than by the absence of another.

use serde_json::{Value, json};

/// One signal, as either end sends it.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// A session description that starts or continues a connection.
    Offer {
        /// The SDP.
        sdp: String,
        /// Whether it continues a session rather than starting one.
        ///
        /// Present as `renegotiation: true` and absent otherwise, never `false`. That is
        /// what the shipped client emits, and [`acl_net::signal_route`] reads it to decide
        /// whether an offer replaces the connection or is applied to it — the distinction
        /// that made a repair destroy the thing it was repairing.
        renegotiation: bool,
    },
    /// The answer to an offer.
    Answer {
        /// The SDP.
        sdp: String,
    },
    /// One trickled ICE candidate, as the browser's `toJSON` renders it.
    ///
    /// Carried opaquely. Its fields belong to the WebRTC implementation at each end, and a
    /// client that parsed and re-serialised it would silently drop anything it did not
    /// know about.
    Candidate(Value),
    /// A request that the other end make a fresh offer.
    Renegotiate,
}

impl Payload {
    /// Reads one, or nothing if it is not a shape either end sends.
    ///
    /// Order matters. `candidate` is checked before `type`, because a candidate has no
    /// `type` and the failure that costs is silent: a connection that works on a local
    /// network and not across one, since the only candidates that survive are the ones
    /// already in the SDP.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        if let Some(candidate) = value.get("candidate") {
            return Some(Self::Candidate(candidate.clone()));
        }
        if value.get("renegotiate").and_then(Value::as_bool) == Some(true) {
            return Some(Self::Renegotiate);
        }
        let sdp = value.get("sdp").and_then(Value::as_str)?;
        match value.get("type").and_then(Value::as_str)? {
            "offer" => Some(Self::Offer {
                sdp: sdp.to_owned(),
                renegotiation: value.get("renegotiation").and_then(Value::as_bool) == Some(true),
            }),
            "answer" => Some(Self::Answer {
                sdp: sdp.to_owned(),
            }),
            _ => None,
        }
    }

    /// Writes one in the shape the other end expects.
    ///
    /// `renegotiation` is omitted when false rather than sent as `false`, because that is
    /// what the shipped client does and a 1.x peer reads the key's presence.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Offer {
                sdp,
                renegotiation: true,
            } => json!({"type": "offer", "sdp": sdp, "renegotiation": true}),
            Self::Offer { sdp, .. } => json!({"type": "offer", "sdp": sdp}),
            Self::Answer { sdp } => json!({"type": "answer", "sdp": sdp}),
            Self::Candidate(candidate) => json!({"candidate": candidate}),
            Self::Renegotiate => json!({"renegotiate": true}),
        }
    }

    /// How [`acl_net::signal_route`] sees it.
    #[must_use]
    pub const fn route(&self) -> acl_net::signal_route::Signal {
        acl_net::signal_route::Signal {
            is_offer: matches!(self, Self::Offer { .. }),
            is_renegotiation: matches!(
                self,
                Self::Offer {
                    renegotiation: true,
                    ..
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Payload;
    use serde_json::json;

    /// The regression the comment in `Voice.tsx` records: only signals carrying a `type`
    /// were forwarded, so trickled candidates -- which have none -- were dropped outright,
    /// and connections depended on whatever was already in the SDP.
    #[test]
    fn a_trickled_candidate_has_no_type_and_is_still_a_signal() {
        let signal =
            json!({"candidate": {"candidate": "candidate:1 1 udp ...", "sdpMLineIndex": 0}});
        let payload = Payload::from_value(&signal).expect("a candidate is a signal");
        assert!(matches!(payload, Payload::Candidate(_)));
    }

    /// And it goes back out unchanged. Its fields belong to the WebRTC implementation at
    /// each end, and re-serialising a parsed copy drops whatever this build does not know.
    #[test]
    fn a_candidate_survives_the_round_trip_untouched() {
        let inner =
            json!({"candidate": "candidate:1 1 udp ...", "usernameFragment": "abc", "future": 7});
        let out = Payload::Candidate(inner.clone()).to_value();
        assert_eq!(out["candidate"], inner);
        assert_eq!(Payload::from_value(&out), Some(Payload::Candidate(inner)));
    }

    /// `renegotiation` is the flag that stopped a repair from destroying the connection it
    /// was repairing, and the other end reads its presence rather than its value.
    #[test]
    fn a_renegotiation_offer_carries_the_flag_and_a_first_offer_omits_it() {
        let first = Payload::Offer {
            sdp: "v=0".to_owned(),
            renegotiation: false,
        };
        assert_eq!(first.to_value(), json!({"type": "offer", "sdp": "v=0"}));
        assert!(first.to_value().get("renegotiation").is_none());

        let again = Payload::Offer {
            sdp: "v=0".to_owned(),
            renegotiation: true,
        };
        assert_eq!(
            again.to_value(),
            json!({"type": "offer", "sdp": "v=0", "renegotiation": true})
        );
    }

    /// Every shape either end sends survives a round trip, which is the whole obligation
    /// of a format shared with a client this one cannot change.
    #[test]
    fn every_shape_round_trips() {
        for payload in [
            Payload::Offer {
                sdp: "v=0 offer".to_owned(),
                renegotiation: false,
            },
            Payload::Offer {
                sdp: "v=0 again".to_owned(),
                renegotiation: true,
            },
            Payload::Answer {
                sdp: "v=0 answer".to_owned(),
            },
            Payload::Candidate(json!({"candidate": "x"})),
            Payload::Renegotiate,
        ] {
            assert_eq!(
                Payload::from_value(&payload.to_value()),
                Some(payload.clone()),
                "{payload:?} did not survive"
            );
        }
    }

    /// Only an offer routes as one, and only a renegotiation offer as that. The route is
    /// what decides whether an incoming offer replaces a live connection.
    #[test]
    fn only_offers_route_as_offers() {
        assert!(
            Payload::Offer {
                sdp: String::new(),
                renegotiation: false
            }
            .route()
            .is_offer
        );
        assert!(!Payload::Answer { sdp: String::new() }.route().is_offer);
        assert!(!Payload::Candidate(json!({})).route().is_offer);
        assert!(
            Payload::Offer {
                sdp: String::new(),
                renegotiation: true
            }
            .route()
            .is_renegotiation
        );
    }

    #[test]
    fn nonsense_is_not_a_signal() {
        for value in [
            json!({}),
            json!({"type": "offer"}),
            json!({"sdp": "v=0"}),
            json!({"type": "pravda", "sdp": "v=0"}),
            json!({"renegotiate": false}),
        ] {
            assert_eq!(
                Payload::from_value(&value),
                None,
                "{value} should not parse"
            );
        }
    }
}
