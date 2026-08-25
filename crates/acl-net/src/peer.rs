//! What a peer connection has to decide, with no transport in it.
//!
//! The same split [`crate::client`] uses, and for the same reason: everything that can go
//! wrong in a hand-written peer goes wrong here rather than inside the `webrtc` crate,
//! where it would need two hosts and a network to reproduce. Five decisions live here —
//! when a candidate may be applied, whether an event still belongs to a live connection,
//! when a connection that never started is declared dead, what to do with a signal from a
//! socket nobody knows, and whether trouble on a live link costs a restart or a rebuild.
//!
//! Three of them are 1.0.0 bugs. §4.6 names those as regression tests because a port will
//! otherwise reintroduce them, and a test that needs a lobby to run is a test that does
//! not run. The fifth is 1.0.4's, and it is here for the same reason: a repair that
//! quietly does nothing is indistinguishable from the fault it was meant to fix.

use std::time::Duration;

/// How long a connection may sit in its initial state before it is given up on.
///
/// ICE that never starts produces no state change at all, so nothing fails on its own and
/// the peer waits for an event that is not coming. Chromium's own ICE failure takes
/// fifteen to thirty seconds to arrive; this is deliberately shorter than that, because a
/// connection that has not begun gathering has nothing to wait for.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The lifetime of one attempt at a connection to a peer.
///
/// `peer.ts` nulls all five event handlers before calling `pc.close()`, and that teardown
/// is how the 1.0.0 fixes stop a connection being replaced from acting on its own dying
/// events. The `webrtc` crate takes a single `Arc<dyn PeerConnectionEventHandler>` with no
/// per-event detach, so the same protection has to be a value the handler reads: it
/// carries the generation it was built for, and anything that does not match the current
/// one is from a connection that has already been replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(u64);

impl Generation {
    /// The generation a peer starts at.
    #[must_use]
    pub const fn first() -> Self {
        Self(0)
    }

    /// The next one, taken when a connection is replaced.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Why a connection ended, when it did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It never left its initial state within [`CONNECT_TIMEOUT`].
    NeverStarted,
    /// ICE reported failure.
    Failed,
    /// This end closed it, or it was replaced.
    Closed,
}

/// Whether an event from the `webrtc` crate should be acted on.
///
/// The handler cannot be detached, so it asks this instead. A stale generation is not an
/// error and is not logged as one: it is the ordinary consequence of replacing a
/// connection while its predecessor is still shutting down.
#[must_use]
pub fn is_current(event_generation: Generation, current: Generation) -> bool {
    event_generation == current
}

/// Holds candidates that arrive before there is anything to apply them to.
///
/// The `webrtc` crate refuses a candidate added before the remote description is set, and
/// the signalling server delivers them as they are gathered — so on a slow answer the
/// first candidates arrive first. Dropping them is what makes a connection depend on
/// whatever happened to ride inside the initial SDP, which is the same failure
/// `trickle_candidate_without_type_is_forwarded` guards from the other side.
#[derive(Debug, Default)]
pub struct CandidateQueue<C> {
    waiting: Vec<C>,
    open: bool,
}

impl<C> CandidateQueue<C> {
    /// A queue for a connection whose remote description has not been set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            waiting: Vec::new(),
            open: false,
        }
    }

    /// Offers a candidate.
    ///
    /// Returns it back when it may be applied now, and `None` when it has been held. A
    /// held candidate comes out of [`CandidateQueue::flush`].
    pub fn offer(&mut self, candidate: C) -> Option<C> {
        if self.open {
            return Some(candidate);
        }
        self.waiting.push(candidate);
        None
    }

    /// Records that the remote description is set, and returns everything held until now.
    ///
    /// Calling it twice is not an error — a renegotiation sets a second remote description
    /// on a connection whose queue is already open, and there is nothing left to flush.
    pub fn flush(&mut self) -> Vec<C> {
        self.open = true;
        std::mem::take(&mut self.waiting)
    }

    /// How many candidates are held. For tests, and for a log line worth having.
    #[must_use]
    pub fn held(&self) -> usize {
        self.waiting.len()
    }

    /// Whether candidates may be applied directly.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }
}

/// What the connection state machine says to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Nothing to do.
    Wait,
    /// The connection is up.
    Connected,
    /// Give up on this attempt, for the given reason.
    GiveUp(Ended),
}

