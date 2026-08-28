//! Custom launch platforms, as the settings file holds them and as a person edits them.
//!
//! `customPlatforms` is a map from a title somebody typed to the four fields
//! `GamePlatformInstance` carries. Reading one has worked since the launch button existed;
//! this is the other half, and the shapes do not line up by themselves.
//!
//! # What the file holds and what a person types
//!
//! They are not the same thing, and `CustomPlatformSettings.tsx` is where the difference
//! lives. A person picks **one path to a program**. The file keeps a *directory* in
//! `runPath` and the program's *own name* as the first entry of `execute`, because that is
//! what starting it needs: a game launched from somewhere else looks for its data relative
//! to the working directory and does not find it.
//!
//! Arguments are the rest of `execute`, and the screen shows them as one line because that
//! is how somebody thinks of them.
//!
//! A URI platform has no program at all: the address goes in `runPath` and `execute` is
//! `[""]` — an entry that is there and empty, which is what the shipped client writes and
//! what [`crate::worn`]'s neighbour `start_game::plan` refuses to treat as a program.
//!
//! The conversions are here, with the tests. The drawing is [`crate::views::platforms`].

/// One custom platform, in the shape the screen shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// The title its owner gave it, which is also its key in the file.
    pub name: String,
    /// Whether it is started through a URI rather than as a program.
    pub is_uri: bool,
    /// The URI, or the full path to the program — one field, because a person has one.
    pub path: String,
    /// Everything after the program, as one line.
    pub arguments: String,
}

/// How the file keeps one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// `runPath`: the URI, or the directory the program lives in.
    pub run_path: String,
    /// `execute`: the program's own name, then its arguments.
    pub execute: Vec<String>,
}

/// Splits a full path into the directory and the program's own name.
///
/// Both separators, because a path somebody pasted on Windows can have either and a
/// settings field is exactly where a pasted path arrives. A path with no separator at all
/// is all program and no directory, which `start_game::plan` then refuses — better than
/// inventing a directory it might run in.
#[must_use]
pub fn split_program(full: &str) -> (String, String) {
    match full.rfind(['\\', '/']) {
        Some(at) => (full[..at].to_owned(), full[at + 1..].to_owned()),
        None => (String::new(), full.to_owned()),
    }
}

/// Puts the two back together for showing.
#[must_use]
pub fn join_program(directory: &str, program: &str) -> String {
    if directory.is_empty() {
        return program.to_owned();
    }
    format!("{}\\{program}", directory.trim_end_matches(['\\', '/']))
}

/// What the file should hold for what somebody typed.
#[must_use]
pub fn to_stored(entry: &Entry) -> Stored {
    if entry.is_uri {
        return Stored {
            run_path: entry.path.trim().to_owned(),
            // `[""]`, not `[]`. `GamePlatform.ts` writes an entry that is there and empty
            // for its two URI platforms, and something reading `execute[0]` should find the
            // same nothing in both generations.
            execute: vec![String::new()],
        };
    }
    let (directory, program) = split_program(entry.path.trim());
    let mut execute = vec![program];
    execute.extend(entry.arguments.split_whitespace().map(ToOwned::to_owned));
    Stored {
        run_path: directory,
        execute,
    }
}

/// What the screen should show for what the file holds.
#[must_use]
pub fn to_entry(name: &str, is_uri: bool, stored: &Stored) -> Entry {
    let (path, arguments) = if is_uri {
        (stored.run_path.clone(), String::new())
    } else {
        let program = stored.execute.first().map_or("", String::as_str);
        (
            join_program(&stored.run_path, program),
            stored
                .execute
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    Entry {
        name: name.to_owned(),
        is_uri,
        path,
        arguments,
    }
}

/// Whether a title can be used as one.
///
/// Empty is refused because the key is the title, and a platform with no name cannot be
/// chosen or deleted. One of the three built-in keys is refused because
/// `Platform::from_key` is asked first, so a custom entry called `STEAM` would be shadowed
/// by the real Steam and could never be started — a name that silently does nothing.
#[must_use]
pub fn is_a_usable_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.contains('.')
        && acl_types::platform::Platform::from_key(name).is_none()
}

#[cfg(test)]
mod tests {
    use super::{Entry, Stored, is_a_usable_name, split_program, to_entry, to_stored};

    fn strings(of: &[&str]) -> Vec<String> {
        of.iter().map(|each| (*each).to_owned()).collect()
    }

    /// A program path becomes a directory and a name, which is what starting it needs.
    #[test]
    fn a_program_is_split_the_way_the_shipped_client_splits_it() {
        let entry = Entry {
            name: "Mine".to_owned(),
            is_uri: false,
            path: r"C:\Games\Among Us\Among Us.exe".to_owned(),
            arguments: "--windowed --lang en".to_owned(),
        };
        assert_eq!(
            to_stored(&entry),
            Stored {
                run_path: r"C:\Games\Among Us".to_owned(),
                execute: strings(&["Among Us.exe", "--windowed", "--lang", "en"]),
            }
        );
    }

    /// And back again, so opening the screen shows what was typed into it.
    #[test]
    fn a_stored_platform_comes_back_as_it_was_typed() {
        let stored = Stored {
            run_path: r"C:\Games\Among Us".to_owned(),
            execute: strings(&["Among Us.exe", "--windowed"]),
        };
        let entry = to_entry("Mine", false, &stored);
        assert_eq!(entry.path, r"C:\Games\Among Us\Among Us.exe");
        assert_eq!(entry.arguments, "--windowed");
        // A round trip changes nothing, which is what makes editing one field safe.
        assert_eq!(to_stored(&entry), stored);
    }

    /// A URI keeps its address and gets the empty program entry the file expects.
    #[test]
    fn a_uri_platform_has_an_empty_program_rather_than_none() {
        let entry = Entry {
            name: "Mine".to_owned(),
            is_uri: true,
            path: "myLauncher://play".to_owned(),
            arguments: "ignored".to_owned(),
        };
        assert_eq!(
            to_stored(&entry),
            Stored {
                run_path: "myLauncher://play".to_owned(),
                execute: vec![String::new()],
            }
        );
    }

    /// Either separator, because a pasted path can have either.
    #[test]
    fn both_separators_split() {
        assert_eq!(
            split_program(r"C:\Games\x.exe"),
            (r"C:\Games".to_owned(), "x.exe".to_owned())
        );
        assert_eq!(
            split_program("C:/Games/x.exe"),
            ("C:/Games".to_owned(), "x.exe".to_owned())
        );
        // No separator is all program and no directory, which `start_game::plan` refuses
        // rather than running somewhere it guessed.
        assert_eq!(split_program("x.exe"), (String::new(), "x.exe".to_owned()));
    }

    /// A name has to be usable as a key, and must not shadow a built-in platform.
    #[test]
    fn a_name_that_could_never_be_started_is_refused() {
        assert!(is_a_usable_name("My Launcher"));
        assert!(!is_a_usable_name(""));
        assert!(!is_a_usable_name("   "));
        // `Platform::from_key` is asked first, so this could never be reached.
        assert!(!is_a_usable_name("STEAM"));
        assert!(!is_a_usable_name("EPIC"));
        // A dot is a path separator in the settings file, so a name with one would write
        // its fields into a nested object nothing reads back.
        assert!(!is_a_usable_name("my.launcher"));
    }
}
