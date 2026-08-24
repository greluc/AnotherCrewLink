#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Gate G1: the Rust reader against the Electron one, frame for frame.
//!
//! The recordings come from `src/main/recorder.ts` in the Electron client, which captures
//! every region the reader touched and the `AmongUsState` it produced from them. This
//! replays the regions into the Rust reader and compares the two states.
//!
//! The gate is exact. Its wording is worth repeating because it is unusual: *"this is a
//! lossless, purely mechanical transformation, so anything less than exact means a bug,
//! not a tolerance."* The one allowance is float positions, within 1e-6, and that exists
//! because JSON round-trips a float through decimal and back.
//!
//! # Running it
//!
//! Put `.ndjson` or `.ndjson.gz` files in `test/recordings/` and run
//! `cargo test -p acl-game`. Without
//! any it skips, loudly. The empty corpus is tracked as
//! <https://github.com/greluc/AnotherCrewLink/issues/10>, because it needs frames from a
//! real game and nobody can write those at a keyboard. A test that quietly passes having compared nothing would be the
//! worst possible outcome here: it would report that the gate is met.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use acl_game::mods::Mod;
use acl_game::offsets::Offsets;
use acl_game::reader::{ReadContext, read_state};
use acl_game::resolve::resolve_offsets;
use acl_game::sparse::SparseProcess;
use acl_game::state::AmongUsState;
use acl_game::{Module, ProcessMemory};
use flate2::read::GzDecoder;
use serde::Deserialize;

/// How far two float positions may differ and still count as equal.
///
/// The gate's number. It is here for JSON's decimal round trip and for nothing else.
const POSITION_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Deserialize)]
struct RecordedRead {
    /// Where it started, as hex.
    a: String,
    /// What was there, base64.
    b: String,
}

#[derive(Debug, Deserialize)]
struct RecordedModule {
    #[allow(dead_code)]
    name: String,
    base: String,
    size: String,
}

#[derive(Debug, Deserialize)]
struct RecordedFrame {
    frame: u64,
    is64: bool,
    module: Option<RecordedModule>,
    reads: Vec<RecordedRead>,
    /// The Electron reader's answer, kept as raw JSON so a field this port does not know
    /// about still shows up as a difference rather than being dropped on the way in.
    state: serde_json::Value,
    /// The bundle the Electron reader was actually using, written once per file.
    ///
    /// The scan cannot be redone on replay: it runs inside `memoryjs` and never passes
    /// through a recorded read, so the module's bytes are not in the file. The first real
    /// recording failed all 124 of its frames for exactly this reason before the recorder
    /// started carrying the resolved bundle.
    #[serde(default)]
    offsets: Option<serde_json::Value>,
}

fn recordings_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/recordings")
}

fn recordings() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(recordings_directory()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            // `.ndjson`, or `.ndjson.gz` for a committed one.
            path.extension()
                .is_some_and(|extension| extension == "ndjson" || extension == "gz")
        })
        .collect();
    found.sort();
    found
}

