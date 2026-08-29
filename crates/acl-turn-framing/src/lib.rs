//! Splitting a TURN stream back into the messages a datagram socket sends one at a time.
//!
//! Over UDP a TURN message is a datagram: the boundary is the packet. Over TCP and TLS
//! there is no boundary, and RFC 8656 §3.1 says the two kinds of message are simply sent
//! back to back — the receiver works out where each ends from its own header. This does
//! that, and nothing else.
//!
//! # Why it exists
//!
//! `webrtc =0.20.3` refuses every relay URL that is not plain UDP
//! (`turn_relayer.rs:250`), so a player on a network that blocks outbound UDP gathers no
//! relay candidate and cannot reach anybody. The client this project replaces uses
//! Chromium, which allocates over TCP and TLS perfectly well, so those players work on 1.x
//! and not here. This is the half of the way back that has to be exactly right, and it is
//! the half that fails *silently* when it is not: a misplaced boundary does not error, it
//! produces a message that parses into something else.
//!
//! # The rule
//!
//! The first two bits of a TURN message say which kind it is (RFC 8656 §12.1, and STUN's
//! own RFC 5389 §6):
//!
//! * `00` — a STUN message. Twenty bytes of header, then the attribute length that bytes
//!   two and three carry. STUN attributes are padded to four bytes, so that length is
//!   always a multiple of four.
//! * `01` — a `ChannelData` message. Four bytes of header, then the length bytes two and
//!   three carry. **Over TCP it is then padded to a multiple of four** (RFC 8656 §12.4),
//!   which is the rule a reader has to apply and a writer has to have applied.
//! * Anything else is not TURN, and a stream carrying it is a stream to abandon: there is
//!   no way to find the next boundary once one has been lost.
//!
//! Nothing here needs to *understand* a message, only to find its end. The bytes go on to
//! the client's own STUN and TURN parsers unchanged, which is what makes this safe to sit
//! in the middle of an authenticated exchange — it never touches the message integrity it
//! is carrying.
//!
//! # The other direction needs nothing
//!
//! Writing is a copy. A datagram already holds exactly one message, `rtc-turn`'s
//! `ChannelData::encode` already pads to four bytes (`proto/chandata.rs:48`), and a STUN
//! message is aligned by construction — so what a UDP socket hands over is already framed
//! the way TCP wants it. That is worth knowing rather than assuming: if the encoder ever
//! stopped padding, this file's tests would still pass and the far end would start
//! rejecting channel data.

/// The fixed part of a STUN message: two bytes of type, two of length, sixteen of magic
/// cookie and transaction id.
const STUN_HEADER: usize = 20;

/// The fixed part of a `ChannelData` message: two bytes of channel number, two of length.
const CHANNEL_HEADER: usize = 4;

/// What `ChannelData` is padded to over TCP.
const PADDING: usize = 4;

/// The largest message worth believing.
///
/// A TURN message carrying audio is a few hundred bytes and the specification's own limit
/// is what a datagram can hold. Anything above this is a length field read out of a stream
/// that has lost its place, and following it would mean waiting for bytes that are never
/// coming while the real messages queue up behind them.
const MAX_MESSAGE: usize = 64 * 1024;

/// Why a stream cannot be read any further.
///
/// One variant, and deliberately: every way this fails is the same failure. The boundary
/// is lost, and there is nothing in the protocol to resynchronise on — no marker, no
/// escape, nothing that cannot also occur inside a payload. A caller's only sound answer
/// is to close the connection and allocate again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotTurn {
    /// What was wrong, for the log line somebody will read when a relay stops working.
    pub reason: String,
}

impl std::fmt::Display for NotTurn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.reason)
    }
}

impl std::error::Error for NotTurn {}

/// How long the message at the front of `buffer` is, if all of it is there.
///
/// `Ok(None)` means the header is there and the body is not, or the header is not there
/// yet — both are "wait for more", which is the ordinary answer on a stream.
///
/// # Errors
///
/// [`NotTurn`] when the first two bits name neither kind of message, or when a length
/// field claims more than [`MAX_MESSAGE`]. Both mean the boundary has been lost.
pub fn message_length(buffer: &[u8]) -> Result<Option<usize>, NotTurn> {
    let Some(&first) = buffer.first() else {
        return Ok(None);
    };

    // The two top bits. `00` is STUN, `01` is ChannelData, and the other two are reserved
    // by RFC 8656 §12.1 for exactly the purpose of making this test total.
    match first >> 6 {
        0b00 => {
            let Some(declared) = length_at(buffer, 2) else {
                return Ok(None);
            };
            // STUN attributes are padded to four bytes, so a length that is not a multiple
            // of four is not a STUN message however plausible its first byte was.
            if declared % 4 != 0 {
                return Err(NotTurn {
                    reason: format!("a STUN length of {declared} is not a multiple of four"),
                });
            }
            whole(STUN_HEADER + declared, buffer.len())
        }
        0b01 => {
            let Some(declared) = length_at(buffer, 2) else {
                return Ok(None);
            };
            // The padding is part of the message on a stream: the next one starts after
            // it, and a reader that stopped at the declared length would begin the next
            // message up to three bytes early.
            whole(padded(CHANNEL_HEADER + declared), buffer.len())
        }
        _ => Err(NotTurn {
            reason: format!("a first byte of {first:#04x} is neither STUN nor channel data"),
        }),
    }
}

