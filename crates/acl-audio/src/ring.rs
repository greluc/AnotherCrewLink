//! The buffer between an audio callback and a thread that is allowed to think.
//!
//! §3.2 rule 1: the audio callback never allocates, never locks, never logs. That rule is
//! what forced the capture side to have a worker thread at all — the echo canceller
//! allocates about 75 times per frame inside its own filters, so it cannot run where the
//! operating system calls us. This is the seam between the two.
//!
//! # What it is, and what it is not
//!
//! A fixed-size ring of `f32` that never allocates or blocks after construction, and never
//! grows. There is no backpressure and no notification: an audio callback cannot wait for
//! a consumer, so when the ring is full the oldest samples are dropped and the loss is
//! counted. Silently discarding them would be worse — a consumer that has fallen behind
//! produces exactly the same audio as one that has not, and the only symptom is that
//! people sound slightly wrong.
//!
//! **It does not split across threads by itself.** Both ends take `&mut self`, so the
//! producer and the consumer have to be the same thread or be separated by something else.
//! That is not the shape the capture path eventually needs, and it is deliberate: making
//! it a real single-producer single-consumer queue means either a hand-written `unsafe`
//! implementation in the middle of the audio path's trusted computing base, or a crate,
//! and that decision belongs with the code that opens the device streams rather than
//! ahead of it. The logic worth owning and testing — the wrap, the overwrite-oldest, the
//! all-or-nothing frame read — is here and is the same either way.
//!
//! An earlier version of this comment claimed to be wait-free across two threads. It was
//! not, and nothing in the file would have caught the claim.

/// A fixed-size ring of samples between a producer and a consumer.
///
/// Sized in samples rather than frames or milliseconds: it sits between two things that
/// disagree about both. The capture callback delivers whatever block size the device
/// chose, and the worker reads in 20 ms frames.
#[derive(Debug)]
pub struct Ring {
    samples: Vec<f32>,
    /// Where the producer writes next.
    write: usize,
    /// Where the consumer reads next.
    read: usize,
    /// How many samples are in the ring.
    filled: usize,
    /// How many samples the producer had to throw away because the ring was full.
    dropped: u64,
}

impl Ring {
    /// Builds a ring that holds `capacity` samples.
    ///
    /// A capacity below one frame is refused rather than clamped: it would produce a ring
    /// that drops something on every single callback, which is a configuration mistake and
    /// not a runtime condition.
    ///
    /// # Errors
    ///
    /// [`RingError::TooSmall`] if `capacity` is zero.
    pub fn new(capacity: usize) -> Result<Self, RingError> {
        if capacity == 0 {
            return Err(RingError::TooSmall);
        }
        Ok(Self {
            samples: vec![0.0; capacity],
            write: 0,
            read: 0,
            filled: 0,
            dropped: 0,
        })
    }

    /// How many samples it can hold.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.samples.len()
    }

    /// How many samples are waiting to be read.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.filled
    }

    /// Whether there is nothing to read.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// How many samples have been thrown away because the ring was full, since the start.
    ///
    /// A number that climbs means the consumer is not keeping up, which on the capture
    /// side means the echo canceller and encoder together cost more than real time. It is
    /// counted rather than logged: nothing logs from an audio callback.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Writes samples, dropping the oldest to make room if it has to.
    ///
    /// Returns how many samples were dropped to fit this write. Callable from an audio
    /// callback: no allocation, no locking, no branching on anything the consumer owns
    /// beyond the shared count.
    pub fn write(&mut self, input: &[f32]) -> usize {
        let capacity = self.samples.len();
        // A write longer than the whole ring can only keep the tail of it. Not an error:
        // a device that hands over an enormous block is unusual, not invalid.
        let keep = input.len().min(capacity);
        let skipped = input.len() - keep;
        // `get` rather than an index: the arithmetic is provably in range, but this runs
        // in an audio callback and a panic there takes the stream down with it.
        let tail = input.get(input.len() - keep..).unwrap_or(&[]);

        let overflow = (self.filled + keep).saturating_sub(capacity);
        if overflow > 0 {
            // Advance the read cursor past what is being overwritten, so the consumer
            // never reads a sample that has already been replaced under it.
            self.read = (self.read + overflow) % capacity;
            self.filled -= overflow;
        }

        for sample in tail {
            if let Some(slot) = self.samples.get_mut(self.write) {
                *slot = *sample;
            }
            self.write = (self.write + 1) % capacity;
        }
        self.filled += keep;

        let lost = overflow + skipped;
        self.dropped += lost as u64;
        lost
    }

    /// Reads into `output`, returning how many samples were filled.
    ///
    /// A short read is normal: the consumer asks for a frame and the callback has not
    /// delivered one yet. The rest of `output` is left untouched, so a caller that wants
    /// silence in the gap has to say so.
    pub fn read(&mut self, output: &mut [f32]) -> usize {
        let capacity = self.samples.len();
        let take = output.len().min(self.filled);
        for slot in output.iter_mut().take(take) {
            *slot = self.samples.get(self.read).copied().unwrap_or(0.0);
            self.read = (self.read + 1) % capacity;
        }
        self.filled -= take;
        take
    }

    /// Reads exactly `output.len()` samples, or nothing at all.
    ///
    /// What the worker wants: a frame is 20 ms of audio and half of one is not useful to
    /// an encoder. Returns whether it read.
    pub fn read_frame(&mut self, output: &mut [f32]) -> bool {
        if self.filled < output.len() {
            return false;
        }
        self.read(output);
        true
    }
}

