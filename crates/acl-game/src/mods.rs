//! Which mod, if any, is installed beside the game — and Valve's `KeyValues` format,
//! which is how the game is found in the first place.
//!
//! Both are ports of code that already exists in the Electron client, with its tests. The
//! VDF parser replaced `vdf-parser`, last published in early 2023, and reads only what
//! Steam's `registry.vdf` actually contains: quoted keys, quoted values, and nested
//! blocks.

use std::collections::BTreeMap;
use std::path::Path;

/// A value in a `KeyValues` file: a string, or a block of more of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    /// A quoted string.
    Text(String),
    /// A nested block.
    Block(BTreeMap<String, VdfValue>),
}

impl VdfValue {
    /// The string at a path of keys, if there is one there.
    ///
    /// `value.get(["Software", "Valve", "Steam"])` walks three blocks.
    #[must_use]
    pub fn get<'a>(&self, path: impl IntoIterator<Item = &'a str>) -> Option<&VdfValue> {
        let mut current = self;
        for key in path {
            let VdfValue::Block(fields) = current else {
                return None;
            };
            current = fields.get(key)?;
        }
        Some(current)
    }

    /// This value as a string, if it is one.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            VdfValue::Text(text) => Some(text),
            VdfValue::Block(_) => None,
        }
    }
}

/// Why a `KeyValues` file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VdfError {
    /// A block opened without a key in front of it.
    #[error("block without a key at byte {0}")]
    BlockWithoutKey(usize),
    /// A closing brace with nothing open.
    #[error("unbalanced closing brace at byte {0}")]
    UnbalancedClose(usize),
    /// The file ended inside a block.
    #[error("unterminated block")]
    Unterminated,
}

/// Parses Valve's `KeyValues` format.
///
/// Comments and unquoted tokens are skipped rather than supported, because Steam does not
/// write them here.
///
/// # Errors
///
/// Returns [`VdfError`] for a stray brace or an unterminated block, both of which would
/// otherwise corrupt the tree silently.
pub fn parse_vdf(input: &str) -> Result<BTreeMap<String, VdfValue>, VdfError> {
    // Blocks under construction, innermost last. Held as owned maps and folded into their
    // parent on close, which avoids the borrow gymnastics a stack of references needs.
    let mut stack: Vec<(Option<String>, BTreeMap<String, VdfValue>)> =
        vec![(None, BTreeMap::new())];
    let mut pending_key: Option<String> = None;

    let bytes = input.as_bytes();
    let mut index = 0usize;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'"' => {
                let (text, next) = read_quoted(input, index);
                index = next;
                match pending_key.take() {
                    None => pending_key = Some(text),
                    Some(key) => {
                        if let Some((_, block)) = stack.last_mut() {
                            block.insert(key, VdfValue::Text(text));
                        }
                    }
                }
            }
            b'{' => {
                let Some(key) = pending_key.take() else {
                    return Err(VdfError::BlockWithoutKey(index));
                };
                stack.push((Some(key), BTreeMap::new()));
                index += 1;
            }
            b'}' => {
                if stack.len() == 1 {
                    return Err(VdfError::UnbalancedClose(index));
                }
                let Some((key, block)) = stack.pop() else {
                    return Err(VdfError::UnbalancedClose(index));
                };
                if let (Some(key), Some((_, parent))) = (key, stack.last_mut()) {
                    parent.insert(key, VdfValue::Block(block));
                }
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                // A line comment. Steam does not write them, but skipping is cheaper than
                // being surprised by one.
                while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }

    if stack.len() != 1 {
        return Err(VdfError::Unterminated);
    }
    Ok(stack.pop().map(|(_, block)| block).unwrap_or_default())
}

/// Reads a quoted string starting at `start`, returning it and the index after it.
fn read_quoted(input: &str, start: usize) -> (String, usize) {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'\\' if index + 1 < bytes.len() => {
                // Steam escapes only backslashes and quotes, and the TypeScript's
                // `replace(/\\(.)/g, '$1')` unescapes whatever follows.
                if let Some(escaped) = input.get(index + 1..).and_then(|rest| rest.chars().next()) {
                    out.push(escaped);
                    index += 1 + escaped.len_utf8();
                } else {
                    index += 1;
                }
            }
            b'"' => return (out, index + 1),
            _ => {
                if let Some(character) = input.get(index..).and_then(|rest| rest.chars().next()) {
                    out.push(character);
                    index += character.len_utf8();
                } else {
                    index += 1;
                }
            }
        }
    }
    (out, index)
}

