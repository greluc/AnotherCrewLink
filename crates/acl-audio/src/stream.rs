//! Choosing what to ask a device for, and opening it.
//!
//! §4.5 item 3c: "`cpal` device enumeration and streams". Enumeration is
//! [`crate::device`]; this is the rest.
//!
//! # Why the choosing is a module of its own
//!
//! Opening a stream cannot be tested here. CI has no sound card, and §5.2 already puts
//! device behaviour in the manual pass — unplug a microphone, switch the Windows default,
//! connect a Bluetooth headset — because that is the only way to see it.
//!
//! What *can* be tested is the decision made before the stream is opened: which of a
//! device's supported configurations to ask for. That decision is where the bugs are. A
//! device offering 44.1 kHz and 48 kHz, or eight channels and two, or a buffer size range
//! that does not contain the frame this client works in, is an ordinary device rather than
//! an exotic one, and picking wrongly produces audio that is resampled twice, or a
//! callback that hands over 4096 samples at a time, or one that fails to open at all.
//!
//! So the choice is a pure function over what the device says it supports, with tests, and
//! the part that cannot be tested is as thin as it can be made.

/// What this client wants, before a device has been consulted.
pub const WANTED_RATE: u32 = crate::codec::SAMPLE_RATE;

/// What a device was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen {
    /// The rate to open at.
    pub rate: u32,
    /// How many channels to open.
    pub channels: u16,
    /// The buffer size to ask for, in frames, if the device accepts a choice.
    pub buffer_frames: Option<u32>,
}

impl Chosen {
    /// Whether the rest of the pipeline has to resample around this.
    #[must_use]
    pub const fn needs_resampling(&self) -> bool {
        self.rate != WANTED_RATE
    }
}

/// One configuration a device says it supports.
///
/// A flattened `cpal::SupportedStreamConfigRange`, so the choosing can be tested without a
/// sound card and without `cpal` in the test's dependency tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Supported {
    /// The lowest rate this configuration covers.
    pub min_rate: u32,
    /// The highest.
    pub max_rate: u32,
    /// How many channels it carries.
    pub channels: u16,
    /// The buffer sizes it accepts, in frames, if it constrains them.
    pub buffer_frames: Option<(u32, u32)>,
}

impl Supported {
    /// Whether this configuration can be opened at `rate`.
    #[must_use]
    pub const fn covers(&self, rate: u32) -> bool {
        self.min_rate <= rate && rate <= self.max_rate
    }
}

/// What went wrong choosing.
#[derive(Debug, PartialEq, Eq)]
pub enum ChoiceError {
    /// The device offered nothing at all.
    NothingSupported,
}

impl std::fmt::Display for ChoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingSupported => write!(f, "the device supports no configuration"),
        }
    }
}

impl std::error::Error for ChoiceError {}

