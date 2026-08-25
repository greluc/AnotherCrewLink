#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Gate G2, fourth criterion: the render path allocates nothing.
//!
//! > The render callback performs zero allocations under the CI allocator.
//!
//! The reason is not tidiness. An audio callback runs on a thread the operating system
//! will not wait for, and a general-purpose allocator is allowed to take a lock, walk a
//! free list, or ask the kernel for more memory. Any of those at the wrong moment is a
//! gap in somebody's audio, and the gaps arrive under load — which is when a call is
//! already going badly.
//!
//! # How it is measured
//!
//! A global allocator that counts. It is installed for the whole test binary, so the
//! counting is real rather than a model of allocation, and the measurement is taken around
//! a *steady state*: every buffer is warmed first, because the first call through any of
//! this is allowed to allocate and the criterion is about the ten thousandth.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use acl_audio::apm::{Apm, Sonora};
use acl_audio::biquad::{Biquad, FilterKind};
use acl_audio::codec::{Encoder, FRAME_SAMPLES};
use acl_audio::gain::Gain;
use acl_audio::jitter::JitterBuffer;
use acl_audio::mixer::Mixer;
use acl_audio::panner::{Panner, Position};
use acl_audio::resample::{Resampler, TARGET_RATE};

/// Counts allocations made by the measuring thread while it is armed.
struct Counting;

// Per thread, not global. The test harness runs tests in parallel, and a global counter
// measures whatever every other test happens to be allocating at the same moment — which
// is how these first passed alone and failed together, reporting thirty allocations for a
// loop that makes none.
//
// `const` initialisers, so reading the cell cannot itself allocate lazily.
thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Adds one to this thread's counter, if it is armed.
///
/// `try_with` rather than `with`: a thread tearing down its locals would otherwise panic
/// inside the allocator, and an allocator that panics takes the process with it.
fn note() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
    });
}

// SAFETY: every method forwards to the system allocator with the same layout and pointer,
// adding only a relaxed counter. The counter cannot affect the allocation itself.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's contract for `alloc` is passed straight through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc is an allocation as far as a callback is concerned: it can move the
        // block, and moving means copying under whatever lock the allocator holds.
        note();
        // SAFETY: as above.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Runs `body` with this thread's counter armed, and returns how many allocations it made.
///
/// Only this thread's, which is what makes the number mean anything while the harness is
/// running other tests beside it.
fn count(body: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|count| count.set(0));
    ARMED.with(|armed| armed.set(true));
    body();
    ARMED.with(|armed| armed.set(false));
    ALLOCATIONS.with(Cell::get)
}

fn frame(index: usize) -> Vec<f32> {
    (0..FRAME_SAMPLES)
        .map(|position| {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            {
                ((std::f64::consts::TAU * 440.0 * (index * FRAME_SAMPLES + position) as f64
                    / 48000.0)
                    .sin()
                    * 0.5) as f32
            }
        })
        .collect()
}

#[test]
fn the_dsp_graph_allocates_nothing_per_frame() {
    // The per-peer path: panner, muffle, gain. Everything it needs is allocated by the
    // caller and reused, which is what a callback has to do.
    let panner = Panner::default();
    let mut muffle = Biquad::new(FilterKind::LowPass, 2000.0, 20.0, 48000.0);
    let gain = Gain::new(0.5);
    let input = frame(0);
    let mut stereo = vec![0.0f32; FRAME_SAMPLES * 2];

    let mut render = |input: &[f32], out: &mut [f32]| {
        for (index, sample) in input.iter().enumerate() {
            let placed = panner.process(
                *sample,
                Position {
                    x: 1.0,
                    y: 0.0,
                    z: -0.5,
                },
            );
            out[index * 2] = gain.process(muffle.process(placed.left));
            out[index * 2 + 1] = gain.process(muffle.process(placed.right));
        }
    };

    // Warm it: the first pass through anything is allowed to allocate.
    render(&input, &mut stereo);

    let allocations = count(|| {
        for _ in 0..100 {
            render(&input, &mut stereo);
        }
    });
    assert_eq!(
        allocations, 0,
        "the DSP graph allocated {allocations} times"
    );
}

#[test]
fn resampling_allocates_nothing_once_it_is_warm() {
    // The property `process_into_buffer` was chosen for. `rubato`'s `process` returns a
    // fresh Vec per call, which in a capture callback is an allocation every ten
    // milliseconds.
    let mut resampler = Resampler::new(44100, 441).unwrap();
    let block: Vec<f32> = frame(0).into_iter().take(441).collect();
    let mut out = Vec::with_capacity(48000);

    for _ in 0..10 {
        out.clear();
        resampler.push(&block, &mut out).unwrap();
    }

    let allocations = count(|| {
        for _ in 0..100 {
            out.clear();
            resampler.push(&block, &mut out).unwrap();
        }
    });
    assert_eq!(allocations, 0, "resampling allocated {allocations} times");
}

#[test]
fn a_matching_rate_allocates_nothing_at_all() {
    // The common path, and the one every 48 kHz device takes.
    let mut resampler = Resampler::new(TARGET_RATE, 480).unwrap();
    let block = frame(0);
    let mut out = Vec::with_capacity(FRAME_SAMPLES * 2);
    out.clear();
    resampler.push(&block, &mut out).unwrap();

    let allocations = count(|| {
        for _ in 0..100 {
            out.clear();
            resampler.push(&block, &mut out).unwrap();
        }
    });
    assert_eq!(
        allocations, 0,
        "the passthrough allocated {allocations} times"
    );
}