/// Where one attempt has got to.
///
/// Deliberately not the `webrtc` crate's state enum. This one answers a different
/// question — has anything happened at all yet — and it is the answer
/// `connection_stuck_in_new_times_out` needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing has happened. ICE has not reported starting.
    New,
    /// ICE is doing something.
    Connecting,
    /// Media can flow.
    Connected,
    /// Over.
    Ended(Ended),
}

/// One attempt's progress, with the timeout that makes a silent failure visible.
///
/// `elapsed` is passed in rather than read from a clock, so the timeout is testable
/// without waiting for it. That is the same reason [`crate::reconnect`] returns a
/// [`Duration`] instead of sleeping.
#[derive(Clone, Copy, Debug)]
pub struct Attempt {
    phase: Phase,
}

impl Default for Attempt {
    fn default() -> Self {
        Self::new()
    }
}

impl Attempt {
    /// A connection that has just been created and has done nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self { phase: Phase::New }
    }

    /// Where it is.
    #[must_use]
    pub const fn phase(self) -> Phase {
        self.phase
    }

    /// Records that ICE started doing something.
    pub const fn started(&mut self) {
        if matches!(self.phase, Phase::New) {
            self.phase = Phase::Connecting;
        }
    }

    /// Records that the connection came up.
    pub const fn connected(&mut self) {
        self.phase = Phase::Connected;
    }

    /// Records that it ended, for a reason the transport reported.
    pub const fn ended(&mut self, reason: Ended) {
        self.phase = Phase::Ended(reason);
    }

    /// What to do, given how long this attempt has been running.
    ///
    /// The timeout applies only to [`Phase::New`]. A connection that reached
    /// `Connecting` is being worked on by ICE, and ICE has its own, longer failure —
    /// cutting that short would throw away connections that were about to succeed on a
    /// slow network, which is exactly who needs them.
    #[must_use]
    pub fn poll(self, elapsed: Duration) -> Progress {
        match self.phase {
            Phase::New if elapsed >= CONNECT_TIMEOUT => Progress::GiveUp(Ended::NeverStarted),
            Phase::New | Phase::Connecting => Progress::Wait,
            Phase::Connected => Progress::Connected,
            Phase::Ended(reason) => Progress::GiveUp(reason),
        }
    }
}

/// How long a connection may sit disconnected before ICE is restarted.
///
/// `disconnected` means connectivity checks have stopped succeeding but ICE has not given
/// up. Sometimes it heals on its own — a wifi roam, a moment of congestion — and tearing
/// anything down for that would be worse than waiting. Sometimes it does not, and the
/// stack then takes fifteen to thirty seconds to admit it by moving to `failed`. For all
/// of that time the player is silent.
///
/// Four seconds is longer than the transient cases take and much shorter than the wait
/// for `failed`. A restart re-gathers — including a fresh relay allocation — and keeps
/// the connection, its tracks and its DTLS session, which is a far cheaper repair than
/// the rebuild a failure costs.
pub const ICE_RESTART_AFTER_DISCONNECTED: Duration = Duration::from_secs(4);

/// What the transport reports about a link that is already up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// Media is flowing.
    Connected,
    /// Checks have stopped succeeding, but ICE has not given up.
    Disconnected,
    /// ICE gave up.
    Failed,
}

/// The cheapest repair that fits what went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    /// Leave it alone.
    None,
    /// Re-gather, keeping the connection, its tracks and its DTLS session.
    RestartIce,
    /// Tear it down and build another.
    Rebuild,
}

/// Decides between an ICE restart and a rebuild.
///
/// A rebuild is not the first response to trouble, and §4.6 says so: only a failure
/// costs one. The two rules that are not obvious from either stack's API are both here.
///
/// **Only the initiator restarts.** A restart works by making the next offer carry fresh
/// ICE credentials, so an end that does not offer cannot perform one.
///
/// **One restart per connection.** A path that is genuinely gone returns to disconnected
/// immediately afterwards, and restarting on a loop would re-gather each time — taking a
/// relay allocation per attempt from a server that grants a finite number — instead of
/// letting it fail once and be rebuilt.
#[derive(Clone, Copy, Debug, Default)]
pub struct RepairPolicy {
    restarted: bool,
}

