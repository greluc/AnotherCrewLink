//! The offsets bundle: what it looks like, and what has to be true of it.
//!
//! The format is the one gate G0 proved, so this is a consumer rather than a design. What
//! it does add is that the structural check runs on **every** load — from the network,
//! from the cache, and from the floor compiled into the binary. The bundle carries no
//! signature, so the validator and the embedded floor are the whole of the check, and a
//! cache hit is exactly the path that would otherwise never be examined again.
//!
//! Every bound below is derived from the forty-four real offsets files in
//! `test/fixtures/offsets`, and the same numbers are enforced by the TypeScript validator
//! the Electron client uses. The two implementations have to agree, or a bundle that one
//! accepts and the other refuses splits the fleet.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Distinct reasons a bundle was refused, so a rejection says which rule it broke.
///
/// The same set as the TypeScript validator's, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// A required field is absent. A truncated download that still parses lands here.
    MissingField,
    /// A field is present with the wrong shape.
    WrongType,
    /// A pointer chain step is outside the module.
    ChainOutOfRange,
    /// A chain is longer than any real one.
    ChainTooLong,
    /// `player.bufferLength` would size an absurd allocation.
    BufferLengthAbsurd,
    /// A function offset is outside the module.
    RvaOutOfModule,
    /// A signature is not a byte pattern, or its offsets are not small steps.
    BadSignature,
    /// A `player.struct` entry is not a field description.
    BadStruct,
    /// A bundle older than the one already held.
    BundleVersionReplayed,
    /// A bundle that needs a newer client than this one.
    ClientTooOld,
}

/// A bundle that was not believed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("offsets rejected ({code:?}) at {path}: {detail}")]
pub struct Rejected {
    /// Which rule was broken.
    pub code: Rejection,
    /// Where in the bundle.
    pub path: String,
    /// What was wrong.
    pub detail: String,
}

impl Rejected {
    fn new(code: Rejection, path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            detail: detail.into(),
        }
    }
}

/// The widest module-relative address a bundle may name.
///
/// The largest value in the real corpus is 0x238A6E0, about 37 MB into
/// `GameAssembly.dll`. This is fourteen times that.
pub const MAX_MODULE_RVA: i64 = 0x2000_0000;

/// Chains use -1 for "not present on this build". Twenty of the forty-four real files do.
pub const MIN_CHAIN_VALUE: i64 = -1;

/// The longest real chain is five.
pub const MAX_CHAIN_LENGTH: usize = 16;

/// Real `bufferLength` values are 56 to 136. This sizes a read buffer, which is why it is
/// bounded rather than merely typed.
pub const MIN_BUFFER_LENGTH: i64 = 8;
/// See [`MIN_BUFFER_LENGTH`].
pub const MAX_BUFFER_LENGTH: i64 = 4096;

/// Real `patternOffset` values are 0 to 10, and `addressOffset` is -5, 0 or 4. Bounding
/// them tightly is what stops a signature turning a pattern match into an arbitrary
/// address.
pub const MAX_PATTERN_OFFSET: i64 = 256;
/// See [`MAX_PATTERN_OFFSET`].
pub const MAX_ADDRESS_OFFSET: i64 = 64;

/// One signature from a bundle.
///
/// Every field is optional because ninety of the six hundred and sixteen signature
/// entries in the real corpus are `{}` — the x64 files carry no pattern for the four
/// write-path functions, since writing is 32-bit only. A validator built from the
/// TypeScript interface rather than from the data would reject twenty real files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureEntry {
    /// The byte pattern, in the bundle's `"48 8B ? ?"` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    /// How far into the match the address is taken from.
    #[serde(
        rename = "patternOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pattern_offset: Option<i64>,
    /// How far to step from there.
    #[serde(
        rename = "addressOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub address_offset: Option<i64>,
}

impl SignatureEntry {
    /// Whether this entry carries a pattern at all.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.sig.is_some()
    }
}