/// Picks what to ask a device for.
///
/// The order of preference, and why each step is where it is:
///
/// 1. **48 kHz above every other rate.** Opus, the echo canceller and the mixer all run
///    there, so anything else costs a resampler on the hot path — and on the capture side
///    that resampler sits in front of the canceller, which then adapts to a signal that
///    has been through it.
/// 2. **The fewest channels that carry the audio.** One for capture, two for playback.
///    A device that offers eight will happily open with eight, and then every callback
///    carries four times the samples for the two that matter.
/// 3. **A buffer near the frame this client works in.** 20 ms, clamped into whatever the
///    device will accept. Asking for less means more callbacks than the work needs;
///    asking for more means the jitter buffer's depth is decided by the device.
///
/// `wanted_channels` is what the caller would like: the choice takes the closest the
/// device offers at or above it, and falls back to the largest below if there is nothing.
///
/// # Errors
///
/// [`ChoiceError::NothingSupported`] if the list is empty.
pub fn choose(supported: &[Supported], wanted_channels: u16) -> Result<Chosen, ChoiceError> {
    if supported.is_empty() {
        return Err(ChoiceError::NothingSupported);
    }

    // Rate first. Everything else is a preference; this one has a cost attached.
    let at_wanted: Vec<&Supported> = supported
        .iter()
        .filter(|config| config.covers(WANTED_RATE))
        .collect();

    let (candidates, rate) = if at_wanted.is_empty() {
        // Nothing at 48 kHz. Take the highest rate on offer: resampling upwards invents
        // detail that is not there, and downwards at least discards honestly.
        let best = supported
            .iter()
            .max_by_key(|config| config.max_rate)
            .ok_or(ChoiceError::NothingSupported)?;
        let rate = best.max_rate;
        (
            supported
                .iter()
                .filter(|config| config.covers(rate))
                .collect::<Vec<_>>(),
            rate,
        )
    } else {
        (at_wanted, WANTED_RATE)
    };

    // Then channels: the closest at or above what was asked for, else the largest below.
    let chosen = candidates
        .iter()
        .filter(|config| config.channels >= wanted_channels)
        .min_by_key(|config| config.channels)
        .or_else(|| candidates.iter().max_by_key(|config| config.channels))
        .ok_or(ChoiceError::NothingSupported)?;

    // 20 ms at whatever rate was chosen: 960 frames at 48 kHz, 882 at 44.1. Asking for
    // 960 at 44.1 would be 21.8 ms and every buffer would drift against the encoder's.
    let frame_at_rate = u32::try_from(u64::from(rate) * u64::from(crate::codec::FRAME_MS) / 1000)
        .unwrap_or(u32::MAX);
    let buffer_frames = chosen
        .buffer_frames
        .map(|(low, high)| frame_at_rate.clamp(low, high));

    Ok(Chosen {
        rate,
        channels: chosen.channels,
        buffer_frames,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn at(rate: u32, channels: u16) -> Supported {
        Supported {
            min_rate: rate,
            max_rate: rate,
            channels,
            buffer_frames: Some((64, 4096)),
        }
    }

    #[test]
    fn it_takes_48_khz_when_the_device_has_it() {
        // The whole pipeline runs there. Anything else puts a resampler in front of the
        // echo canceller, which then adapts to a signal that has been through one.
        let chosen = choose(&[at(44_100, 2), at(48_000, 2), at(96_000, 2)], 2).unwrap();
        assert_eq!(chosen.rate, 48_000);
        assert!(!chosen.needs_resampling());
    }

    #[test]
    fn it_takes_the_highest_rate_when_there_is_no_48() {
        // Some devices genuinely offer only 44.1. Resampling down discards; resampling up
        // invents, so the higher rate is the one to start from.
        let chosen = choose(&[at(44_100, 2), at(32_000, 2)], 2).unwrap();
        assert_eq!(chosen.rate, 44_100);
        assert!(chosen.needs_resampling());
    }

    #[test]
    fn it_takes_the_fewest_channels_that_will_do() {
        // A device offering eight opens with eight if asked, and then every callback
        // carries four times the samples for the two that matter.
        let chosen = choose(&[at(48_000, 8), at(48_000, 2), at(48_000, 6)], 2).unwrap();
        assert_eq!(chosen.channels, 2);
    }

    #[test]
    fn it_asks_for_one_channel_on_the_capture_side() {
        let chosen = choose(&[at(48_000, 2), at(48_000, 1)], 1).unwrap();
        assert_eq!(chosen.channels, 1);
    }

    #[test]
    fn it_settles_for_more_channels_when_there_are_no_fewer() {
        // A device with no mono configuration is ordinary. Refusing to open it because it
        // will not give one channel would be worse than downmixing.
        let chosen = choose(&[at(48_000, 2)], 1).unwrap();
        assert_eq!(chosen.channels, 2);
    }

    #[test]
    fn it_falls_back_below_what_was_asked_when_there_is_nothing_above() {
        let chosen = choose(&[at(48_000, 1)], 2).unwrap();
        assert_eq!(chosen.channels, 1);
    }

    #[test]
    fn the_buffer_is_the_frame_this_client_works_in() {
        // 20 ms at 48 kHz. Asking for less means more callbacks than the work needs;
        // asking for more hands the jitter buffer's depth to the device.
        let chosen = choose(&[at(48_000, 2)], 2).unwrap();
        assert_eq!(chosen.buffer_frames, Some(960));
    }

    #[test]
    fn the_buffer_is_clamped_into_what_the_device_accepts() {
        // A device whose smallest buffer is bigger than a frame is not a failure; it is a
        // device whose callbacks carry more than one frame, which the ring absorbs.
        let wide = Supported {
            buffer_frames: Some((2048, 8192)),
            ..at(48_000, 2)
        };
        assert_eq!(choose(&[wide], 2).unwrap().buffer_frames, Some(2048));

        let narrow = Supported {
            buffer_frames: Some((64, 128)),
            ..at(48_000, 2)
        };
        assert_eq!(choose(&[narrow], 2).unwrap().buffer_frames, Some(128));
    }

    #[test]
    fn a_device_that_constrains_nothing_is_asked_for_nothing() {
        let free = Supported {
            buffer_frames: None,
            ..at(48_000, 2)
        };
        assert_eq!(choose(&[free], 2).unwrap().buffer_frames, None);
    }

    #[test]
    fn a_range_that_spans_48_counts_as_having_it() {
        // ALSA in particular reports ranges rather than points, and a check for equality
        // would resample half the devices on the machine for no reason.
        let range = Supported {
            min_rate: 8_000,
            max_rate: 192_000,
            channels: 2,
            buffer_frames: None,
        };
        assert_eq!(choose(&[range], 2).unwrap().rate, 48_000);
    }

    #[test]
    fn the_frame_follows_the_rate_when_the_rate_is_not_ours() {
        // 20 ms at 44.1 kHz is 882 frames, not 960. Asking for 960 there would be 21.8 ms
        // and every buffer would drift against the encoder's.
        let chosen = choose(&[at(44_100, 2)], 2).unwrap();
        assert_eq!(chosen.buffer_frames, Some(882));
    }

    #[test]
    fn a_device_that_supports_nothing_is_an_error_rather_than_a_guess() {
        assert_eq!(choose(&[], 2).unwrap_err(), ChoiceError::NothingSupported);
    }
}

/// The part that touches a sound card.
///
/// Deliberately the thinnest layer that can exist over [`choose`]. Everything it decides
/// has already been decided and tested above; what is left is translating `cpal`'s types,
/// which no test here can exercise because CI has no device. §5.2 covers it in the manual
/// pass, with a call live, because that is the only place it can be seen.
#[cfg(feature = "cpal-backend")]
pub mod system {
    use cpal::traits::DeviceTrait as _;

    use super::{Chosen, Supported, choose};
    use crate::device::{DeviceError, Direction};

    /// Flattens what `cpal` says a device supports into something [`choose`] can read.
    ///
    /// # Errors
    ///
    /// [`DeviceError::Unavailable`] if the device will not describe itself.
    pub fn supported(
        device: &cpal::Device,
        direction: Direction,
    ) -> Result<Vec<Supported>, DeviceError> {
        let ranges: Vec<cpal::SupportedStreamConfigRange> = match direction {
            Direction::Input => device
                .supported_input_configs()
                .map_err(|error| DeviceError::Unavailable(error.to_string()))?
                .collect(),
            Direction::Output => device
                .supported_output_configs()
                .map_err(|error| DeviceError::Unavailable(error.to_string()))?
                .collect(),
        };

        Ok(ranges
            .into_iter()
            .map(|range| Supported {
                // `SampleRate` is a plain `u32` in cpal 0.18; it was a newtype before.
                min_rate: range.min_sample_rate(),
                max_rate: range.max_sample_rate(),
                channels: range.channels(),
                buffer_frames: match range.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                    cpal::SupportedBufferSize::Unknown => None,
                },
            })
            .collect())
    }

    /// What to open a device with, decided by [`choose`] and translated for `cpal`.
    ///
    /// # Errors
    ///
    /// [`DeviceError::Unavailable`] if the device offers nothing usable.
    pub fn config_for(
        device: &cpal::Device,
        direction: Direction,
        wanted_channels: u16,
    ) -> Result<(Chosen, cpal::StreamConfig), DeviceError> {
        let chosen = choose(&supported(device, direction)?, wanted_channels)
            .map_err(|error| DeviceError::Unavailable(error.to_string()))?;
        Ok((
            chosen,
            cpal::StreamConfig {
                channels: chosen.channels,
                sample_rate: chosen.rate,
                buffer_size: chosen
                    .buffer_frames
                    .map_or(cpal::BufferSize::Default, |frames| {
                        cpal::BufferSize::Fixed(frames)
                    }),
            },
        ))
    }
}
