//! Sums every peer into the one buffer the output device is handed.
//!
//! The last stage of the render path. Each peer has already been through its own panner,
//! filter, reverb and gain; what is left is to add them together, and to decide what
//! happens when the sum leaves the range a sound card accepts.
//!
//! # Why this also produces the echo canceller's reference
//!
//! §3.3: the far-end reference the APM receives has to be *the buffer handed to the output
//! device* — after mixing, after panning, filtering and reverb — because that is what
//! actually left the speakers and came back into the microphone. Not one peer's decoded
//! audio, and not the mix before the graph.
//!
//! Putting the downmix here rather than leaving the caller to build it is deliberate.
//! There is exactly one buffer that is correct to use, this is where it exists, and a
//! caller that assembled its own would be free to assemble the wrong one — which does not
//! fail, it just quietly stops cancelling.
//!
//! # Clipping
//!
//! Thirteen people talking at once sum past what a sound card can represent. Chromium's
//! destination node clamps, so this clamps: the reference implementation for every other
//! number in this crate is what the Electron client does, and it would be strange to match
//! Chromium to −80 dBFS through five DSP nodes and then differ at the last addition.
//!
//! Clamping is audibly harsh when it happens. It is also rare — proximity chat attenuates
//! almost everyone almost all the time — and a limiter that avoided it would be a
//! deliberate difference from the thing this is a port of. If that changes, it changes
//! with a measurement and a listening test, not here.

/// Mixes the per-peer render output into one stereo buffer.
///
/// Every buffer it needs is allocated once, in [`Mixer::new`]. It runs on the render
/// callback, where §3.2 rule 1 forbids allocating at all.
#[derive(Debug)]
pub struct Mixer {
    /// Interleaved stereo, `frames * 2` long.
    output: Vec<f32>,
    /// The mono downmix of the same audio, for the echo canceller.
    reference: Vec<f32>,
    /// How many frames wide the buffers are.
    frames: usize,
    /// How many peers were added since [`Mixer::begin`].
    added: usize,
    /// Whether the sum had to be clamped in the last block.
    clipped: bool,
}

impl Mixer {
    /// Builds a mixer for a fixed block size, in frames.
    ///
    /// One frame is one sample per channel, so the interleaved buffer is twice this.
    #[must_use]
    pub fn new(frames: usize) -> Self {
        Self {
            output: vec![0.0; frames * 2],
            reference: vec![0.0; frames],
            frames,
            added: 0,
            clipped: false,
        }
    }

    /// How many frames wide this mixer is.
    #[must_use]
    pub const fn frames(&self) -> usize {
        self.frames
    }

    /// Clears the accumulator, ready for a new block.
    pub fn begin(&mut self) {
        self.output.fill(0.0);
        self.added = 0;
        self.clipped = false;
    }

    /// Adds one peer's interleaved stereo contribution.
    ///
    /// A block of the wrong length is ignored rather than partially mixed: half a peer is
    /// a click, and on the render callback there is nothing useful to do with an error.
    /// Returns whether it was added, so a caller that can act on it may.
    pub fn add(&mut self, stereo: &[f32]) -> bool {
        if stereo.len() != self.output.len() {
            return false;
        }
        for (into, sample) in self.output.iter_mut().zip(stereo) {
            *into += *sample;
        }
        self.added += 1;
        true
    }

    /// Finishes the block: clamps, builds the reference, and returns the output.
    ///
    /// The returned slice is what goes to the device. [`Mixer::reference`] is what goes to
    /// the echo canceller, and it is only valid after this has been called.
    pub fn finish(&mut self) -> &[f32] {
        for sample in &mut self.output {
            if *sample > 1.0 {
                *sample = 1.0;
                self.clipped = true;
            } else if *sample < -1.0 {
                *sample = -1.0;
                self.clipped = true;
            }
        }

        // The downmix the canceller needs. Averaged rather than summed: a microphone hears
        // one room, not the sum of two speakers, and a reference twice as loud as reality
        // makes the adaptive filter converge on a gain that is wrong by 6 dB.
        for (frame, into) in self.reference.iter_mut().enumerate() {
            let left = self.output.get(frame * 2).copied().unwrap_or(0.0);
            let right = self.output.get(frame * 2 + 1).copied().unwrap_or(0.0);
            *into = f32::midpoint(left, right);
        }
        &self.output
    }

    /// The mono far-end reference for the echo canceller, valid after [`Mixer::finish`].
    #[must_use]
    pub fn reference(&self) -> &[f32] {
        &self.reference
    }

    /// The finished block, without recomputing it.
    #[must_use]
    pub fn output(&self) -> &[f32] {
        &self.output
    }