/// Reads a number that should be an integer but may not be.
///
/// Only the five dead write-path fields use this. They are never read, and a bundle is
/// not worth refusing over a value nothing consumes — a float out of range saturates
/// rather than failing, which is what an unused field deserves.
fn lenient_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number.as_i64().unwrap_or_else(|| {
            // `as_f64` covers both a float and an integer past i64, and the cast
            // saturates rather than wrapping.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "saturating is the point: the value is never read"
            )]
            {
                number.as_f64().unwrap_or(0.0) as i64
            }
        }),
        _ => 0,
    })
}

/// One field of the player struct the reader parses out of a buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructField {
    /// One of the type names structron uses.
    #[serde(rename = "type")]
    pub kind: String,
    /// What the field is called.
    pub name: String,
    /// How many bytes to skip, for `SKIP`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<i64>,
}

/// The type names that appear in real data, plus the rest of structron's set.
const STRUCT_TYPES: [&str; 12] = [
    "INT",
    "INT_BE",
    "UINT",
    "UINT_BE",
    "SHORT",
    "SHORT_BE",
    "USHORT",
    "USHORT_BE",
    "FLOAT",
    "CHAR",
    "BYTE",
    "SKIP",
];

/// The player layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerOffsets {
    /// How many bytes one player record takes.
    #[serde(rename = "bufferLength")]
    pub buffer_length: i64,
    /// How the bytes of that record are laid out.
    #[serde(rename = "struct")]
    pub fields: Vec<StructField>,
    /// Everything else is a chain, kept by name so an unknown field is carried rather than
    /// dropped.
    #[serde(flatten)]
    pub rest: BTreeMap<String, serde_json::Value>,
}

/// One offsets file, as the bundle ships it.
///
/// The fields the reader uses are named; the rest are carried in `rest` so a build that
/// adds one is not silently truncated by a round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offsets {
    /// Where the join function was, in a bundle written for the removed write path.
    ///
    /// Read leniently and never used. A 32-bit client scanned for these and, when the
    /// signature missed, wrote an unsigned wrap of a negative number — 1.8446744073709552e19,
    /// a float, in a field this struct reads as an integer. Refusing the bundle over it
    /// made gate G1 discard a whole recording and blame the reader.
    #[serde(rename = "connectFunc", deserialize_with = "lenient_i64")]
    pub connect_func: i64,
    /// See [`Offsets::connect_func`].
    #[serde(rename = "fixedUpdateFunc", deserialize_with = "lenient_i64")]
    pub fixed_update_func: i64,
    /// See [`Offsets::connect_func`].
    #[serde(rename = "showModStampFunc", deserialize_with = "lenient_i64")]
    pub show_mod_stamp_func: i64,
    /// See [`Offsets::connect_func`].
    #[serde(rename = "modLateUpdateFunc", deserialize_with = "lenient_i64")]
    pub mod_late_update_func: i64,
    /// See [`Offsets::connect_func`].
    #[serde(rename = "pingMessageString", deserialize_with = "lenient_i64")]
    pub ping_message_string: i64,
    /// Whether this build's writes are disabled.
    #[serde(rename = "disableWriting")]
    pub disable_writing: bool,
    /// Whether this build has the older meeting HUD layout.
    #[serde(rename = "oldMeetingHud")]
    pub old_meeting_hud: bool,
    /// Whether this build has the newer game options layout.
    #[serde(rename = "newGameOptions")]
    pub new_game_options: bool,
    /// The player record layout.
    pub player: PlayerOffsets,
    /// The byte patterns.
    pub signatures: BTreeMap<String, SignatureEntry>,
    /// Every other top-level field, carried unchanged.
    #[serde(flatten)]
    pub rest: BTreeMap<String, serde_json::Value>,
}

/// The five fields that become addresses.
///
/// `GameReader` overwrites four of them with pattern-scan results before use, so the
/// numbers here are placeholders — the real values in the corpus are 255 to 4095, far too
/// small to be functions. What a bundle actually steers is the *signature*, which is why
/// the tight bounds are on [`MAX_PATTERN_OFFSET`] and [`MAX_ADDRESS_OFFSET`] and the
/// module-range check lives at the point the resolved address is produced.
const RVA_FIELDS: [&str; 5] = [
    "connectFunc",
    "fixedUpdateFunc",
    "showModStampFunc",
    "modLateUpdateFunc",
    "pingMessageString",
];