/// The sixteen-bit length at `at`, if both its bytes have arrived.
fn length_at(buffer: &[u8], at: usize) -> Option<usize> {
    let high = buffer.get(at)?;
    let low = buffer.get(at + 1)?;
    Some(usize::from(u16::from_be_bytes([*high, *low])))
}

/// `total` rounded up to a multiple of four.
const fn padded(total: usize) -> usize {
    total.div_ceil(PADDING) * PADDING
}

/// `total` if it is believable and has all arrived.
fn whole(total: usize, available: usize) -> Result<Option<usize>, NotTurn> {
    if total > MAX_MESSAGE {
        return Err(NotTurn {
            reason: format!("a message length of {total} is past anything TURN carries"),
        });
    }
    Ok((available >= total).then_some(total))
}

/// A stream being split back into messages.
///
/// Holds only what has not yet been handed over. A caller feeds it whatever the socket
/// gave and takes whole messages out until there are none, which is the shape a stream
/// reader wants and the shape that cannot lose a boundary across a read.
#[derive(Debug, Default)]
pub struct Frames {
    held: Vec<u8>,
}

impl Frames {
    /// A reader with nothing held.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: Vec::new() }
    }

    /// Adds what the socket delivered.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.held.extend_from_slice(bytes);
    }

    /// How many bytes are waiting for the rest of their message.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.held.len()
    }

    /// Takes the next whole message, if there is one.
    ///
    /// # Errors
    ///
    /// [`NotTurn`] when the stream cannot be split any further. The reader is left as it
    /// was: there is nothing useful to do with it afterwards, and clearing it would hide
    /// the bytes somebody debugging this will want to look at.
    pub fn next_message(&mut self) -> Result<Option<Vec<u8>>, NotTurn> {
        let Some(length) = message_length(&self.held)? else {
            return Ok(None);
        };
        let message = self.held.drain(..length).collect();
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// A STUN message with `body` bytes of attributes.
    fn stun(body: usize) -> Vec<u8> {
        assert_eq!(body % 4, 0, "STUN attributes are padded to four bytes");
        let mut message = vec![0u8; STUN_HEADER + body];
        // A Binding request: the top two bits are zero, which is what the split reads.
        message[0] = 0x00;
        message[1] = 0x01;
        message[2..4].copy_from_slice(&u16::try_from(body).unwrap().to_be_bytes());
        // The magic cookie, so this looks like what it claims to be to anything that
        // checks -- nothing here does, and a fixture that would not parse downstream is a
        // fixture that teaches the wrong thing.
        message[4..8].copy_from_slice(&0x2112_A442_u32.to_be_bytes());
        message
    }

    /// A `ChannelData` message carrying `payload` bytes, padded as TCP requires.
    fn channel(number: u16, payload: usize) -> Vec<u8> {
        assert!(
            (0x4000..=0x7FFF).contains(&number),
            "a channel number's top two bits are 01"
        );
        let mut message = vec![0u8; padded(CHANNEL_HEADER + payload)];
        message[0..2].copy_from_slice(&number.to_be_bytes());
        message[2..4].copy_from_slice(&u16::try_from(payload).unwrap().to_be_bytes());
        for (index, slot) in message[CHANNEL_HEADER..CHANNEL_HEADER + payload]
            .iter_mut()
            .enumerate()
        {
            // Recognisable bytes, so a test that reassembles the wrong span says so.
            *slot = u8::try_from(index % 251).unwrap();
        }
        message
    }

    #[test]
    fn a_stun_message_ends_where_its_length_says() {
        let message = stun(12);
        assert_eq!(message.len(), 32);
        assert_eq!(message_length(&message).unwrap(), Some(32));
        // And with more behind it, the answer is still this one's length.
        let mut stream = message.clone();
        stream.extend_from_slice(&stun(8));
        assert_eq!(message_length(&stream).unwrap(), Some(32));
    }

    #[test]
    fn a_channel_message_ends_after_its_padding() {
        // The case the whole file exists for. A ChannelData carrying 200 bytes of Opus is
        // 204 bytes long and occupies 204 on the wire; one carrying 201 occupies 208, and
        // a reader that stopped at 205 would begin the next message three bytes early --
        // which does not fail, it produces a first byte that means something else.
        assert_eq!(message_length(&channel(0x4000, 200)).unwrap(), Some(204));
        assert_eq!(message_length(&channel(0x4000, 201)).unwrap(), Some(208));
        assert_eq!(message_length(&channel(0x4000, 202)).unwrap(), Some(208));
        assert_eq!(message_length(&channel(0x4000, 203)).unwrap(), Some(208));
        assert_eq!(message_length(&channel(0x4000, 204)).unwrap(), Some(208));
    }

    #[test]
    fn a_header_that_has_not_all_arrived_is_not_an_error() {
        // The ordinary state of a stream, and the one a reader is in most of the time.
        for taken in 0..STUN_HEADER {
            let partial = &stun(8)[..taken];
            assert_eq!(
                message_length(partial).unwrap(),
                None,
                "{taken} bytes of a header is not yet an answer"
            );
        }
        // Nor is a header with a body still on its way.
        let message = channel(0x4000, 60);
        assert_eq!(message_length(&message[..10]).unwrap(), None);
    }

    #[test]
    fn a_stream_of_messages_comes_back_out_one_at_a_time() {
        let sent = [
            stun(0),
            channel(0x4000, 160),
            stun(28),
            channel(0x4001, 3),
            channel(0x4000, 160),
        ];
        let mut stream = Vec::new();
        for message in &sent {
            stream.extend_from_slice(message);
        }

        // Fed a byte at a time, which is the shape a stream actually arrives in and the
        // shape that finds an off-by-one in the boundary.
        let mut frames = Frames::new();
        let mut taken = Vec::new();
        for byte in &stream {
            frames.feed(std::slice::from_ref(byte));
            while let Some(message) = frames.next_message().unwrap() {
                taken.push(message);
            }
        }
        assert_eq!(taken.len(), sent.len());
        for (got, expected) in taken.iter().zip(sent.iter()) {
            assert_eq!(got, expected);
        }
        assert_eq!(frames.pending(), 0, "nothing left over");
    }

    #[test]
    fn a_whole_read_of_several_messages_is_split_the_same_way() {
        // The other extreme: everything in one read, which is what a fast connection does.
        let sent = [channel(0x4000, 160), channel(0x4000, 160), stun(8)];
        let mut stream = Vec::new();
        for message in &sent {
            stream.extend_from_slice(message);
        }

        let mut frames = Frames::new();
        frames.feed(&stream);
        for expected in &sent {
            assert_eq!(frames.next_message().unwrap().as_ref(), Some(expected));
        }
        assert_eq!(frames.next_message().unwrap(), None);
    }

    #[test]
    fn a_first_byte_that_is_neither_ends_the_stream() {
        // RFC 8656 §12.1 reserves the other two values of the top two bits precisely so
        // that this test is total. There is nothing to resynchronise on -- no marker that
        // cannot also occur inside a payload -- so the only sound answer is to stop.
        for first in [0x80_u8, 0xC0] {
            let mut frames = Frames::new();
            frames.feed(&[first, 0, 0, 0, 0, 0, 0, 0]);
            assert!(frames.next_message().is_err(), "{first:#04x}");
        }
    }

    #[test]
    fn a_stun_length_that_is_not_aligned_ends_the_stream() {
        // Every STUN attribute is padded to four bytes, so the length is always a multiple
        // of four. One that is not means these bytes are not a STUN header -- most likely
        // the middle of something else, read as though it were the start.
        let mut message = stun(8);
        message[3] = 9;
        let mut frames = Frames::new();
        frames.feed(&message);
        assert!(frames.next_message().is_err());
    }

    #[test]
    fn an_absurd_length_ends_the_stream_rather_than_waiting_for_it() {
        // Sixty-four kilobytes is already far past anything TURN carries. Without this a
        // lost boundary reads a length out of a payload, and the reader waits for bytes
        // that are not coming while the real messages queue up behind them -- a relay that
        // stops working with no error anywhere.
        let mut message = channel(0x4000, 4);
        message[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        let mut frames = Frames::new();
        frames.feed(&message);
        assert!(frames.next_message().is_err());
    }

    #[test]
    fn what_a_datagram_holds_is_already_framed_for_a_stream() {
        // The write direction needs no work, and this is what makes that true rather than
        // assumed. `rtc-turn`'s `ChannelData::encode` pads to four bytes before it sends,
        // and STUN is aligned by construction -- so a datagram copied verbatim onto a
        // stream lands on a boundary the far end will find.
        //
        // If the encoder ever stopped padding, the split above would still pass its own
        // tests and the far end would begin rejecting channel data. This is the test that
        // would go first.
        for payload in [0_usize, 1, 3, 4, 160, 201] {
            let datagram = channel(0x4000, payload);
            assert_eq!(
                datagram.len() % PADDING,
                0,
                "a channel message of {payload} bytes must be a whole number of words"
            );
            assert_eq!(message_length(&datagram).unwrap(), Some(datagram.len()));
        }
    }
}
