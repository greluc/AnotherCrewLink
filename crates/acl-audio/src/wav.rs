//! Just enough WAV to read a golden vector.
//!
//! The vectors are 32-bit float, written by `scripts/golden-vectors`, and nothing else
//! reads them. A general-purpose decoder would be a dependency, an attack surface and a
//! source of its own disagreements for a format this file handles in sixty lines.

use std::fmt;

/// What went wrong reading a vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WavError {
    /// The file is shorter than a header.
    TooShort,
    /// The magic bytes are not `RIFF`/`WAVE`.
    NotRiff,
    /// A chunk the reader needs is missing.
    MissingChunk(&'static str),
    /// The format is not 32-bit IEEE float.
    ///
    /// Carried rather than glossed over: a vector accidentally written as 16-bit PCM
    /// would quantise the reference to about −96 dBFS, and the gate's tolerance is −80.
    Unsupported {
        /// 1 for PCM, 3 for IEEE float.
        format: u16,
        /// Bits per sample.
        bits: u16,
    },
}

impl fmt::Display for WavError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(formatter, "shorter than a WAV header"),
            Self::NotRiff => write!(formatter, "not a RIFF/WAVE file"),
            Self::MissingChunk(name) => write!(formatter, "no {name} chunk"),
            Self::Unsupported { format, bits } => {
                write!(
                    formatter,
                    "format {format} at {bits} bits, not 32-bit float"
                )
            }
        }
    }
}

impl std::error::Error for WavError {}

/// A decoded vector: interleaved samples and how many channels they interleave.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    /// Frames per second.
    pub sample_rate: u32,
    /// How many channels the samples interleave.
    pub channels: usize,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl Wav {
    /// How many frames it holds.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len().checked_div(self.channels).unwrap_or(0)
    }

    /// One channel, deinterleaved.
    #[must_use]
    pub fn channel(&self, index: usize) -> Vec<f32> {
        if index >= self.channels {
            return Vec::new();
        }
        self.samples
            .iter()
            .skip(index)
            .step_by(self.channels)
            .copied()
            .collect()
    }
}

/// Reads a 32-bit float WAV.
///
/// # Errors
///
/// Returns [`WavError`] if the file is not one, or is not 32-bit float.
pub fn decode(bytes: &[u8]) -> Result<Wav, WavError> {
    if bytes.len() < 12 {
        return Err(WavError::TooShort);
    }
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotRiff);
    }

    let mut at = 12usize;
    let mut format = None;
    let mut data = None;

    // Chunk walking rather than assuming the layout: a writer is free to put anything
    // between `fmt ` and `data`, and some do.
    while at + 8 <= bytes.len() {
        let id = bytes.get(at..at + 4).ok_or(WavError::TooShort)?;
        let size = u32::from_le_bytes(
            bytes
                .get(at + 4..at + 8)
                .ok_or(WavError::TooShort)?
                .try_into()
                .map_err(|_| WavError::TooShort)?,
        ) as usize;
        let body = at + 8;
        let end = body.checked_add(size).ok_or(WavError::TooShort)?;
        let chunk = bytes.get(body..end.min(bytes.len())).unwrap_or_default();

        if id == b"fmt " {
            format = Some(chunk.to_vec());
        } else if id == b"data" {
            data = Some(chunk.to_vec());
        }

        // Chunks are word-aligned, and an odd size carries a pad byte that is not counted.
        at = end + (size & 1);
    }

    let format = format.ok_or(WavError::MissingChunk("fmt "))?;
    let data = data.ok_or(WavError::MissingChunk("data"))?;
    if format.len() < 16 {
        return Err(WavError::MissingChunk("fmt "));
    }

    let field = |at: usize, len: usize| format.get(at..at + len).unwrap_or(&[]);
    let tag = u16::from_le_bytes(field(0, 2).try_into().unwrap_or([0, 0]));
    let channels = usize::from(u16::from_le_bytes(field(2, 2).try_into().unwrap_or([0, 0])));
    let sample_rate = u32::from_le_bytes(field(4, 4).try_into().unwrap_or([0; 4]));
    let bits = u16::from_le_bytes(field(14, 2).try_into().unwrap_or([0, 0]));

    // 3 is IEEE float. 1 is PCM, and reading one as the other silently produces noise.
    if tag != 3 || bits != 32 {
        return Err(WavError::Unsupported { format: tag, bits });
    }

    let samples = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();

    Ok(Wav {
        sample_rate,
        channels,
        samples,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn encode(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let data_length = samples.len() * 4;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&u32::try_from(36 + data_length).unwrap().to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
        out.extend_from_slice(&(channels * 4).to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&u32::try_from(data_length).unwrap().to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    #[test]
    fn reads_back_what_the_generator_writes() {
        let bytes = encode(&[0.0, 0.5, -0.5, 1.0], 2, 48000);
        let wav = decode(&bytes).expect("decodes");
        assert_eq!(wav.sample_rate, 48000);
        assert_eq!(wav.channels, 2);
        assert_eq!(wav.frames(), 2);
        assert_eq!(wav.samples, vec![0.0, 0.5, -0.5, 1.0]);
    }

    #[test]
    fn deinterleaves_a_channel() {
        let wav = decode(&encode(&[1.0, -1.0, 2.0, -2.0], 2, 48000)).expect("decodes");
        assert_eq!(wav.channel(0), vec![1.0, 2.0]);
        assert_eq!(wav.channel(1), vec![-1.0, -2.0]);
        // Asking for a channel that is not there is empty rather than a panic.
        assert!(wav.channel(2).is_empty());
    }

    #[test]
    fn refuses_anything_that_is_not_32_bit_float() {
        // A vector written as 16-bit PCM would quantise the reference to about −96 dBFS,
        // and the gate's tolerance is −80: close enough that the container would be part
        // of the measurement. Read as float it would be noise, which is worse.
        let mut bytes = encode(&[0.0], 1, 48000);
        bytes[20] = 1;
        assert_eq!(
            decode(&bytes),
            Err(WavError::Unsupported {
                format: 1,
                bits: 32
            })
        );
    }

    #[test]
    fn refuses_something_that_is_not_a_wav() {
        assert_eq!(decode(b"not a wav at all"), Err(WavError::NotRiff));
        assert_eq!(decode(b"RIFF"), Err(WavError::TooShort));
    }

    #[test]
    fn walks_past_a_chunk_it_does_not_know() {
        // Writers put `LIST` and `fact` between `fmt ` and `data`, and a reader that
        // assumed the layout would read the wrong bytes as samples rather than fail.
        let mut bytes = encode(&[0.25, 0.5], 1, 48000);
        let mut with_extra = bytes.drain(..36).collect::<Vec<_>>();
        with_extra.extend_from_slice(b"LIST");
        with_extra.extend_from_slice(&4u32.to_le_bytes());
        with_extra.extend_from_slice(b"INFO");
        with_extra.extend_from_slice(&bytes);
        // The RIFF size is now wrong, which a reader that trusts it would trip over.
        let wav = decode(&with_extra).expect("decodes past the extra chunk");
        assert_eq!(wav.samples, vec![0.25, 0.5]);
    }
}