/// The signature names every real file carries.
const SIGNATURE_NAMES: [&str; 14] = [
    "innerNetClient",
    "meetingHud",
    "gameData",
    "shipStatus",
    "miniGame",
    "palette",
    "playerControl",
    "connectFunc",
    "fixedUpdateFunc",
    "pingMessageString",
    "serverManager",
    "showModStamp",
    "modLateUpdate",
    "gameOptionsManager",
];

impl Offsets {
    /// Checks the structure, the ranges and the pattern syntax.
    ///
    /// # Errors
    ///
    /// Returns [`Rejected`] naming the rule that was broken.
    pub fn validate(&self) -> Result<(), Rejected> {
        for (name, value) in [
            ("connectFunc", self.connect_func),
            ("fixedUpdateFunc", self.fixed_update_func),
            ("showModStampFunc", self.show_mod_stamp_func),
            ("modLateUpdateFunc", self.mod_late_update_func),
            ("pingMessageString", self.ping_message_string),
        ] {
            if !(0..=MAX_MODULE_RVA).contains(&value) {
                return Err(Rejected::new(
                    Rejection::RvaOutOfModule,
                    format!("offsets.{name}"),
                    format!("{value} is outside 0..{MAX_MODULE_RVA:#x}"),
                ));
            }
        }

        if !(MIN_BUFFER_LENGTH..=MAX_BUFFER_LENGTH).contains(&self.player.buffer_length) {
            return Err(Rejected::new(
                Rejection::BufferLengthAbsurd,
                "offsets.player.bufferLength",
                format!(
                    "{} is outside {MIN_BUFFER_LENGTH}..{MAX_BUFFER_LENGTH}",
                    self.player.buffer_length
                ),
            ));
        }

        let mut described = 0i64;
        for (index, field) in self.player.fields.iter().enumerate() {
            let at = format!("offsets.player.struct[{index}]");
            if !STRUCT_TYPES.contains(&field.kind.as_str()) {
                return Err(Rejected::new(
                    Rejection::BadStruct,
                    format!("{at}.type"),
                    format!("{:?} is not a known field type", field.kind),
                ));
            }
            if field.kind == "SKIP" {
                let skip = field.skip.ok_or_else(|| {
                    Rejected::new(
                        Rejection::MissingField,
                        format!("{at}.skip"),
                        "field is absent",
                    )
                })?;
                if !(0..=MAX_BUFFER_LENGTH).contains(&skip) {
                    return Err(Rejected::new(
                        Rejection::BadStruct,
                        format!("{at}.skip"),
                        format!("{skip} is outside 0..{MAX_BUFFER_LENGTH}"),
                    ));
                }
                described += skip;
            } else {
                described += 1;
            }
        }
        // The struct is parsed out of a buffer of `bufferLength` bytes. A description
        // longer than the buffer is a mistake or an attempted over-read.
        if described > self.player.buffer_length {
            return Err(Rejected::new(
                Rejection::BadStruct,
                "offsets.player.struct",
                format!(
                    "describes at least {described} bytes of a {} byte buffer",
                    self.player.buffer_length
                ),
            ));
        }

        for name in SIGNATURE_NAMES {
            let entry = self.signatures.get(name).ok_or_else(|| {
                Rejected::new(
                    Rejection::MissingField,
                    format!("offsets.signatures.{name}"),
                    "field is absent",
                )
            })?;
            validate_signature(entry, &format!("offsets.signatures.{name}"))?;
        }

        for (name, value) in &self.player.rest {
            validate_value(value, &format!("offsets.player.{name}"))?;
        }
        for (name, value) in &self.rest {
            if RVA_FIELDS.contains(&name.as_str()) {
                continue;
            }
            validate_value(value, &format!("offsets.{name}"))?;
        }
        Ok(())
    }
}

