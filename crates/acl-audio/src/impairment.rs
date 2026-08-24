//! A network that is not perfect, reproducibly.
//!
//! Gate G2's third criterion measures the receive path under loss, jitter, reordering and
//! a freeze, and a measurement is only worth anything if the impairment is the same every
//! time. So this carries its own generator rather than calling a random number source: the
//! same profile and the same seed produce the same packets dropped, in the same order,
//! with the same delays, on every machine.
//!
//! # What the profiles are
//!
//! The plan's: loss at 0, 1, 2, 5 and 10 percent, jitter at 0, 20, 50 and 100 milliseconds,
//! reordering at 0, 1 and 5 percent, and one 500 ms freeze. They are not arbitrary — 5%
//! loss is where a conversation starts to break down, and a 500 ms freeze is what a
//! wireless handover does.

/// One set of impairments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// How many packets are dropped, as a percentage.
    pub loss_percent: u8,
    /// The largest delay a packet may pick up, in milliseconds.
    pub jitter_ms: u32,
    /// How many packets arrive out of order, as a percentage.
    pub reorder_percent: u8,
    /// A single stall, in milliseconds, halfway through.
    pub freeze_ms: u32,
}

impl Profile {
    /// A network that does nothing to the stream. The control.
    #[must_use]
    pub const fn perfect() -> Self {
        Self {
            loss_percent: 0,
            jitter_ms: 0,
            reorder_percent: 0,
            freeze_ms: 0,
        }
    }

    /// The profiles the criterion names, in the order it names them.
    #[must_use]
    pub fn suite() -> Vec<(&'static str, Self)> {
        let mut profiles = vec![("clean", Self::perfect())];
        for loss in [1u8, 2, 5, 10] {
            profiles.push((
                match loss {
                    1 => "loss-1",
                    2 => "loss-2",
                    5 => "loss-5",
                    _ => "loss-10",
                },
                Self {
                    loss_percent: loss,
                    ..Self::perfect()
                },
            ));
        }
        for jitter in [20u32, 50, 100] {
            profiles.push((
                match jitter {
                    20 => "jitter-20",
                    50 => "jitter-50",
                    _ => "jitter-100",
                },
                Self {
                    jitter_ms: jitter,
                    ..Self::perfect()
                },
            ));
        }
        for reorder in [1u8, 5] {
            profiles.push((
                if reorder == 1 {
                    "reorder-1"
                } else {
                    "reorder-5"
                },
                Self {
                    reorder_percent: reorder,
                    ..Self::perfect()
                },
            ));
        }
        profiles.push((
            "freeze-500",
            Self {
                freeze_ms: 500,
                ..Self::perfect()
            },
        ));
        // The one that is closest to a bad evening on a home connection: everything at
        // once, moderately.
        profiles.push((
            "realistic",
            Self {
                loss_percent: 2,
                jitter_ms: 50,
                reorder_percent: 1,
                freeze_ms: 0,
            },
        ));
        profiles
    }
}

/// One packet as it arrives, or does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    /// The sequence number it was sent with.
    pub sequence: u16,
    /// When it arrives, in milliseconds since the stream started.
    pub at_ms: u32,
}

/// A 32-bit xorshift, so a profile is the same impairment on every machine and every run.
///
/// `rand` would be a dependency, a supply-chain entry, and a source of numbers that could
/// change between versions. A measurement whose impairment moved under it would be worse
/// than no measurement.
struct Xorshift(u32);

impl Xorshift {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    /// A percentage roll: true `percent` times in a hundred.
    fn rolls(&mut self, percent: u8) -> bool {
        if percent == 0 {
            return false;
        }
        self.next() % 100 < u32::from(percent)
    }

    /// A value in `0..=max`.
    fn upto(&mut self, max: u32) -> u32 {
        if max == 0 { 0 } else { self.next() % (max + 1) }
    }
}

