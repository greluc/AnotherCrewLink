//! Where an incoming signal goes.
//!
//! A straight port of `src/renderer/signalRoute.ts` and its tests. Four lines of
//! branching in `Voice.tsx` used to decide this, and one of the four was wrong in a way
//! nothing could catch: every offer was treated as the start of a new connection, so a
//! renegotiation offer — which exists to keep a connection alive — destroyed it. The
//! repair for a stalled connection was the thing that killed it.
//!
//! It is a pure function of two facts, which is why it comes across unchanged and why it
//! sits here rather than inside the peer that will use it: the decision is testable
//! without a transport, and `offer_glare_does_not_destroy_replacement` is one of the four
//! named regression tests §4.6 requires of this phase.

/// What the signalling layer should do with one incoming signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalRoute {
    /// Apply it to the connection already running with this peer.
    Existing,
    /// Build a connection to answer with, replacing anything there.
    Create,
    /// Nothing to apply it to.
    Drop,
}

/// The sender's intent, as the signal carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signal {
    /// Whether this is an offer rather than an answer or a trickled candidate.
    pub is_offer: bool,
    /// Whether the sender marked it as continuing a session it already has.
    pub is_renegotiation: bool,
}

/// What this end already holds for the peer the signal came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerState {
    /// Whether there is a connection object for this peer at all.
    pub exists: bool,
    /// Whether this end has applied a remote description, so there is a live session to
    /// continue rather than an empty shell.
    pub has_session: bool,
}

/// Decides where a signal goes.
///
/// The two conditions for continuing are both required and mean different things.
/// `is_renegotiation` is the sender's intent, and a sender that predates the marker does
/// not set it — so an older client's renegotiation still rebuilds, exactly as it did
/// before the marker existed, rather than being applied to a session the sender may not
/// think it has. `has_session` is this end's own state, and without it there is nothing
/// for an offer to be applied to.
#[must_use]
pub fn route_signal(signal: Signal, peer: PeerState) -> SignalRoute {
    if signal.is_offer {
        if signal.is_renegotiation && peer.exists && peer.has_session {
            return SignalRoute::Existing;
        }
        // A first offer, an offer from a peer that rebuilt its side, or a renegotiation
        // this end has no session for. All of them want a fresh connection to answer with.
        return SignalRoute::Create;
    }
    // An answer or a trickled candidate. There is nothing sensible to do with either
    // without the connection they belong to.
    if peer.exists {
        SignalRoute::Existing
    } else {
        SignalRoute::Drop
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    const OFFER: Signal = Signal {
        is_offer: true,
        is_renegotiation: false,
    };
    const RENEGOTIATION: Signal = Signal {
        is_offer: true,
        is_renegotiation: true,
    };
    const ANSWER_OR_CANDIDATE: Signal = Signal {
        is_offer: false,
        is_renegotiation: false,
    };

    const LIVE: PeerState = PeerState {
        exists: true,
        has_session: true,
    };
    const HALF_BUILT: PeerState = PeerState {
        exists: true,
        has_session: false,
    };
    const NOTHING: PeerState = PeerState {
        exists: false,
        has_session: false,
    };

    #[test]
    fn applies_a_renegotiation_to_the_connection_it_is_renegotiating() {
        // The one this file exists for. An ICE restart sends this offer to keep a stalled
        // connection alive; rebuilding for it destroys exactly what was being repaired,
        // and the player hears a repair attempt in the log and silence in their ears.
        assert_eq!(route_signal(RENEGOTIATION, LIVE), SignalRoute::Existing);
    }

    #[test]
    fn builds_a_connection_for_a_first_offer() {
        assert_eq!(route_signal(OFFER, NOTHING), SignalRoute::Create);
    }

    /// One of the four named regression tests of §4.6: the old connection's teardown used
    /// to take the replacement with it, because a first offer for a peer that already had
    /// one was routed to the existing object.
    #[test]
    fn offer_glare_does_not_destroy_replacement() {
        // Both ends tried to open at once, or the far end gave up and started again. Its
        // new offer carries new ICE credentials and a new certificate, so answering it on
        // this end's abandoned attempt would be answering with the wrong session.
        assert_eq!(route_signal(OFFER, HALF_BUILT), SignalRoute::Create);
        assert_eq!(route_signal(OFFER, LIVE), SignalRoute::Create);
    }

    #[test]
    fn rebuilds_for_a_renegotiation_this_end_has_no_session_for() {
        // The marker says the far end thinks it is continuing something. If this end has
        // nothing to continue, believing it would answer from an empty connection.
        assert_eq!(route_signal(RENEGOTIATION, HALF_BUILT), SignalRoute::Create);
        assert_eq!(route_signal(RENEGOTIATION, NOTHING), SignalRoute::Create);
    }

    /// One of the four named regression tests of §4.6: only signals carrying a `type`
    /// used to be forwarded, so a trickled candidate — which has none — was dropped, and
    /// connections depended on whatever candidates happened to ride in the initial SDP.
    #[test]
    fn trickle_candidate_without_type_is_forwarded() {
        assert_eq!(
            route_signal(ANSWER_OR_CANDIDATE, LIVE),
            SignalRoute::Existing
        );
        assert_eq!(
            route_signal(ANSWER_OR_CANDIDATE, HALF_BUILT),
            SignalRoute::Existing
        );
    }

    #[test]
    fn drops_an_answer_or_a_candidate_with_nothing_to_apply_it_to() {
        // A candidate for a connection that has already been torn down. Applying it
        // somewhere else would be worse than losing it.
        assert_eq!(
            route_signal(ANSWER_OR_CANDIDATE, NOTHING),
            SignalRoute::Drop
        );
    }

    #[test]
    fn never_drops_an_offer() {
        // An offer is always actionable: either it continues a session or it starts one.
        // Dropping one leaves the far end waiting for an answer that never comes, and it
        // will keep offering until it gives up.
        for peer in [LIVE, HALF_BUILT, NOTHING] {
            for signal in [OFFER, RENEGOTIATION] {
                assert_ne!(route_signal(signal, peer), SignalRoute::Drop);
            }
        }
    }

    #[test]
    fn treats_an_unmarked_offer_as_a_first_offer_however_the_session_looks() {
        // A client older than the marker never sets it. Its renegotiations rebuild, which
        // is what they did before this existed — not ideal, and not a regression.
        assert_eq!(route_signal(OFFER, LIVE), SignalRoute::Create);
    }
}