/// Checks a chain, an integer offset, or a nested object of them.
fn validate_value(value: &serde_json::Value, path: &str) -> Result<(), Rejected> {
    match value {
        serde_json::Value::Array(steps) => {
            if steps.len() > MAX_CHAIN_LENGTH {
                return Err(Rejected::new(
                    Rejection::ChainTooLong,
                    path,
                    format!("{} steps, limit is {MAX_CHAIN_LENGTH}", steps.len()),
                ));
            }
            for (index, step) in steps.iter().enumerate() {
                let at = format!("{path}[{index}]");
                let Some(step) = step.as_i64() else {
                    return Err(Rejected::new(
                        Rejection::WrongType,
                        at,
                        "expected an integer",
                    ));
                };
                if !(MIN_CHAIN_VALUE..=MAX_MODULE_RVA).contains(&step) {
                    return Err(Rejected::new(
                        Rejection::ChainOutOfRange,
                        at,
                        format!("{step} is outside {MIN_CHAIN_VALUE}..{MAX_MODULE_RVA:#x}"),
                    ));
                }
            }
            Ok(())
        }
        serde_json::Value::Number(number) => {
            let Some(offset) = number.as_i64() else {
                return Err(Rejected::new(
                    Rejection::WrongType,
                    path,
                    "expected an integer",
                ));
            };
            if !(MIN_CHAIN_VALUE..=MAX_MODULE_RVA).contains(&offset) {
                return Err(Rejected::new(
                    Rejection::ChainOutOfRange,
                    path,
                    format!("{offset} is outside the module"),
                ));
            }
            Ok(())
        }
        serde_json::Value::Object(fields) => {
            for (name, child) in fields {
                validate_value(child, &format!("{path}.{name}"))?;
            }
            Ok(())
        }
        // Booleans and strings are carried; nulls are how a build says "not here".
        _ => Ok(()),
    }
}

/// Checks one signature entry, allowing the empty one that real x64 files carry.
fn validate_signature(entry: &SignatureEntry, path: &str) -> Result<(), Rejected> {
    let Some(sig) = entry.sig.as_deref() else {
        // `{}`. Ninety of the six hundred and sixteen entries in the corpus.
        return Ok(());
    };
    if crate::scan::Pattern::parse(sig).is_err() {
        return Err(Rejected::new(
            Rejection::BadSignature,
            format!("{path}.sig"),
            format!("{sig:?} is not a byte pattern"),
        ));
    }

    let pattern_offset = entry.pattern_offset.ok_or_else(|| {
        Rejected::new(
            Rejection::MissingField,
            format!("{path}.patternOffset"),
            "field is absent",
        )
    })?;
    if !(0..=MAX_PATTERN_OFFSET).contains(&pattern_offset) {
        return Err(Rejected::new(
            Rejection::BadSignature,
            format!("{path}.patternOffset"),
            format!("{pattern_offset} is outside 0..{MAX_PATTERN_OFFSET}"),
        ));
    }

    let address_offset = entry.address_offset.ok_or_else(|| {
        Rejected::new(
            Rejection::MissingField,
            format!("{path}.addressOffset"),
            "field is absent",
        )
    })?;
    if !(-MAX_ADDRESS_OFFSET..=MAX_ADDRESS_OFFSET).contains(&address_offset) {
        return Err(Rejected::new(
            Rejection::BadSignature,
            format!("{path}.addressOffset"),
            format!("{address_offset} is outside ±{MAX_ADDRESS_OFFSET}"),
        ));
    }
    Ok(())
}

/// One entry of the lookup: which offsets file a game build needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupEntry {
    /// The game build this describes.
    pub version: String,
    /// The offsets file, relative to `offsets/<arch>/`.
    pub file: String,
    /// Which revision of that file.
    #[serde(rename = "offsetsVersion")]
    pub offsets_version: i64,
}

/// The lookup that maps a game build to an offsets file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lookup {
    /// Keyed by the build hash the game broadcasts, plus `default`.
    pub versions: BTreeMap<String, LookupEntry>,
    /// The patterns used to find that hash in the first place.
    pub patterns: serde_json::Value,
    /// Moves whenever the contents do; a lower one arriving is a rollback.
    #[serde(
        rename = "bundle_version",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bundle_version: Option<i64>,
    /// The oldest client that can read this bundle.
    #[serde(
        rename = "min_client_version",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_client_version: Option<String>,
    /// Which upstream commit the offsets came from.
    #[serde(
        rename = "upstream_commit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub upstream_commit: Option<String>,
}