    /// How many peers went into the last block.
    #[must_use]
    pub const fn added(&self) -> usize {
        self.added
    }

    /// Whether the last block had to be clamped.
    ///
    /// Worth reporting rather than hiding: repeated clipping means somebody's per-player
    /// volumes add up to more than the output can carry, and that is a thing a player can
    /// act on once they are told.
    #[must_use]
    pub const fn clipped(&self) -> bool {
        self.clipped
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss
    )]

    use super::*;

    const FRAMES: usize = 480;

    fn constant(value: f32) -> Vec<f32> {
        vec![value; FRAMES * 2]
    }

    #[test]
    fn a_block_with_nobody_in_it_is_silence() {
        // The menu, and every moment when nobody is close enough to hear.
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        assert!(mixer.finish().iter().all(|s| *s == 0.0));
        assert_eq!(mixer.added(), 0);
        assert!(mixer.reference().iter().all(|s| *s == 0.0));
    }

    #[test]
    fn peers_add_together() {
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        assert!(mixer.add(&constant(0.1)));
        assert!(mixer.add(&constant(0.25)));
        let out = mixer.finish();
        assert!((out[0] - 0.35).abs() < 1e-6, "{}", out[0]);
        assert_eq!(mixer.added(), 2);
        assert!(!mixer.clipped());
    }

    #[test]
    fn begin_clears_what_the_last_block_left() {
        // Without this every block would be louder than the one before it, which is a bug
        // that sounds like a bad microphone for about four seconds and then like nothing
        // at all, because everything clamps.
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        mixer.add(&constant(0.5));
        mixer.finish();
        mixer.begin();
        mixer.add(&constant(0.5));
        let out = mixer.finish();
        assert!((out[0] - 0.5).abs() < 1e-6, "{}", out[0]);
    }

    #[test]
    fn the_sum_is_clamped_the_way_chromium_clamps_it() {
        // Thirteen people at once. Chromium's destination node clamps, and every other
        // number in this crate is matched against Chromium, so the last addition is not
        // where to start differing.
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        for _ in 0..13 {
            mixer.add(&constant(0.2));
        }
        let out = mixer.finish();
        assert!((out[0] - 1.0).abs() < 1e-6, "{}", out[0]);
        assert!(mixer.clipped(), "clamping happened and was not reported");
    }

    #[test]
    fn it_clamps_downwards_too() {
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        for _ in 0..13 {
            mixer.add(&constant(-0.2));
        }
        assert!((mixer.finish()[0] + 1.0).abs() < 1e-6);
        assert!(mixer.clipped());
    }

    #[test]
    fn the_reference_is_the_mix_and_not_one_peer() {
        // The property §3.3 spends a paragraph on. A canceller given one peer's audio, or
        // the mix before the graph, subtracts something the microphone never heard: it
        // reports success and removes nothing.
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        mixer.add(&constant(0.1));
        mixer.add(&constant(0.3));
        mixer.finish();
        // 0.4 in both channels, averaged back to 0.4.
        assert!(
            (mixer.reference()[0] - 0.4).abs() < 1e-6,
            "{}",
            mixer.reference()[0]
        );
        assert_eq!(mixer.reference().len(), FRAMES);
    }

    #[test]
    fn the_reference_averages_the_channels_rather_than_summing_them() {
        // A microphone hears one room. A reference twice as loud as reality has the
        // adaptive filter converge on a gain that is wrong by 6 dB, which cancels less
        // than doing nothing would in some bands.
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        let mut hard_left = vec![0.0f32; FRAMES * 2];
        for frame in 0..FRAMES {
            hard_left[frame * 2] = 0.8;
        }
        mixer.add(&hard_left);
        mixer.finish();
        assert!(
            (mixer.reference()[0] - 0.4).abs() < 1e-6,
            "{}",
            mixer.reference()[0]
        );
    }

    #[test]
    fn a_block_of_the_wrong_length_is_refused_rather_than_half_mixed() {
        let mut mixer = Mixer::new(FRAMES);
        mixer.begin();
        assert!(!mixer.add(&vec![0.5; FRAMES]));
        assert!(!mixer.add(&vec![0.5; FRAMES * 2 + 1]));
        assert_eq!(mixer.added(), 0);
        assert!(mixer.finish().iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_channels_stay_apart() {
        // Interleaved, so an off-by-one here swaps left and right for everybody -- which
        // is exactly as wrong as it sounds and produces no error at all.
        let mut mixer = Mixer::new(2);
        mixer.begin();
        mixer.add(&[1.0, 0.0, 0.5, 0.0]);
        let out = mixer.finish();
        assert_eq!(out, &[1.0, 0.0, 0.5, 0.0]);
    }
}
