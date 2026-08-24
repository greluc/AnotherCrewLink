//! Turning what the far end reports about loss into redundancy on the wire.
//!
//! This is the sending half of §3e. The receiving half lives in [`crate::jitter`], which
//! holds packet *N+1* long enough to reconstruct *N* out of it.
//!
//! # Why a controller and not a wire
//!
//! `set_inband_fec(true)` on its own emits nothing. libopus spends bits on redundancy in
//! proportion to the loss percentage it has been told about, and that number can only come
//! from the far end, in RTCP receiver reports. A client that sets the flag and never calls
//! `OPUS_SET_PACKET_LOSS_PERC` has error correction that is switched on and doing nothing —
//! which is the exact failure §3e exists to prevent, and it is invisible: the flag reads
//! back as set, the packets look normal, and the only symptom is that a lossy call sounds
//! broken when it did not have to.
//!
//! Passing the reported number straight through would be worse than not trying:
//!
//! - **Reports are noisy.** One interval that happens to straddle a burst reports loss the
//!   next does not. Following each report exactly makes the encoder change its bit
//!   allocation every couple of seconds, and each change is audible as the bitrate
//!   available to the actual speech moves under it.
//! - **The number comes off the network.** It is a byte a peer chose. A peer reporting
//!   100% loss would have libopus spend most of the budget carrying copies of frames
//!   nobody lost, and the peer that sent it is not the one who suffers.
//! - **Loss arrives in bursts and leaves gradually.** Wireless interference, a congested
//!   uplink and a neighbour's microwave all come back. Dropping protection the moment one
//!   clean report arrives means the next burst is unprotected.
//!
//! So: rise quickly, fall slowly, clamp hard, and only disturb the encoder when the
//! difference is worth the disturbance.
//!
//! # What this does not do
//!
//! It does not touch the bitrate. §3e is explicit that there is to be no ladder: below
//! roughly 16–20 kbps libopus carries no meaningful redundancy at all, so a ladder's
//! bottom rung would switch this loop off exactly when it is needed. `codec::Encoder`
//! leaves libopus at its own choice for the configuration, which sits above that floor —
//! there is a test that says so, because it is a silent precondition for everything here.

/// The most loss this will ever report to the encoder, as a percentage.
///
/// Not 100. Above roughly a quarter, libopus is spending more of the budget on copies of
/// frames than on the speech itself, and a call at that loss rate has worse problems than
/// its error correction. The clamp also means a peer that lies — or one whose own receive
/// path is broken and reports loss that is not there — can degrade its own audio and
/// nobody else's.
pub const MAX_APPLIED: u8 = 25;

/// How far the estimate must sit from what the encoder was last told before it is worth
/// telling it again.
///
/// Every change re-plans libopus's bit allocation. Chasing a one-point move costs more
/// than it buys.
const DEAD_BAND: u8 = 3;

/// Weight given to a new report that is worse than the current estimate.
///
/// High, deliberately. Loss that has started is a problem now, and the cost of
/// over-reacting to one bad report is some wasted bitrate for a few seconds.
const RISE: f32 = 0.6;

/// Weight given to a new report that is better than the current estimate.
///
/// Low, equally deliberately. Loss leaves gradually and comes back; a single clean report
/// is not evidence that the interference has gone.
const FALL: f32 = 0.12;

/// Decides what to tell the Opus encoder about loss.
///
/// Feed it every receiver report. It returns `Some(percent)` only when the encoder should
/// actually be told, so the caller can pass the result straight to
/// [`crate::codec::Encoder::set_packet_loss`] and do nothing the rest of the time.
#[derive(Debug, Clone)]
pub struct FecController {
    /// The smoothed estimate, as a percentage. Fractional, so slow decay is not rounded
    /// away one report at a time.
    smoothed: f32,
    /// What the encoder was last told.
    applied: u8,
}

impl Default for FecController {
    fn default() -> Self {
        Self::new()
    }
}

