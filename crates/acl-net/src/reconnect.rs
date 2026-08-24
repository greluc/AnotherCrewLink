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
/// How many attempts are worth making.
pub const MAX_ATTEMPTS: u32 = 6;

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

/// Whether the attempt number is past what is worth trying.
#[must_use]
pub fn should_give_up(attempt: u32) -> bool {
    attempt > MAX_ATTEMPTS
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
    fn stops_after_six_attempts() {
        assert!(!should_give_up(MAX_ATTEMPTS));
        assert!(should_give_up(MAX_ATTEMPTS + 1));
    }
}