/// A relative path inside the offsets tree, ending in `.json`. Nothing else is fetched.
///
/// Case-sensitively, and clippy is told so on purpose. The TypeScript validator matches
/// `\.json$`, and the whole point of this function is to accept and refuse exactly what
/// that one does. A bundle that one client fetches and the other will not is worse than a
/// bundle neither fetches.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_relative_json_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains("..")
        && !path.contains(':')
        && !path.starts_with('/')
        && path.ends_with(".json")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// What the caller knows that the bundle cannot: who is reading it, and what came before.
#[derive(Debug, Clone, Default)]
pub struct BundleContext {
    /// The running client, for `min_client_version`.
    pub client_version: String,
    /// The `bundle_version` already held, if any.
    pub held_bundle_version: Option<i64>,
}

impl Lookup {
    /// Checks the structure, the file paths and the envelope.
    ///
    /// # Errors
    ///
    /// Returns [`Rejected`] naming the rule that was broken.
    pub fn validate(&self, context: &BundleContext) -> Result<(), Rejected> {
        if !self.versions.contains_key("default") {
            // Every unrecognised game build falls back to `default`. Losing it is an
            // outage for exactly the players a new Among Us release just created.
            return Err(Rejected::new(
                Rejection::MissingField,
                "lookup.versions.default",
                "the fallback entry is absent",
            ));
        }
        for (id, entry) in &self.versions {
            let at = format!("lookup.versions.{id}");
            if !is_relative_json_path(&entry.file) {
                // The path is interpolated into a URL. A traversal or an absolute URL
                // here redirects the fetch to a host of the author's choosing, which is a
                // more direct route than any wrong number.
                return Err(Rejected::new(
                    Rejection::WrongType,
                    format!("{at}.file"),
                    format!("{:?} is not a relative .json path", entry.file),
                ));
            }
            if entry.offsets_version < 0 {
                return Err(Rejected::new(
                    Rejection::WrongType,
                    format!("{at}.offsetsVersion"),
                    format!("{} is negative", entry.offsets_version),
                ));
            }
        }

        if let Some(bundle_version) = self.bundle_version {
            if bundle_version < 0 {
                return Err(Rejected::new(
                    Rejection::WrongType,
                    "lookup.bundle_version",
                    format!("{bundle_version} is negative"),
                ));
            }
            if let Some(held) = context.held_bundle_version
                && bundle_version < held
            {
                // How someone who once got a bad file onto the mirror gets it back after
                // it is reverted.
                return Err(Rejected::new(
                    Rejection::BundleVersionReplayed,
                    "lookup.bundle_version",
                    format!("{bundle_version} is older than the {held} already held"),
                ));
            }
        }

        if let Some(minimum) = self.min_client_version.as_deref()
            && !context.client_version.is_empty()
            && compare_versions(&context.client_version, minimum) < 0
        {
            return Err(Rejected::new(
                Rejection::ClientTooOld,
                "lookup.min_client_version",
                format!(
                    "bundle needs {minimum}, this client is {}",
                    context.client_version
                ),
            ));
        }
        Ok(())
    }

    /// The entry for a game build, falling back to `default`.
    #[must_use]
    pub fn entry_for(&self, build: &str) -> Option<&LookupEntry> {
        self.versions
            .get(build)
            .or_else(|| self.versions.get("default"))
    }
}