impl RepairPolicy {
    /// A policy for a connection that has not been repaired yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { restarted: false }
    }

    /// Whether this connection has already spent its one restart.
    #[must_use]
    pub const fn has_restarted(self) -> bool {
        self.restarted
    }

    /// What to do, given the state, how long the link has been in it, and whether this
    /// end is the one that offers.
    pub const fn poll(&mut self, state: LinkState, held: Duration, initiator: bool) -> Repair {
        match state {
            // Whichever end notices. Both will, and the reconnect policy decides which of
            // them offers the replacement.
            LinkState::Failed => Repair::Rebuild,
            LinkState::Disconnected
                if initiator
                    && !self.restarted
                    && held.as_millis() >= ICE_RESTART_AFTER_DISCONNECTED.as_millis() =>
            {
                self.restarted = true;
                Repair::RestartIce
            }
            LinkState::Disconnected | LinkState::Connected => Repair::None,
        }
    }
}

/// What to do with a signal whose sender the mesh does not recognise.
///
/// The server sends `{ data, from }` and nothing else. The 1.0.0 client destructured a
/// `client` that was never there, so every use of it was `undefined`: the cleanup that
/// should have removed a stale audio element ran against nothing, and whether that threw
/// depended on which field was touched first. The fix is not to make the lookup succeed —
/// there is genuinely no such peer — it is to say so and carry on.
#[must_use]
pub fn accepts_signal_from(known_peers: &[&str], from: &str) -> bool {
    known_peers.contains(&from)
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_candidate_before_the_remote_description_is_held_not_dropped() {
        let mut queue = CandidateQueue::new();
        assert_eq!(queue.offer("a"), None);
        assert_eq!(queue.offer("b"), None);
        assert_eq!(queue.held(), 2);
        assert!(!queue.is_open());
        assert_eq!(queue.flush(), ["a", "b"]);
        assert_eq!(queue.held(), 0);
    }

    #[test]
    fn a_candidate_after_the_remote_description_goes_straight_through() {
        let mut queue = CandidateQueue::new();
        queue.flush();
        assert!(queue.is_open());
        assert_eq!(queue.offer("a"), Some("a"));
        assert_eq!(queue.held(), 0);
    }

    #[test]
    fn flushing_twice_is_not_an_error() {
        // A renegotiation sets a second remote description on a connection whose queue is
        // already open. There is nothing to flush and nothing to complain about.
        let mut queue: CandidateQueue<&str> = CandidateQueue::new();
        assert!(queue.flush().is_empty());
        assert!(queue.flush().is_empty());
        assert!(queue.is_open());
    }

    #[test]
    fn the_queue_keeps_the_order_it_was_given() {
        // ICE tries candidates in the order it receives them, and the first one gathered
        // is usually the host candidate that connects on a LAN.
        let mut queue = CandidateQueue::new();
        for candidate in ["host", "srflx", "relay"] {
            queue.offer(candidate);
        }
        assert_eq!(queue.flush(), ["host", "srflx", "relay"]);
    }

    /// One of the four named regression tests of §4.6. ICE never starts, so no state
    /// change is ever reported and the connection never fails on its own — the peer waits
    /// for an event that is not coming.
    #[test]
    fn connection_stuck_in_new_times_out() {
        let attempt = Attempt::new();
        assert_eq!(attempt.poll(Duration::ZERO), Progress::Wait);
        assert_eq!(
            attempt.poll(
                CONNECT_TIMEOUT
                    .checked_sub(Duration::from_millis(1))
                    .unwrap()
            ),
            Progress::Wait
        );
        assert_eq!(
            attempt.poll(CONNECT_TIMEOUT),
            Progress::GiveUp(Ended::NeverStarted)
        );
    }

    #[test]
    fn a_connection_that_started_is_left_to_ice() {
        // ICE's own failure takes fifteen to thirty seconds. Cutting that short throws
        // away connections that were about to succeed on a slow network, which is exactly
        // the network whose players need them.
        let mut attempt = Attempt::new();
        attempt.started();
        assert_eq!(attempt.poll(Duration::from_secs(600)), Progress::Wait);
    }

    #[test]
    fn started_does_not_undo_connected() {
        // The two arrive out of order often enough to matter: a late `checking` after
        // `connected` would otherwise reopen the timeout on a working connection.
        let mut attempt = Attempt::new();
        attempt.connected();
        attempt.started();
        assert_eq!(attempt.poll(Duration::from_secs(600)), Progress::Connected);
    }

    #[test]
    fn a_failure_is_reported_however_long_it_took() {
        let mut attempt = Attempt::new();
        attempt.ended(Ended::Failed);
        assert_eq!(
            attempt.poll(Duration::ZERO),
            Progress::GiveUp(Ended::Failed)
        );
    }

    /// One of the four named regression tests of §4.6. The server sends `{ data, from }`;
    /// the 1.0.0 client destructured a `client` that was never there, so the cleanup ran
    /// against `undefined`.
    #[test]
    fn signal_from_unknown_socket_is_ignored_not_crashed() {
        let known = ["alice", "bob"];
        assert!(accepts_signal_from(&known, "alice"));
        assert!(!accepts_signal_from(&known, "mallory"));
        // The empty lobby is the case that used to reach the destructuring first.
        assert!(!accepts_signal_from(&[], "alice"));
    }

    #[test]
    fn a_disconnected_link_is_left_alone_for_four_seconds() {
        // Transient cases -- a wifi roam, a moment of congestion -- heal on their own, and
        // re-gathering for one costs a relay allocation for nothing.
        let mut policy = RepairPolicy::new();
        assert_eq!(
            policy.poll(LinkState::Disconnected, Duration::ZERO, true),
            Repair::None
        );
        assert_eq!(
            policy.poll(
                LinkState::Disconnected,
                ICE_RESTART_AFTER_DISCONNECTED
                    .checked_sub(Duration::from_millis(1))
                    .unwrap(),
                true
            ),
            Repair::None
        );
        assert_eq!(
            policy.poll(
                LinkState::Disconnected,
                ICE_RESTART_AFTER_DISCONNECTED,
                true
            ),
            Repair::RestartIce
        );
    }

    #[test]
    fn only_the_initiator_restarts() {
        // A restart works by making the next offer carry fresh ICE credentials. An end
        // that does not offer cannot perform one, and trying would be a silent no-op --
        // which looks exactly like the fault it is meant to repair.
        let mut policy = RepairPolicy::new();
        assert_eq!(
            policy.poll(LinkState::Disconnected, Duration::from_secs(60), false),
            Repair::None
        );
        assert!(!policy.has_restarted());
    }

    #[test]
    fn one_restart_per_connection() {
        // A path that is genuinely gone returns to disconnected immediately afterwards.
        // Restarting on a loop re-gathers each time, taking a relay allocation per attempt
        // from a server that grants a finite number, instead of failing once and being
        // rebuilt.
        let mut policy = RepairPolicy::new();
        assert_eq!(
            policy.poll(
                LinkState::Disconnected,
                ICE_RESTART_AFTER_DISCONNECTED,
                true
            ),
            Repair::RestartIce
        );
        assert_eq!(
            policy.poll(LinkState::Disconnected, Duration::from_secs(600), true),
            Repair::None
        );
    }

    #[test]
    fn a_recovery_does_not_return_the_restart() {
        // The budget is per connection, not per disconnection. A link that flaps would
        // otherwise re-gather on every dip.
        let mut policy = RepairPolicy::new();
        policy.poll(
            LinkState::Disconnected,
            ICE_RESTART_AFTER_DISCONNECTED,
            true,
        );
        assert_eq!(
            policy.poll(LinkState::Connected, Duration::ZERO, true),
            Repair::None
        );
        assert_eq!(
            policy.poll(LinkState::Disconnected, Duration::from_secs(600), true),
            Repair::None
        );
    }

    #[test]
    fn only_a_failure_costs_a_rebuild() {
        // The whole point of the restart: it keeps the connection, its tracks and its
        // DTLS session. A rebuild throws all three away.
        let mut policy = RepairPolicy::new();
        assert_eq!(
            policy.poll(LinkState::Failed, Duration::ZERO, true),
            Repair::Rebuild
        );
        // Either end may notice, and both will.
        assert_eq!(
            RepairPolicy::new().poll(LinkState::Failed, Duration::ZERO, false),
            Repair::Rebuild
        );
    }

    #[test]
    fn a_connected_link_is_never_repaired() {
        let mut policy = RepairPolicy::new();
        assert_eq!(
            policy.poll(LinkState::Connected, Duration::from_secs(3600), true),
            Repair::None
        );
    }

    #[test]
    fn an_event_from_a_replaced_connection_is_ignored() {
        let first = Generation::first();
        let second = first.next();
        assert!(is_current(second, second));
        // The old connection is still shutting down and still emitting. Acting on this is
        // how the replacement got torn down in 1.0.0.
        assert!(!is_current(first, second));
    }

    #[test]
    fn generations_do_not_repeat_within_one_session() {
        let mut seen = Generation::first();
        for _ in 0..1000 {
            let next = seen.next();
            assert_ne!(next, seen);
            seen = next;
        }
    }
}