fn parse_hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    // A small decoder rather than a dependency. The recorder writes standard base64 with
    // padding and nothing else, and adding a crate to the reader's test dependencies to
    // read its own fixtures is not a trade worth making.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (index, byte) in TABLE.iter().enumerate() {
        lookup[*byte as usize] = u8::try_from(index).ok()?;
    }

    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        if byte == b'=' || byte == b'\n' || byte == b'\r' {
            continue;
        }
        let value = lookup[byte as usize];
        if value == 255 {
            return None;
        }
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((accumulator >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// Reads a recording, compressed or not.
///
/// A session runs about 10 KB per frame and gzips by a factor of 130, because 99.8% of
/// the regions in a frame are byte-identical to the frame before. Committing them
/// compressed keeps a working copy small; the harness does not care which it is given.
fn read_recording(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("a recording");
    if path.extension().is_some_and(|extension| extension == "gz") {
        let mut text = String::new();
        GzDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .expect("a gzipped recording");
        text
    } else {
        String::from_utf8(bytes).expect("a recording is UTF-8")
    }
}

/// Rebuilds the process as it was when the frame was recorded.
fn replay(frame: &RecordedFrame) -> Option<(SparseProcess, Module)> {
    let recorded = frame.module.as_ref()?;
    let base = parse_hex(&recorded.base)?;
    let size = parse_hex(&recorded.size)?;

    let mut process = SparseProcess::new(frame.is64).with_module("GameAssembly.dll", base, size);
    for read in &frame.reads {
        // A region that cannot be parsed is dropped, not fatal. Returning `None` here
        // discards the whole frame, and one unusable address among hundreds of good ones
        // is not a reason to throw the frame away — it is a reason for the read that
        // needed it to fail, loudly, where the comparison can see it.
        let (Some(address), Some(bytes)) = (parse_hex(&read.a), decode_base64(&read.b)) else {
            continue;
        };
        process = process.with_region(address, bytes);
    }
    Some((
        process,
        Module {
            name: "GameAssembly.dll".to_owned(),
            base,
            size,
        },
    ))
}

/// Every difference between two states, as `path -> (electron, rust)`.
///
/// Compared as JSON rather than as structs, so a field the Rust reader does not model at
/// all is a difference rather than something quietly absent from both sides.
fn differences(
    electron: &serde_json::Value,
    rust: &serde_json::Value,
    path: &str,
    out: &mut BTreeMap<String, (String, String)>,
) {
    match (electron, rust) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            let mut keys: Vec<&String> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                differences(
                    left.get(key).unwrap_or(&serde_json::Value::Null),
                    right.get(key).unwrap_or(&serde_json::Value::Null),
                    &child,
                    out,
                );
            }
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            if left.len() != right.len() {
                out.insert(
                    format!("{path}.length"),
                    (left.len().to_string(), right.len().to_string()),
                );
            }
            for index in 0..left.len().max(right.len()) {
                differences(
                    left.get(index).unwrap_or(&serde_json::Value::Null),
                    right.get(index).unwrap_or(&serde_json::Value::Null),
                    &format!("{path}[{index}]"),
                    out,
                );
            }
        }
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) else {
                if left != right {
                    out.insert(path.to_owned(), (left.to_string(), right.to_string()));
                }
                return;
            };
            // The gate's one tolerance, and only for the fields it names. These are JSON
            // paths, not file names; clippy's extension lint does not apply.
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            let tolerance = if path.ends_with(".x") || path.ends_with(".y") {
                POSITION_TOLERANCE
            } else {
                0.0
            };
            if (left - right).abs() > tolerance {
                out.insert(path.to_owned(), (left.to_string(), right.to_string()));
            }
        }
        (left, right) => {
            if left != right {
                out.insert(path.to_owned(), (left.to_string(), right.to_string()));
            }
        }
    }
}

fn offsets_for(process: &dyn ProcessMemory) -> Offsets {
    let name = if process.is_64bit() {
        "offsets__x64__V2026.8.18__offsets.json"
    } else {
        "offsets__x86__V2026.8.18__offsets.json"
    };
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/offsets")
            .join(name),
    )
    .expect("a fixture");
    serde_json::from_str(&text).expect("parses")
}