/// What can go wrong building one.
#[derive(Debug, PartialEq, Eq)]
pub enum RingError {
    /// A capacity of zero.
    TooSmall,
}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "a ring must hold at least one sample"),
        }
    }
}

impl std::error::Error for RingError {}

#[cfg(test)]
mod tests {
    // Exact float comparison is the point here rather than a mistake: a ring must hand
    // back the bits it was given. A tolerance would pass for an implementation that
    // quietly altered samples, which is the one thing this must never do.
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::float_arithmetic
    )]

    use super::*;

    #[test]
    fn what_goes_in_comes_out_in_order() {
        let mut ring = Ring::new(16).unwrap();
        assert_eq!(ring.write(&[1.0, 2.0, 3.0]), 0);
        let mut out = [0.0; 3];
        assert_eq!(ring.read(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert!(ring.is_empty());
    }

    #[test]
    fn it_wraps() {
        // The whole point of a ring, and the place an off-by-one produces audio that is
        // correct for the first few seconds of a call and then is not.
        let mut ring = Ring::new(4).unwrap();
        ring.write(&[1.0, 2.0, 3.0]);
        let mut out = [0.0; 2];
        ring.read(&mut out);
        assert_eq!(out, [1.0, 2.0]);
        ring.write(&[4.0, 5.0, 6.0]);
        let mut rest = [0.0; 4];
        assert_eq!(ring.read(&mut rest), 4);
        assert_eq!(rest, [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn a_full_ring_drops_the_oldest_and_counts_it() {
        // A callback cannot wait, so something has to go. The newest audio is what anybody
        // is about to say; the oldest is what they said while the consumer was stalled.
        let mut ring = Ring::new(4).unwrap();
        ring.write(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(ring.write(&[5.0, 6.0]), 2);
        assert_eq!(ring.dropped(), 2);
        let mut out = [0.0; 4];
        assert_eq!(ring.read(&mut out), 4);
        assert_eq!(out, [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn the_consumer_never_reads_a_sample_that_was_overwritten() {
        // The failure this is really guarding: if the read cursor is not advanced past
        // what the producer overwrote, the consumer reads the new sample believing it is
        // the old one. Nothing errors and the audio is subtly wrong.
        let mut ring = Ring::new(3).unwrap();
        ring.write(&[1.0, 2.0, 3.0]);
        ring.write(&[4.0]);
        let mut out = [0.0; 3];
        ring.read(&mut out);
        assert_eq!(out, [2.0, 3.0, 4.0], "the overwritten sample came back");
    }

    #[test]
    fn a_write_bigger_than_the_ring_keeps_the_newest_of_it() {
        let mut ring = Ring::new(3).unwrap();
        assert_eq!(ring.write(&[1.0, 2.0, 3.0, 4.0, 5.0]), 2);
        let mut out = [0.0; 3];
        ring.read(&mut out);
        assert_eq!(out, [3.0, 4.0, 5.0]);
    }

    #[test]
    fn a_short_read_says_how_short() {
        let mut ring = Ring::new(8).unwrap();
        ring.write(&[1.0, 2.0]);
        let mut out = [9.0; 4];
        assert_eq!(ring.read(&mut out), 2);
        assert_eq!(out, [1.0, 2.0, 9.0, 9.0], "it wrote past what it had");
    }

    #[test]
    fn read_frame_is_all_or_nothing() {
        // The encoder wants twenty milliseconds. Half of one is not a smaller frame, it is
        // a click.
        let mut ring = Ring::new(8).unwrap();
        ring.write(&[1.0, 2.0, 3.0]);
        let mut frame = [0.0; 4];
        assert!(!ring.read_frame(&mut frame));
        assert_eq!(frame, [0.0; 4], "it took some anyway");
        assert_eq!(ring.len(), 3, "it consumed what it would not return");

        ring.write(&[4.0]);
        assert!(ring.read_frame(&mut frame));
        assert_eq!(frame, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_ring_that_holds_nothing_is_refused() {
        assert_eq!(Ring::new(0).unwrap_err(), RingError::TooSmall);
    }

    #[test]
    fn a_full_cycle_leaves_no_drift() {
        // Ten thousand frames through a ring sized like the real one. If the cursors drift
        // by one, this ends up reading the wrong sample, and a call that has been running
        // for an hour sounds different from one that just started.
        let mut ring = Ring::new(1920).unwrap();
        let mut expected = 0.0f32;
        let mut frame = [0.0f32; 480];
        for _ in 0..10_000 {
            let block: Vec<f32> = (0..480u16)
                .map(|index| expected + f32::from(index))
                .collect();
            ring.write(&block);
            assert!(ring.read_frame(&mut frame));
            assert_eq!(frame[0], expected);
            assert_eq!(frame[479], expected + 479.0);
            expected += 480.0;
        }
        assert_eq!(
            ring.dropped(),
            0,
            "a balanced producer and consumer dropped audio"
        );
    }
}