/// Which mod is loaded, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mod {
    /// No mod, or one this build does not recognise by name.
    None,
    /// Town of Us: Mira.
    TownOfUsMira,
    /// Town of Us: Reactivated.
    TownOfUs,
    /// The Other Roles.
    TheOtherRoles,
    /// Las Monjas.
    LasMonjas,
    /// A `BepInEx` plugin that is not one of the above.
    Other,
}

impl Mod {
    /// The identifier the wire protocol and the settings use.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Mod::None => "NONE",
            Mod::TownOfUsMira => "TOWN_OF_US_MIRA",
            Mod::TownOfUs => "TOWN_OF_US",
            Mod::TheOtherRoles => "THE_OTHER_ROLES",
            Mod::LasMonjas => "LAS_MONJAS",
            Mod::Other => "OTHER",
        }
    }
}

/// The plugin file-name prefixes each mod ships under.
///
/// Order matters: `TownOfUsMira` has to be tested before `TownOfUs`, or the Mira variant
/// is reported as the base mod. The TypeScript's `modList` has the same ordering for the
/// same reason, and it is the kind of thing a reorder breaks silently.
const PREFIXES: [(&str, Mod); 4] = [
    ("TownOfUsMira", Mod::TownOfUsMira),
    ("TownOfUs", Mod::TownOfUs),
    ("TheOtherRoles", Mod::TheOtherRoles),
    ("LasMonjas", Mod::LasMonjas),
];

/// Which mod a plugin file name belongs to.
#[must_use]
pub fn mod_for_plugin(file_name: &str) -> Option<Mod> {
    PREFIXES
        .iter()
        .find(|(prefix, _)| file_name.contains(prefix))
        .map(|(_, which)| *which)
}