/// The recorder's own output, parsed and replayed by this harness.
///
/// Not a parity check — the fixture's state is written by `src/main/recorder.test.ts`,
/// so comparing against it would only prove that both sides agree with whoever wrote the
/// test. What it proves is narrower and is the thing that would otherwise be discovered
/// too late: that a file the Electron recorder produces is one this harness can read.
///
/// The alternative is somebody playing five sessions and finding out afterwards that a
/// field was renamed on one side of the boundary. Regenerate with
/// `npx vitest run src/main/recorder.test.ts`.
#[test]
fn a_file_from_the_electron_recorder_is_one_this_harness_can_replay() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/recording-format/one-frame.ndjson");
    let text = std::fs::read_to_string(&path).expect("the committed format fixture");

    let mut seen = 0usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let frame: RecordedFrame =
            serde_json::from_str(line).expect("the recorder's format still deserialises");
        let (process, module) =
            replay(&frame).expect("the frame carries a module and readable regions");

        // The regions really did arrive, at the addresses the recorder wrote as hex.
        let mut first = [0u8; 2];
        process
            .read_exact(0x1_4000_1000, &mut first)
            .expect("a region the fixture recorded");
        assert_eq!(&first, b"MZ", "base64 and hex survived the round trip");
        assert_eq!(module.base, 0x1_4000_0000);
        assert!(frame.is64, "the fixture records a 64-bit process");

        // And the state is carried as raw JSON, so a field this port does not know about
        // still reaches the comparison rather than being dropped on the way in.
        assert_eq!(
            frame
                .state
                .get("lobbyCode")
                .and_then(serde_json::Value::as_str),
            Some("FORMAT")
        );
        seen += 1;
    }
    assert_eq!(seen, 1, "the fixture is one frame");
}

#[test]
fn the_rust_reader_agrees_with_the_electron_one() {
    let files = recordings();
    if files.is_empty() {
        eprintln!(
            "skipping gate G1: no recordings in {}.\n\
             Record with `set ACL_RECORD=<name>` before starting the Electron client, then \
             copy userData/recordings/*.ndjson there. One session per map, covering lobby, \
             tasks, meeting, vents, cameras, sabotage and deaths.
\n             Tracked as https://github.com/greluc/AnotherCrewLink/issues/10.",
            recordings_directory().display()
        );
        return;
    }

    let mut frames = 0usize;
    let mut mismatched = 0usize;
    let mut first_report = String::new();

    for path in &files {
        let text = read_recording(path);
        // Written once per file and used for every frame in it.
        let mut carried: Option<Offsets> = None;
        // Threaded from frame to frame, the way the reader sees it when it runs.
        let mut previous: Option<AmongUsState> = None;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let frame: RecordedFrame = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{} has a bad frame: {error}", path.display()));
            let Some((process, module)) = replay(&frame) else {
                // A frame recorded before the module was known cannot be replayed. It is
                // not a parity failure, but it is not a pass either.
                continue;
            };

            if let Some(recorded) = frame.offsets.as_ref() {
                carried = serde_json::from_value(recorded.clone()).ok();
            }
            // The recorded bundle if the file carries one, otherwise a scan against the
            // fixture — which only works for a recording that happens to include the
            // module's bytes, and says so when it does not.
            let resolved = if let Some(offsets) = carried.as_ref() {
                offsets.clone()
            } else {
                let offsets = offsets_for(&process);
                resolve_offsets(&process, &module, &offsets)
                    .expect("resolving offsets against a replayed process")
                    .offsets
            };
            let context = ReadContext {
                module_base: module.base,
                // The frame before it, as this reader produced it. Two fields are defined
                // against it — `oldGameState` and `lightRadiusChanged` — and passing None
                // every time made both differ on every frame after the first, which read
                // as a reader bug and was a harness one.
                previous: previous.clone(),
                loaded_mod: Mod::None,
                current_server: frame
                    .state
                    .get("currentServer")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            };

            frames += 1;
            let state = match read_state(&process, &resolved, &context) {
                Ok(state) => state,
                Err(error) => {
                    mismatched += 1;
                    if first_report.is_empty() {
                        // The error itself, not just that there was one: "could not read
                        // the frame" is what this said at first, and it cost a debugging
                        // round to find out which chain had given up.
                        first_report = format!(
                            "{} frame {}: the Rust reader could not read the frame: {error}",
                            path.display(),
                            frame.frame
                        );
                    }
                    continue;
                }
            };
            previous = Some(state.clone());

            let ours = serde_json::to_value(&state).expect("the state serialises");
            let mut found = BTreeMap::new();
            differences(&frame.state, &ours, "", &mut found);
            if !found.is_empty() {
                mismatched += 1;
                if first_report.is_empty() {
                    let mut lines: Vec<String> = found
                        .iter()
                        .take(20)
                        .map(|(field, (left, right))| {
                            format!("  {field}: electron={left} rust={right}")
                        })
                        .collect();
                    if found.len() > 20 {
                        lines.push(format!("  … and {} more", found.len() - 20));
                    }
                    first_report = format!(
                        "{} frame {}: {} field(s) differ\n{}",
                        path.display(),
                        frame.frame,
                        found.len(),
                        lines.join("\n")
                    );
                }
            }
        }
    }

    assert!(frames > 0, "the recordings held no replayable frames");
    assert_eq!(
        mismatched, 0,
        "gate G1: {mismatched} of {frames} frames differ.\n{first_report}"
    );
    eprintln!("gate G1: {frames} frames, no differences");
}