/// Applies a profile to a stream of `count` packets sent every `frame_ms`.
///
/// Returns the arrivals, in the order a receiver sees them — which is the point of the
/// reordering, and why this returns a list rather than a filter.
#[must_use]
pub fn apply(profile: Profile, count: u16, frame_ms: u32, seed: u32) -> Vec<Arrival> {
    let mut random = Xorshift(if seed == 0 { 0x1234_5678 } else { seed });
    let freeze_at = u32::from(count) / 2;
    let mut arrivals = Vec::with_capacity(count as usize);
    let mut pending_swap: Option<Arrival> = None;

    for index in 0..count {
        if random.rolls(profile.loss_percent) {
            continue;
        }

        let mut at_ms = u32::from(index) * frame_ms + random.upto(profile.jitter_ms);
        // The freeze delays everything from its point on, the way a handover does: the
        // packets are not lost, they arrive late and all at once.
        if u32::from(index) >= freeze_at {
            at_ms += profile.freeze_ms;
        }

        let arrival = Arrival {
            sequence: index,
            at_ms,
        };

        // Reordering is modelled as a swap with the next packet, which is what a route
        // change actually does — not as a random shuffle, which would produce a stream no
        // network could.
        if let Some(held) = pending_swap.take() {
            arrivals.push(arrival);
            arrivals.push(held);
        } else if random.rolls(profile.reorder_percent) {
            pending_swap = Some(arrival);
        } else {
            arrivals.push(arrival);
        }
    }
    if let Some(held) = pending_swap {
        arrivals.push(held);
    }
    arrivals
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::cast_precision_loss)]

    use super::*;

    const COUNT: u16 = 1000;
    const FRAME_MS: u32 = 20;

    #[test]
    fn a_clean_network_delivers_everything_in_order_on_time() {
        let arrivals = apply(Profile::perfect(), COUNT, FRAME_MS, 1);
        assert_eq!(arrivals.len(), COUNT as usize);
        for (index, arrival) in arrivals.iter().enumerate() {
            assert_eq!(arrival.sequence as usize, index);
            assert_eq!(arrival.at_ms, index as u32 * FRAME_MS);
        }
    }

    #[test]
    fn the_same_seed_is_the_same_network() {
        // The property the whole file exists for. A measurement whose impairment moved
        // between runs would be worse than no measurement.
        let once = apply(
            Profile {
                loss_percent: 5,
                jitter_ms: 50,
                reorder_percent: 5,
                freeze_ms: 500,
            },
            COUNT,
            FRAME_MS,
            42,
        );
        let again = apply(
            Profile {
                loss_percent: 5,
                jitter_ms: 50,
                reorder_percent: 5,
                freeze_ms: 500,
            },
            COUNT,
            FRAME_MS,
            42,
        );
        assert_eq!(once, again);
    }

    #[test]
    fn a_different_seed_is_a_different_network() {
        let one = apply(
            Profile {
                loss_percent: 5,
                ..Profile::perfect()
            },
            COUNT,
            FRAME_MS,
            1,
        );
        let other = apply(
            Profile {
                loss_percent: 5,
                ..Profile::perfect()
            },
            COUNT,
            FRAME_MS,
            2,
        );
        assert_ne!(one, other);
    }

    #[test]
    fn loss_is_roughly_the_percentage_asked_for() {
        for percent in [1u8, 2, 5, 10] {
            let arrivals = apply(
                Profile {
                    loss_percent: percent,
                    ..Profile::perfect()
                },
                COUNT,
                FRAME_MS,
                7,
            );
            let lost = COUNT as usize - arrivals.len();
            let measured = lost as f64 / f64::from(COUNT) * 100.0;
            assert!(
                (measured - f64::from(percent)).abs() < 2.0,
                "asked for {percent}%, measured {measured:.1}%"
            );
        }
    }

    #[test]
    fn jitter_moves_packets_later_and_never_earlier() {
        // A packet that arrived before it was sent would be a bug in the model rather
        // than a network anyone has.
        let arrivals = apply(
            Profile {
                jitter_ms: 50,
                ..Profile::perfect()
            },
            COUNT,
            FRAME_MS,
            3,
        );
        let mut moved = 0;
        for arrival in &arrivals {
            let sent = u32::from(arrival.sequence) * FRAME_MS;
            assert!(arrival.at_ms >= sent, "packet arrived before it was sent");
            assert!(arrival.at_ms <= sent + 50);
            if arrival.at_ms > sent {
                moved += 1;
            }
        }
        assert!(moved > COUNT as usize / 2, "jitter moved almost nothing");
    }

    #[test]
    fn reordering_swaps_neighbours_rather_than_shuffling() {
        // What a route change does. A random shuffle would produce a stream no network
        // could, and a buffer tuned against it would be tuned against fiction.
        let arrivals = apply(
            Profile {
                reorder_percent: 5,
                ..Profile::perfect()
            },
            COUNT,
            FRAME_MS,
            9,
        );
        assert_eq!(arrivals.len(), COUNT as usize);
        let mut out_of_order = 0;
        for pair in arrivals.windows(2) {
            if pair[1].sequence < pair[0].sequence {
                out_of_order += 1;
                // Never by more than one place.
                assert_eq!(pair[0].sequence - pair[1].sequence, 1);
            }
        }
        assert!(out_of_order > 0, "nothing was reordered");
    }

    #[test]
    fn the_freeze_delays_rather_than_drops() {
        // A wireless handover: the packets are not lost, they arrive late and all at
        // once, which is a different problem for a buffer than loss is.
        let arrivals = apply(
            Profile {
                freeze_ms: 500,
                ..Profile::perfect()
            },
            COUNT,
            FRAME_MS,
            4,
        );
        assert_eq!(arrivals.len(), COUNT as usize, "nothing should be lost");
        let half = COUNT / 2;
        let before = arrivals
            .iter()
            .find(|a| a.sequence == half - 1)
            .expect("a packet before the freeze");
        let after = arrivals
            .iter()
            .find(|a| a.sequence == half)
            .expect("a packet after it");
        assert_eq!(after.at_ms - before.at_ms, 500 + FRAME_MS);
    }

    #[test]
    fn the_suite_covers_what_the_criterion_names() {
        let suite = Profile::suite();
        let names: Vec<&str> = suite.iter().map(|(name, _)| *name).collect();
        for expected in [
            "clean",
            "loss-1",
            "loss-2",
            "loss-5",
            "loss-10",
            "jitter-20",
            "jitter-50",
            "jitter-100",
            "reorder-1",
            "reorder-5",
            "freeze-500",
        ] {
            assert!(names.contains(&expected), "{expected} is missing");
        }
    }
}