#[test]
fn encoding_allocates_nothing_once_the_packet_buffer_has_grown() {
    // The buffer is resized to the maximum packet size on the first call and stays
    // there; after that the encoder writes into it.
    let mut encoder = Encoder::new().unwrap();
    let mut packet = Vec::new();
    for index in 0..5 {
        encoder.encode(&frame(index), &mut packet).unwrap();
    }

    let input = frame(6);
    let allocations = count(|| {
        for _ in 0..100 {
            encoder.encode(&input, &mut packet).unwrap();
        }
    });
    assert_eq!(allocations, 0, "encoding allocated {allocations} times");
}

#[test]
fn the_jitter_buffer_allocates_per_frame_and_this_says_how_much() {
    // This one does allocate, and the number is recorded rather than asserted away. Its
    // interface hands back an owned `Frame`, and `neteq`'s decoder trait does the same --
    // `decode(&[u8]) -> Vec<f32>` -- so a receive path built on either allocates once per
    // frame per peer. At 50 frames a second and ten peers that is 500 allocations a
    // second on the playback thread.
    //
    // It is not on the render callback today: the buffer is drained by the mixer, not by
    // the device. Whether that stays true is what criterion 4 will be measured against
    // when the playback path exists, and this test is the number to compare with.
    let mut encoder = Encoder::new().unwrap();
    let mut packet = Vec::new();
    let mut buffer = JitterBuffer::new(3).unwrap();
    for index in 0..40u16 {
        encoder.encode(&frame(index as usize), &mut packet).unwrap();
        buffer.push(index, &packet);
    }
    for _ in 0..5 {
        buffer.pop().unwrap();
    }

    let allocations = count(|| {
        for _ in 0..30 {
            let _ = buffer.pop().unwrap();
        }
    });
    // One `Vec` per frame, and one for the payload the buffer hands back to the decoder.
    assert!(
        allocations <= 30 * 2,
        "expected at most two allocations a frame, got {allocations} over 30"
    );
    assert!(
        allocations > 0,
        "if this reaches zero the comment above is stale and should go"
    );
    eprintln!("jitter buffer: {allocations} allocations over 30 frames");
}

#[test]
fn mixing_a_full_lobby_allocates_nothing() {
    // The last stage of the render callback, and the one whose cost grows with the lobby:
    // thirteen additions per block rather than one. Everything it writes into is allocated
    // in `new`, including the mono downmix the echo canceller is handed.
    const FRAMES: usize = 480;
    let mut mixer = Mixer::new(FRAMES);
    let peer = vec![0.05f32; FRAMES * 2];

    mixer.begin();
    mixer.add(&peer);
    mixer.finish();

    let allocations = count(|| {
        for _ in 0..100 {
            mixer.begin();
            for _ in 0..13 {
                mixer.add(&peer);
            }
            let _ = mixer.finish();
            let _ = mixer.reference();
        }
    });
    assert_eq!(
        allocations, 0,
        "the mixer allocated {allocations} times over 100 blocks"
    );
}

#[test]
fn the_echo_cancellers_two_paths_are_not_alike_and_this_says_how() {
    // Criterion 4 is about the render callback, and the render half of the echo canceller
    // is what runs there: it is handed the buffer on its way to the speakers so the
    // canceller knows what to subtract later. That half must be, and is, allocation-free.
    //
    // The capture half is not, and no amount of care in this crate makes it so -- the
    // allocations are inside `sonora`'s adaptive filters, not in the wrapper, whose
    // scratch buffer is allocated once in `new`. The number is recorded rather than
    // asserted away, because it decides where the capture path is allowed to run: §3.2's
    // diagram puts the APM on the cpal capture callback, and rule 1 of the same section
    // says that callback never allocates. Both cannot be true. See §3.2, which now says
    // which one gives way.
    let mut apm = Sonora::new();
    let reference: Vec<f32> = frame(0);
    let mut captured = frame(1);

    // Warm it. The first pass through an adaptive filter is allowed to allocate.
    for _ in 0..10 {
        apm.render(&reference).unwrap();
        apm.capture(&mut captured).unwrap();
    }

    let render_only = count(|| {
        for _ in 0..100 {
            apm.render(&reference).unwrap();
        }
    });
    let capture_only = count(|| {
        for _ in 0..100 {
            apm.capture(&mut captured).unwrap();
        }
    });
    eprintln!("sonora: render {render_only}, capture {capture_only}, over 100 frames each");

    // The half that runs on the render callback. This one is the gate criterion.
    assert_eq!(
        render_only, 0,
        "the far-end reference path allocated {render_only} times over 100 frames"
    );

    // And the half that cannot run there. Bounded rather than exact: the count depends on
    // what the adaptive filters are doing, and pinning it would fail on a `sonora` patch
    // release for no reason anybody could act on.
    assert!(
        capture_only > 0,
        "if this reaches zero the comment above is stale and the APM can move back onto the callback"
    );
    assert!(
        capture_only < 100 * 200,
        "capture allocated {capture_only} times over 100 frames, far more than the ~75 a frame measured"
    );
}

#[test]
fn the_counter_counts() {
    // A measurement that cannot see an allocation would pass every test above by
    // accident, which is the only way they could all pass on the first attempt.
    let seen = count(|| {
        let mut grown: Vec<u8> = Vec::new();
        for index in 0..100u8 {
            grown.push(index);
        }
        std::hint::black_box(&grown);
    });
    assert!(seen > 0, "the allocator counted nothing while a Vec grew");
}