#[test]
fn the_comparison_notices_what_it_is_supposed_to() {
    // The harness has to be able to fail. Two states that differ by one field, one array
    // length and one float past the tolerance must all be reported — otherwise a green
    // parity run means nothing.
    let electron = serde_json::json!({
        "gameState": 1,
        "players": [{ "name": "a", "x": 1.0 }],
        "map": 2
    });
    let rust = serde_json::json!({
        "gameState": 1,
        "players": [{ "name": "b", "x": 1.000_5 }, { "name": "c", "x": 0.0 }],
        "map": 3
    });

    let mut found = BTreeMap::new();
    differences(&electron, &rust, "", &mut found);

    assert!(found.contains_key("map"), "{found:?}");
    assert!(found.contains_key("players.length"), "{found:?}");
    assert!(found.contains_key("players[0].name"), "{found:?}");
    assert!(found.contains_key("players[0].x"), "{found:?}");
}

#[test]
fn a_float_within_the_gates_tolerance_is_not_a_difference() {
    // The one allowance, and only for positions: JSON round-trips a float through decimal.
    let electron = serde_json::json!({ "players": [{ "x": 1.000_000_1, "y": -3.5 }] });
    let rust = serde_json::json!({ "players": [{ "x": 1.000_000_2, "y": -3.5 }] });

    let mut found = BTreeMap::new();
    differences(&electron, &rust, "", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // But the same slack does not apply to anything else. A light radius that is nearly
    // right is still wrong.
    let electron = serde_json::json!({ "lightRadius": 1.0 });
    let rust = serde_json::json!({ "lightRadius": 1.000_000_1 });
    let mut found = BTreeMap::new();
    differences(&electron, &rust, "", &mut found);
    assert!(found.contains_key("lightRadius"), "{found:?}");
}

#[test]
fn base64_round_trips_what_the_recorder_writes() {
    // The decoder is hand-written to keep a dependency out of the reader's tests, so it
    // needs its own check. These are the shapes the recorder produces: every padding
    // case, and a byte range that covers the whole table.
    for length in 0..64usize {
        let bytes: Vec<u8> = (0..length)
            .map(|index| u8::try_from(index * 7 % 256).unwrap_or(0))
            .collect();
        let encoded = encode_base64(&bytes);
        assert_eq!(decode_base64(&encoded).as_deref(), Some(bytes.as_slice()));
    }
    assert!(decode_base64("not base64!").is_none());
}

/// Encodes bytes the way `Buffer.toString('base64')` does, for the round-trip test.
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let packed =
            (u32::from(buffer[0]) << 16) | (u32::from(buffer[1]) << 8) | u32::from(buffer[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                let position = ((packed >> (18 - index * 6)) & 0x3f) as usize;
                out.push(char::from(TABLE[position]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