/// Looks beside the game executable for a `BepInEx` installation and names the mod.
///
/// A path containing `?\volume` is the shape Windows reports for a game on a drive
/// mounted without a letter; there is nothing to look at beside it, so the answer is
/// [`Mod::None`] rather than an error.
#[must_use]
pub fn detect_mod(game_executable: &Path) -> Mod {
    if game_executable
        .to_string_lossy()
        .to_lowercase()
        .contains("?\\volume")
    {
        return Mod::None;
    }
    let Some(directory) = game_executable.parent() else {
        return Mod::None;
    };
    // Both, not either: winhttp.dll alone is how BepInEx hooks the process, and the
    // plugins directory alone can be left behind by an uninstall.
    if !directory.join("winhttp.dll").exists() || !directory.join("BepInEx/plugins").is_dir() {
        return Mod::None;
    }
    let Ok(entries) = std::fs::read_dir(directory.join("BepInEx/plugins")) else {
        return Mod::None;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(found) = mod_for_plugin(&name) {
            return found;
        }
    }
    Mod::None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn reads_the_shape_steams_registry_has() {
        let parsed = parse_vdf(
            r#"
            "Registry"
            {
                "HKCU"
                {
                    "Software"
                    {
                        "Valve"
                        {
                            "Steam"
                            {
                                "SteamPath"    "C:\\Program Files\\Steam"
                            }
                        }
                    }
                }
            }
            "#,
        )
        .expect("a registry file");

        let root = VdfValue::Block(parsed);
        let path = root
            .get([
                "Registry",
                "HKCU",
                "Software",
                "Valve",
                "Steam",
                "SteamPath",
            ])
            .and_then(VdfValue::as_text);
        assert_eq!(path, Some(r"C:\Program Files\Steam"));
    }

    #[test]
    fn unescapes_the_way_the_typescript_does() {
        let parsed = parse_vdf(r#""k" "a\\b\"c""#).expect("parses");
        assert_eq!(
            parsed.get("k"),
            Some(&VdfValue::Text(r#"a\b"c"#.to_owned()))
        );
    }

    #[test]
    fn skips_a_line_comment() {
        let parsed = parse_vdf("// a comment\n\"k\" \"v\"").expect("parses");
        assert_eq!(parsed.get("k"), Some(&VdfValue::Text("v".to_owned())));
    }

    #[test]
    fn refuses_a_block_without_a_key() {
        assert!(matches!(
            parse_vdf("{ \"k\" \"v\" }").unwrap_err(),
            VdfError::BlockWithoutKey(_)
        ));
    }

    #[test]
    fn refuses_an_unbalanced_file() {
        // Both directions. Either would otherwise corrupt the tree silently.
        assert!(matches!(
            parse_vdf("\"k\" \"v\" }").unwrap_err(),
            VdfError::UnbalancedClose(_)
        ));
        assert!(matches!(
            parse_vdf("\"k\" { \"a\" \"b\"").unwrap_err(),
            VdfError::Unterminated
        ));
    }

    #[test]
    fn an_empty_file_is_an_empty_tree() {
        assert!(parse_vdf("").expect("parses").is_empty());
        assert!(parse_vdf("   \n\t ").expect("parses").is_empty());
    }

    #[test]
    fn names_the_mods_by_their_plugin_files() {
        assert_eq!(
            mod_for_plugin("TheOtherRoles.dll"),
            Some(Mod::TheOtherRoles)
        );
        assert_eq!(mod_for_plugin("LasMonjas.dll"), Some(Mod::LasMonjas));
        assert_eq!(mod_for_plugin("Reactor.dll"), None);
    }

    #[test]
    fn tests_the_mira_variant_before_the_base_mod() {
        // `TownOfUsMira.dll` contains `TownOfUs`, so an unordered search reports the wrong
        // mod. The TypeScript's modList has the same ordering for the same reason, and it
        // is the kind of thing a reorder breaks silently.
        assert_eq!(mod_for_plugin("TownOfUsMira.dll"), Some(Mod::TownOfUsMira));
        assert_eq!(mod_for_plugin("TownOfUs.dll"), Some(Mod::TownOfUs));
    }

    #[test]
    fn the_ids_match_the_wire_protocol() {
        // These strings cross to the renderer and into the settings file, so they are not
        // free to change with the enum.
        assert_eq!(Mod::None.id(), "NONE");
        assert_eq!(Mod::TownOfUsMira.id(), "TOWN_OF_US_MIRA");
        assert_eq!(Mod::TheOtherRoles.id(), "THE_OTHER_ROLES");
    }

    #[test]
    fn a_volume_path_has_nothing_beside_it_to_look_at() {
        // What Windows reports for a game on a drive mounted without a letter.
        assert_eq!(
            detect_mod(Path::new(r"\\?\Volume{1234}\Among Us\Among Us.exe")),
            Mod::None
        );
    }

    #[test]
    fn reports_no_mod_when_bepinex_is_not_installed() {
        let temporary = std::env::temp_dir().join("acl-mod-test-none");
        std::fs::create_dir_all(&temporary).expect("a directory");
        assert_eq!(detect_mod(&temporary.join("Among Us.exe")), Mod::None);
    }

    #[test]
    fn finds_a_mod_beside_the_executable() {
        let root = std::env::temp_dir().join("acl-mod-test-found");
        let plugins = root.join("BepInEx/plugins");
        std::fs::create_dir_all(&plugins).expect("a plugins directory");
        std::fs::write(root.join("winhttp.dll"), b"").expect("the hook");
        std::fs::write(plugins.join("TheOtherRoles.dll"), b"").expect("a plugin");

        assert_eq!(detect_mod(&root.join("Among Us.exe")), Mod::TheOtherRoles);

        // Both are required: a plugins directory left behind by an uninstall, with no
        // winhttp.dll to load it, is not a modded game.
        std::fs::remove_file(root.join("winhttp.dll")).expect("removable");
        assert_eq!(detect_mod(&root.join("Among Us.exe")), Mod::None);

        std::fs::remove_dir_all(&root).ok();
    }
}
