//! When a peer connection that failed mid-lobby gets rebuilt, and by which of the two
//! ends.
//!
//! A straight port of `src/renderer/reconnectPolicy.ts` and its tests. It is pure
//! arithmetic with no transport coupling, which is why it comes across unchanged: the
//! transport's only obligation is to report "closed" honestly.
//!
//! The connection itself is torn down by whichever side notices the failure, and both
//! ends then want it back. If both sent an offer they would each answer the other's and
//! neither would finish negotiating, so exactly one end offers: the one whose socket id
//! sorts higher. The other end still schedules an attempt, delayed by a grace period, for
//! the case where only one side saw anything go wrong.

use std::time::Duration;

/// The first delay, doubled per attempt.
pub const BASE_DELAY: Duration = Duration::from_secs(2);
/// The ceiling on that doubling.
pub const MAX_DELAY: Duration = Duration::from_secs(30);
/// What the answering end waits on top, so the two do not collide.
pub const ANSWER_GRACE: Duration = Duration::from_secs(6);
/// How many attempts the fast burst makes before it slows down.
///
/// Not how many are worth making. See [`SLOW_DELAY`].
pub const MAX_ATTEMPTS: u32 = 6;

/// After this many failed attempts, stop trying to reach the peer directly.
///
/// A direct path that failed twice will keep failing: the reason is the network between
/// the two ends, and waiting longer does not change it. Symmetric NAT and carrier-grade
/// NAT are the usual ones, and no amount of STUN gets through either — a relay does.
///
/// Two, not one: a single failure is often a lost packet or a peer that was still
/// starting up, and routing a whole lobby through a relay that was not needed costs the
/// relay's bandwidth and adds a hop of latency to every voice.
pub const RELAY_AFTER: u32 = 2;

/// How long to wait between attempts once the fast burst is spent.
///
/// This is relay rule four of §4.6, and the reason [`should_give_up`] does not mean what
/// its name suggests. The burst is six attempts over about ninety seconds, and it used to
/// be the end: after it, that player was silent for the rest of the round however the
/// situation changed around them. That was the difference between a bad minute and a
/// ruined evening, because the reasons a connection cannot be made are frequently not
/// permanent.
///
/// The one that prompted it: a relay grants a limited number of reservations, and when
/// they are all taken the next request is refused outright. Somebody leaving frees one —
/// and nothing was ever going to ask again.
///
/// Forty-five seconds is often enough to catch a change within the round and rare enough
/// that a genuinely unreachable peer costs almost nothing to keep trying.
pub const SLOW_DELAY: Duration = Duration::from_secs(45);

/// Whether this end is the one that sends the offer when rebuilding.
#[must_use]
pub fn initiates_reconnect(own_socket_id: &str, peer_socket_id: &str) -> bool {
    own_socket_id > peer_socket_id
}

/// Delay before the given attempt, counted from 1.
///
/// It doubles per attempt so a peer that is simply unreachable is not retried in a tight
/// loop.
#[must_use]
pub fn reconnect_delay(attempt: u32, initiates: bool) -> Duration {
    let doublings = attempt.saturating_sub(1).min(u32::BITS - 1);
    let backoff = BASE_DELAY
        .checked_mul(1u32 << doublings)
        .unwrap_or(MAX_DELAY)
        .min(MAX_DELAY);
    if initiates {
        backoff
    } else {
        backoff + ANSWER_GRACE
    }
}

/// Whether the fast burst of attempts is spent.
///
/// **Not whether to stop.** The name is carried over from `reconnectPolicy.ts`, where it
/// once meant exactly that and no longer does: after this returns true the caller keeps
/// trying at [`SLOW_DELAY`]. A port that read the old name and stopped here would
/// reintroduce the behaviour 1.0.4 removed and relay rule four forbids.
#[must_use]
pub fn should_give_up(attempt: u32) -> bool {
    attempt > MAX_ATTEMPTS
}

/// Whether a direct connection has failed often enough to be worth giving up on.
///
/// The caller still has to have a relay to escalate to; without one this says nothing
/// useful, which is itself worth logging, because a lobby that cannot reach anyone and
/// has no relay advertised is a server configuration problem rather than a client one.
#[must_use]
pub fn should_force_relay(attempt: u32) -> bool {
    attempt >= RELAY_AFTER
}

/// What the connection itself has told us about the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelaySignals {
    /// Which attempt this is, counted from 1.
    pub attempt: u32,
    /// How many relay candidates the last attempt gathered.
    ///
    /// `None` means nothing was observed — no failure yet, or a peer built before this
    /// client counted them.
    pub relay_candidates: Option<u32>,
    /// Whether any other peer in this lobby has already needed the relay.
    pub other_peers_needed_relay: bool,
}