impl FecController {
    /// A controller for a peer that has not reported anything yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            smoothed: 0.0,
            applied: 0,
        }
    }

    /// Takes one receiver report's `fraction lost` field.
    ///
    /// RFC 3550 §6.4.1 carries it as 8-bit fixed point: the loss fraction since the last
    /// report, multiplied by 256 and truncated. So 26 is about 10%.
    ///
    /// Returns the percentage to give the encoder, or `None` when it is not worth
    /// changing.
    pub fn observe_fraction_lost(&mut self, fraction_lost: u8) -> Option<u8> {
        self.observe_percent(f32::from(fraction_lost) * 100.0 / 256.0)
    }

    /// Takes an already-computed loss percentage.
    ///
    /// Anything not finite is ignored rather than allowed to poison the estimate: this
    /// number is derived from counters that came off the network, and one NaN would make
    /// every later comparison false and freeze the controller for the rest of the call.
    pub fn observe_percent(&mut self, percent: f32) -> Option<u8> {
        if !percent.is_finite() {
            return None;
        }
        let observed = percent.clamp(0.0, 100.0);
        let weight = if observed > self.smoothed { RISE } else { FALL };
        self.smoothed += (observed - self.smoothed) * weight;
        self.settle()
    }

    /// Records that a report interval passed with no report in it.
    ///
    /// Silence is not evidence that the loss stopped -- it is just as likely to mean the
    /// path got worse -- so this decays at the same slow rate as a good report rather than
    /// dropping protection. What it prevents is a controller that keeps paying for
    /// redundancy forever because the peer that reported the loss has left.
    pub fn idle(&mut self) -> Option<u8> {
        self.smoothed -= self.smoothed * FALL;
        self.settle()
    }

    /// What the encoder was last told.
    #[must_use]
    pub const fn applied(&self) -> u8 {
        self.applied
    }

    /// The current estimate before clamping, for tests and logging.
    #[must_use]
    pub const fn estimate(&self) -> f32 {
        self.smoothed
    }

    /// Applies the dead band and the clamp, and reports whether anything changed.
    fn settle(&mut self) -> Option<u8> {
        // `round` rather than a cast: a cast truncates, so an estimate creeping from 2.9
        // to 3.9 would read as one point of movement instead of two.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let target = (self.smoothed.round().clamp(0.0, f32::from(MAX_APPLIED))) as u8;

        // Zero is exempt from the dead band in one direction: going from any protection to
        // none is worth doing exactly, or a call that has been clean for a minute keeps
        // paying for redundancy it does not need.
        let far_enough =
            target.abs_diff(self.applied) >= DEAD_BAND || (target == 0 && self.applied != 0);
        if !far_enough {
            return None;
        }
        self.applied = target;
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// Feeds the same report `count` times and returns the last value applied.
    fn sustained(controller: &mut FecController, percent: f32, count: usize) -> u8 {
        for _ in 0..count {
            controller.observe_percent(percent);
        }
        controller.applied()
    }

    #[test]
    fn a_clean_call_tells_the_encoder_nothing() {
        // The common case, and the one that must cost nothing: no loss, no calls into
        // libopus, no bits spent on redundancy.
        let mut controller = FecController::new();
        for _ in 0..50 {
            assert_eq!(controller.observe_fraction_lost(0), None);
        }
        assert_eq!(controller.applied(), 0);
    }

    #[test]
    fn loss_reaches_the_encoder() {
        // The whole point. Without this the error-correction flag is set and idle.
        //
        // Near ten rather than exactly ten: the dead band deliberately stops chasing the
        // last point or two, and telling libopus 9 where the truth is 10 changes nothing
        // it does. A test that demanded the exact figure would be testing the absence of
        // the dead band.
        let applied = sustained(&mut FecController::new(), 10.0, 10);
        assert!(
            (8..=10).contains(&applied),
            "settled at {applied}% for a 10% loss"
        );
    }

    #[test]
    fn it_rises_faster_than_it_falls() {
        // Loss that has started is a problem now; loss that appears to have stopped may
        // just be between bursts.
        let mut rising = FecController::new();
        rising.observe_percent(10.0);
        let after_one_bad = rising.estimate();

        let mut falling = FecController::new();
        sustained(&mut falling, 10.0, 20);
        let settled = falling.estimate();
        falling.observe_percent(0.0);
        let dropped = settled - falling.estimate();

        assert!(
            after_one_bad > dropped,
            "one bad report moved {after_one_bad:.2}, one good report moved {dropped:.2}"
        );
    }

    #[test]
    fn one_outlier_does_not_swing_it_to_the_top() {
        // A single interval that straddles a burst is not a 40% call.
        let mut controller = FecController::new();
        controller.observe_percent(40.0);
        assert!(
            controller.applied() < 30,
            "one report took it to {}",
            controller.applied()
        );
    }

    #[test]
    fn a_lying_peer_cannot_drive_the_encoder_anywhere_harmful() {
        // The number is a byte a peer chose. At 100% libopus would spend most of the
        // budget carrying copies of frames nobody lost.
        let mut controller = FecController::new();
        assert_eq!(sustained(&mut controller, 100.0, 200), MAX_APPLIED);

        // And through the RTCP field, where 255 is the largest value that fits.
        let mut through_rtcp = FecController::new();
        for _ in 0..200 {
            through_rtcp.observe_fraction_lost(255);
        }
        assert_eq!(through_rtcp.applied(), MAX_APPLIED);
    }

    #[test]
    fn the_rtcp_fraction_is_read_as_fixed_point() {
        // RFC 3550 §6.4.1: the loss fraction times 256, truncated. Reading it as a
        // percentage directly would report 26% where the peer meant 10%.
        let mut controller = FecController::new();
        for _ in 0..60 {
            controller.observe_fraction_lost(26);
        }
        assert!(
            (9..=11).contains(&controller.applied()),
            "26/256 should settle near 10%, got {}",
            controller.applied()
        );
    }

    #[test]
    fn a_settled_call_stops_disturbing_the_encoder() {
        // Every change re-plans libopus's bit allocation, and the plan is what the speech
        // is competing for. A controller that reported every interval would be worse than
        // one that reported none.
        let mut controller = FecController::new();
        sustained(&mut controller, 8.0, 30);
        let mut changes = 0;
        for _ in 0..100 {
            if controller.observe_percent(8.0).is_some() {
                changes += 1;
            }
        }
        assert_eq!(
            changes, 0,
            "a steady report changed the encoder {changes} times"
        );
    }

    #[test]
    fn small_wobbles_do_not_flap() {
        // Reports jitter by a point or two even on a stable path.
        let mut controller = FecController::new();
        sustained(&mut controller, 8.0, 30);
        let settled = controller.applied();
        let mut changes = 0;
        for (index, _) in (0..60).enumerate() {
            let wobble = if index % 2 == 0 { 7.0 } else { 9.0 };
            if controller.observe_percent(wobble).is_some() {
                changes += 1;
            }
        }
        assert_eq!(
            changes, 0,
            "wobbling around {settled}% changed the encoder {changes} times"
        );
    }

    #[test]
    fn a_real_change_does_get_through() {
        // The other half of the dead band: it must not be so wide that the controller
        // stops responding.
        let mut controller = FecController::new();
        sustained(&mut controller, 2.0, 30);
        let before = controller.applied();
        let after = sustained(&mut controller, 15.0, 30);
        assert!(
            after > before + 5,
            "{before}% -> {after}% is not a response"
        );
    }

    #[test]
    fn protection_is_given_up_completely_when_the_loss_goes() {
        // Not left at two or three percent forever because the dead band swallowed the
        // last step down.
        let mut controller = FecController::new();
        sustained(&mut controller, 12.0, 30);
        assert!(controller.applied() > 0);
        assert_eq!(sustained(&mut controller, 0.0, 200), 0);
    }

    #[test]
    fn a_peer_that_stops_reporting_stops_costing_bitrate() {
        // A peer that left, or whose reports are not arriving. Holding protection forever
        // would be paying for a correspondent who is not there.
        let mut controller = FecController::new();
        sustained(&mut controller, 12.0, 30);
        assert!(controller.applied() > 0);
        for _ in 0..200 {
            controller.idle();
        }
        assert_eq!(controller.applied(), 0);
    }

    #[test]
    fn silence_is_not_treated_as_good_news() {
        // A path that has gone quiet is at least as likely to have got worse. Decaying on
        // silence faster than on a clean report would drop protection precisely when the
        // evidence is weakest.
        let mut on_silence = FecController::new();
        sustained(&mut on_silence, 12.0, 30);
        let settled = on_silence.estimate();
        on_silence.idle();
        let quiet_drop = settled - on_silence.estimate();

        let mut on_good_news = FecController::new();
        sustained(&mut on_good_news, 12.0, 30);
        on_good_news.observe_percent(0.0);
        let reported_drop = settled - on_good_news.estimate();

        assert!(quiet_drop <= reported_drop + f32::EPSILON);
    }

    #[test]
    fn nonsense_is_ignored_rather_than_absorbed() {
        // These are derived from counters that came off the network. One NaN in the
        // estimate makes every later comparison false and freezes the controller for the
        // rest of the call.
        let mut controller = FecController::new();
        sustained(&mut controller, 10.0, 30);
        let before = controller.estimate();
        assert_eq!(controller.observe_percent(f32::NAN), None);
        assert_eq!(controller.observe_percent(f32::INFINITY), None);
        assert_eq!(controller.observe_percent(f32::NEG_INFINITY), None);
        assert!((controller.estimate() - before).abs() < f32::EPSILON);
        assert!(controller.estimate().is_finite());
    }

    #[test]
    fn a_negative_report_cannot_push_the_estimate_below_zero() {
        let mut controller = FecController::new();
        for _ in 0..50 {
            controller.observe_percent(-40.0);
        }
        assert!(controller.estimate() >= 0.0);
        assert_eq!(controller.applied(), 0);
    }
}