/// Compares two dotted numeric versions. Anything after the numbers is ignored.
///
/// Deliberately not semver: the question is only "is this client older than the bundle
/// asks for", and a pre-release suffix on our own version should not make a client look
/// older than it is.
#[must_use]
pub fn compare_versions(left: &str, right: &str) -> i32 {
    let parse = |text: &str| -> Vec<i64> {
        text.split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let a = parse(left);
    let b = parse(right);
    for index in 0..a.len().max(b.len()) {
        let difference = a.get(index).copied().unwrap_or(0) - b.get(index).copied().unwrap_or(0);
        if difference != 0 {
            return if difference < 0 { -1 } else { 1 };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/offsets")
    }

    fn offset_files() -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(fixtures())
            .expect("the fixtures directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with("__offsets.json"))
            .collect();
        found.sort();
        found
    }

    fn load_offsets(path: &PathBuf) -> Offsets {
        let text = std::fs::read_to_string(path).expect("a fixture");
        serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} did not parse: {error}", path.display()))
    }

    // Gate G0's second half, in Rust this time: the validator must accept every real file
    // unchanged. The two implementations have to agree, or a bundle one accepts and the
    // other refuses splits the fleet.
    #[test]
    fn accepts_every_real_offsets_file() {
        let files = offset_files();
        assert!(files.len() >= 40, "found only {} fixtures", files.len());
        for path in files {
            let offsets = load_offsets(&path);
            offsets
                .validate()
                .unwrap_or_else(|error| panic!("{} was rejected: {error}", path.display()));
        }
    }

    #[test]
    fn a_round_trip_loses_nothing() {
        // The reader carries fields it does not name, so a build that adds one is not
        // silently truncated.
        for path in offset_files().into_iter().take(5) {
            let original: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let parsed: Offsets = serde_json::from_value(original.clone()).unwrap();
            let round_tripped = serde_json::to_value(&parsed).unwrap();
            assert_eq!(round_tripped, original, "{} lost a field", path.display());
        }
    }

    #[test]
    fn accepts_the_real_lookup() {
        let text = std::fs::read_to_string(fixtures().join("lookup.json")).unwrap();
        let lookup: Lookup = serde_json::from_str(&text).expect("the lookup parses");
        lookup
            .validate(&BundleContext {
                client_version: "1.0.3".to_owned(),
                held_bundle_version: None,
            })
            .expect("the real lookup is accepted");
        assert!(lookup.bundle_version.is_some());
        assert!(
            lookup.entry_for("no-such-build").is_some(),
            "falls back to default"
        );
    }

    fn sample() -> Offsets {
        load_offsets(&offset_files()[0])
    }

    #[test]
    fn refuses_an_rva_outside_the_module() {
        let mut offsets = sample();
        offsets.fixed_update_func = 0x7fff_ffff;
        assert_eq!(
            offsets.validate().unwrap_err().code,
            Rejection::RvaOutOfModule
        );
    }

    #[test]
    fn refuses_an_absurd_buffer_length() {
        let mut offsets = sample();
        offsets.player.buffer_length = 0x4000_0000;
        assert_eq!(
            offsets.validate().unwrap_err().code,
            Rejection::BufferLengthAbsurd
        );
    }

    #[test]
    fn refuses_a_chain_step_outside_the_module() {
        let mut offsets = sample();
        offsets.rest.insert(
            "allPlayersPtr".to_owned(),
            serde_json::json!([0x10, 0x7fff_fff0i64]),
        );
        assert_eq!(
            offsets.validate().unwrap_err().code,
            Rejection::ChainOutOfRange
        );
    }

    #[test]
    fn accepts_minus_one_as_not_present_on_this_build() {
        // Twenty of the forty-four real files use it. A naive non-negative check would
        // have been an outage.
        let mut offsets = sample();
        offsets
            .rest
            .insert("meetingHud".to_owned(), serde_json::json!([-1, 92, 0]));
        assert!(offsets.validate().is_ok());
    }

    #[test]
    fn refuses_a_signature_that_is_not_a_byte_pattern() {
        let mut offsets = sample();
        offsets.signatures.insert(
            "innerNetClient".to_owned(),
            SignatureEntry {
                sig: Some("ZZ 90".to_owned()),
                pattern_offset: Some(0),
                address_offset: Some(0),
            },
        );
        assert_eq!(
            offsets.validate().unwrap_err().code,
            Rejection::BadSignature
        );
    }

    #[test]
    fn accepts_the_empty_signature_entries_real_x64_files_carry() {
        // Ninety of six hundred and sixteen. A validator built from the TypeScript
        // interface rather than from the data would reject twenty real files.
        let mut offsets = sample();
        offsets
            .signatures
            .insert("connectFunc".to_owned(), SignatureEntry::default());
        assert!(offsets.validate().is_ok());
    }

    #[test]
    fn refuses_a_signature_offset_that_is_not_a_small_step() {
        let mut offsets = sample();
        offsets.signatures.insert(
            "innerNetClient".to_owned(),
            SignatureEntry {
                sig: Some("48 8B".to_owned()),
                pattern_offset: Some(0x7fff_fff0),
                address_offset: Some(0),
            },
        );
        assert_eq!(
            offsets.validate().unwrap_err().code,
            Rejection::BadSignature
        );
    }

    #[test]
    fn refuses_a_struct_that_over_reads_its_buffer() {
        let mut offsets = sample();
        offsets.player.buffer_length = 64;
        offsets.player.fields = vec![StructField {
            kind: "SKIP".to_owned(),
            name: "over".to_owned(),
            skip: Some(4096),
        }];
        assert_eq!(offsets.validate().unwrap_err().code, Rejection::BadStruct);
    }

    #[test]
    fn refuses_a_lookup_path_that_leaves_the_offsets_tree() {
        let text = std::fs::read_to_string(fixtures().join("lookup.json")).unwrap();
        let mut lookup: Lookup = serde_json::from_str(&text).unwrap();
        for bad in [
            "../../../etc/passwd.json",
            "https://evil.example/offsets.json",
            "/etc/passwd.json",
        ] {
            lookup.versions.get_mut("default").unwrap().file = bad.to_owned();
            assert_eq!(
                lookup.validate(&BundleContext::default()).unwrap_err().code,
                Rejection::WrongType,
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn refuses_a_replayed_bundle_and_a_client_that_is_too_old() {
        let text = std::fs::read_to_string(fixtures().join("lookup.json")).unwrap();
        let lookup: Lookup = serde_json::from_str(&text).unwrap();

        let replay = lookup.validate(&BundleContext {
            client_version: "1.0.3".to_owned(),
            held_bundle_version: Some(99),
        });
        assert_eq!(replay.unwrap_err().code, Rejection::BundleVersionReplayed);

        let old = lookup.validate(&BundleContext {
            client_version: "0.9.0".to_owned(),
            held_bundle_version: None,
        });
        assert_eq!(old.unwrap_err().code, Rejection::ClientTooOld);
    }

    #[test]
    fn a_bundle_without_the_envelope_is_data_rather_than_an_error() {
        // Clients in the field predate these fields, and a mirror mid-rollout is not an
        // outage.
        let text = std::fs::read_to_string(fixtures().join("lookup.json")).unwrap();
        let mut lookup: Lookup = serde_json::from_str(&text).unwrap();
        lookup.bundle_version = None;
        lookup.min_client_version = None;
        assert!(
            lookup
                .validate(&BundleContext {
                    client_version: "0.0.1".to_owned(),
                    held_bundle_version: Some(99),
                })
                .is_ok()
        );
    }

    #[test]
    fn compares_versions_the_way_the_typescript_does() {
        assert!(compare_versions("1.0.3", "1.0.0") > 0);
        assert!(compare_versions("1.0.0", "1.0.3") < 0);
        assert_eq!(compare_versions("1.0.3", "1.0.3"), 0);
        // Missing components are zero.
        assert_eq!(compare_versions("1.0", "1.0.0"), 0);

        // A pre-release suffix is *not* handled the way semver would, and this asserts
        // the quirk rather than the ideal: "1.0.3-beta.1" splits on dots into four
        // components, so it sorts *above* "1.0.3". The TypeScript does exactly the same,
        // which is what matters — the two implementations gate the same bundles, and one
        // being cleverer than the other is how a bundle gets accepted on one client and
        // refused on the next. Neither project has ever published a pre-release tag.
        assert!(compare_versions("1.0.3-beta.1", "1.0.3") > 0);
        assert_eq!(compare_versions("1.0.3-beta", "1.0.3"), 0);
    }
}