/// Whether the next attempt to one peer should be forced through the relay.
///
/// [`should_force_relay`] alone waits for two failures, and each costs a connect timeout
/// plus a backoff — the better part of a minute during which a player is simply missing
/// from the conversation. Two things let it decide sooner, and both come from evidence
/// the connection itself produced.
///
/// **Relay candidates above zero means the relay answered.** The allocation succeeded, so
/// the relay is reachable from this machine, and the direct path failed anyway. There is
/// nothing to learn from failing at it a second time.
///
/// **Zero means the allocation failed**, and forcing relay-only would be worse than doing
/// nothing: with no relay candidate to offer there would be no candidate at all, so a
/// connection that sometimes succeeds directly would stop succeeding ever. This is relay
/// rule three again, arrived at from the other direction — and it is checked before the
/// lobby-wide signal, because a relay that works for somebody else does not work here.
///
/// **`other_peers_needed_relay` carries the lobby's experience across peers.** What
/// blocks a direct path is almost always the network at one end, not the pair, so the
/// second peer to fail is evidence about the eleventh.
#[must_use]
pub fn should_use_relay(signals: RelaySignals) -> bool {
    if signals.relay_candidates == Some(0) {
        return false;
    }
    if signals.other_peers_needed_relay {
        return true;
    }
    if signals
        .relay_candidates
        .is_some_and(|gathered| gathered > 0)
    {
        return true;
    }
    should_force_relay(signals.attempt)
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn exactly_one_end_offers() {
        assert!(initiates_reconnect("b", "a"));
        assert!(!initiates_reconnect("a", "b"));
        // Two ends that both think they should offer would deadlock negotiating, and two
        // that both wait would never reconnect.
        assert!(!initiates_reconnect("a", "a"));
    }

    #[test]
    fn the_delay_doubles_and_then_stops() {
        assert_eq!(reconnect_delay(1, true), Duration::from_secs(2));
        assert_eq!(reconnect_delay(2, true), Duration::from_secs(4));
        assert_eq!(reconnect_delay(3, true), Duration::from_secs(8));
        assert_eq!(reconnect_delay(4, true), Duration::from_secs(16));
        assert_eq!(reconnect_delay(5, true), MAX_DELAY);
        assert_eq!(reconnect_delay(6, true), MAX_DELAY);
    }

    #[test]
    fn the_answering_end_waits_longer() {
        assert_eq!(
            reconnect_delay(1, false),
            Duration::from_secs(2) + ANSWER_GRACE
        );
        assert_eq!(reconnect_delay(9, false), MAX_DELAY + ANSWER_GRACE);
    }

    #[test]
    fn a_far_out_attempt_does_not_overflow_the_shift() {
        // The TypeScript used floating point and saturated harmlessly. `1 << 64` is
        // undefined-ish arithmetic in Rust and panics in debug, so the port has to bound
        // the shift rather than the product.
        assert_eq!(reconnect_delay(100, true), MAX_DELAY);
        assert_eq!(reconnect_delay(u32::MAX, true), MAX_DELAY);
    }

    #[test]
    fn the_fast_burst_is_six_attempts() {
        assert!(!should_give_up(MAX_ATTEMPTS));
        assert!(should_give_up(MAX_ATTEMPTS + 1));
    }

    /// Socket.IO ids as they actually look, including two pairs that differ only late in
    /// the string — a comparison that looked at a prefix would pass the simple cases.
    const IDS: [&str; 6] = [
        "0NNRYKaxTPXTusamAAAD",
        "L3irzfbdl-cdX4KIAAAH",
        "qyocGexuH_Xzr19DAAAL",
        "U-fi5qR0rG4EsTRgAAAX",
        "aaaaaaaaaaaaaaaaAAAA",
        "aaaaaaaaaaaaaaaaAAAB",
    ];

    #[test]
    fn exactly_one_end_offers_for_every_pair_of_real_ids() {
        for a in IDS {
            for b in IDS {
                if a == b {
                    continue;
                }
                // Both ends run this with their own id first. Precisely one may offer, or
                // they would answer each other and neither connection would come up.
                assert_ne!(
                    initiates_reconnect(a, b),
                    initiates_reconnect(b, a),
                    "{a} and {b}"
                );
            }
        }
    }

    #[test]
    fn the_answering_end_keeps_its_lead_across_the_whole_burst() {
        for attempt in 1..=MAX_ATTEMPTS {
            let lead = reconnect_delay(attempt, false)
                .checked_sub(reconnect_delay(attempt, true))
                .unwrap();
            assert_eq!(lead, ANSWER_GRACE);
        }
    }

    #[test]
    fn the_grace_outlasts_a_round_trip() {
        // It has to outlast a round trip through the signalling server plus ICE, or both
        // ends rebuild and one connection is thrown away every time.
        assert!(ANSWER_GRACE >= Duration::from_secs(5));
    }

    fn signals() -> RelaySignals {
        RelaySignals {
            attempt: 1,
            relay_candidates: None,
            other_peers_needed_relay: false,
        }
    }

    #[test]
    fn goes_to_the_relay_on_the_first_failure_when_the_relay_answered() {
        // The allocation succeeded and the direct path failed anyway. Failing at it a
        // second time takes the better part of a minute and teaches nothing, and the
        // player is missing from the conversation for all of it.
        assert!(should_use_relay(RelaySignals {
            relay_candidates: Some(2),
            ..signals()
        }));
    }

    #[test]
    fn never_forces_the_relay_when_the_allocation_produced_nothing() {
        // The trap. With no relay candidate to offer, relay-only leaves the connection
        // with no candidates at all — so a peer that sometimes connects directly would
        // stop connecting ever. This is the one case where the obvious escalation makes
        // things worse.
        for attempt in [1, 2, 3, 8] {
            assert!(!should_use_relay(RelaySignals {
                attempt,
                relay_candidates: Some(0),
                ..signals()
            }));
        }
    }

    #[test]
    fn does_not_let_a_working_relay_elsewhere_override_a_failed_allocation_here() {
        // A relay that works for somebody else does not work from this machine, and the
        // lobby-wide signal must not talk this into a configuration that cannot connect.
        assert!(!should_use_relay(RelaySignals {
            attempt: 5,
            relay_candidates: Some(0),
            other_peers_needed_relay: true,
        }));
    }

    #[test]
    fn starts_later_peers_on_the_relay_once_one_has_needed_it() {
        // What blocks a direct path is the network at one end, not the pair. The second
        // peer to fail is evidence about the eleventh, and rediscovering it per peer costs
        // a minute each.
        assert!(should_use_relay(RelaySignals {
            other_peers_needed_relay: true,
            ..signals()
        }));
    }

    #[test]
    fn falls_back_to_the_attempt_count_when_nothing_was_observed() {
        // No failure yet, or a peer built before this client counted candidates. This is
        // the behaviour that existed before, and it has to stay reachable.
        assert!(!should_use_relay(signals()));
        assert!(should_use_relay(RelaySignals {
            attempt: RELAY_AFTER,
            ..signals()
        }));
    }

    #[test]
    fn agrees_with_should_force_relay_when_it_has_nothing_else_to_go_on() {
        for attempt in 1..=6 {
            assert_eq!(
                should_use_relay(RelaySignals {
                    attempt,
                    ..signals()
                }),
                should_force_relay(attempt),
            );
        }
    }

    #[test]
    fn the_slow_delay_is_longer_than_any_delay_in_the_fast_burst() {
        // The burst is meant to be over by the time this takes over. If it were shorter
        // the two would interleave and "still trying, slowly" would be a lie.
        assert!(SLOW_DELAY > MAX_DELAY);
    }

    #[test]
    fn the_slow_delay_is_short_enough_to_catch_a_change_within_a_round() {
        // The case it exists for: a relay with no reservations left frees one when
        // somebody leaves the lobby. A round lasts minutes, so an interval measured in
        // minutes would miss it.
        assert!(SLOW_DELAY <= Duration::from_secs(60));
    }

    #[test]
    fn the_burst_reaches_its_cap_before_the_slow_interval_starts() {
        // The burst has to finish backing off before the flat interval begins, or the
        // escalation to the relay never happens at its intended attempt.
        assert_eq!(reconnect_delay(MAX_ATTEMPTS, true), MAX_DELAY);
    }

    #[test]
    fn the_burst_being_spent_is_not_a_reason_to_stop() {
        // Relay rule four. `should_give_up` reads like a stop and is not one: after it
        // the caller waits `SLOW_DELAY` and tries again, for as long as the round lasts.
        // A port that stopped here would reintroduce what 1.0.4 removed.
        assert!(should_give_up(MAX_ATTEMPTS + 1));
        assert!(SLOW_DELAY > Duration::ZERO);
    }
}
