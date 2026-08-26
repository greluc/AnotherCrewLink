#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! The installer's CLI contract, checked as text.
//!
//! §4.9 item 1 says the NSIS script "keeps its exact CLI contract", and §4.12 says why that
//! is not a preference: P8 publishes a bridge build into the 1.x feed, and the **installed
//! fleet's** `electron-updater` is what runs it. That updater spawns
//!
//! ```text
//! installer.exe --updated /S /D=<installDirectory>
//! ```
//!
//! and it is not going to be persuaded otherwise. A script that stopped handling any one of
//! those three would fail on a large number of machines at once, at the only moment this
//! project ever asks them all to run an installer.
//!
//! # Why a text check rather than a build
//!
//! Building the installer needs `makensis`, which is not on a Rust CI runner and would be
//! an install step on every job for a file that changes twice a year. Running it needs
//! Windows and a willing victim. What is left is the contract itself, which is a property
//! of the source — and a property of the source is exactly what a test can hold.
//!
//! This does not prove the installer works. It proves it still claims to do the four things
//! that would strand the fleet if it stopped claiming them, and it fails the moment somebody
//! edits them out. `installer/README.md` carries the manual check that a text test cannot.

use std::path::PathBuf;

/// The script with its comments removed.
///
/// NSIS comments start with `;`, and the script explains itself at length -- including,
/// in one place, by naming the very thing a test below forbids. A check that matched prose
/// would fail on the sentence explaining why the instruction is not there.
fn instructions() -> String {
    let mut kept = String::new();
    for line in script().lines() {
        kept.push_str(line.split(';').next().unwrap_or_default());
        kept.push('\n');
    }
    kept
}

fn script() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../installer/anothercrewlink.nsi");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// `--updated` is electron-builder's flag and the fleet's updater always passes it. A
/// script that does not read it treats every update as a first install — which here means
/// opening a window the user had closed.
#[test]
fn the_updated_flag_is_read() {
    let script = script();
    assert!(
        script.contains("--updated"),
        "the installer no longer reads --updated"
    );
    assert!(
        script.contains("${GetOptions}"),
        "something reads --updated, but not with the parser that handles it"
    );
}

/// `/S` is NSIS's own silent flag, and the updater always passes it. What has to be true is
/// that a silent install does not start a GUI: a silent install that opened a window is not
/// a silent install, and it happens on somebody else's schedule.
#[test]
fn a_silent_install_starts_nothing() {
    let script = script();
    assert!(
        script.contains("${IfNot} ${Silent}"),
        "the launch is no longer guarded on silence"
    );
    let launch = script
        .find("Exec '\"$INSTDIR\\anothercrewlink.exe\"'")
        .expect("the installer still launches the app somewhere");
    let guard = script
        .find("${IfNot} ${Silent}")
        .expect("checked just above");
    assert!(
        guard < launch,
        "the app is launched before the silence check, so a silent install opens a window"
    );
}

/// `/D=` sets `$INSTDIR`, which means the script must not overwrite it after NSIS has. The
/// failure this guards is an update that installs beside the installation it was meant to
/// replace, leaving two copies and a fleet that never updates again.
#[test]
fn the_install_directory_can_be_told_from_outside() {
    let script = script();
    assert!(
        script.contains("InstallDir "),
        "there is no default install directory"
    );
    // `StrCpy $INSTDIR` anywhere is the way this breaks: NSIS fills `$INSTDIR` from `/D=`
    // before `.onInit` runs, so anything assigning it afterwards silently wins.
    assert!(
        !script.contains("StrCpy $INSTDIR"),
        "something overwrites $INSTDIR, which `/D=` had already set"
    );
}

/// Per-user, never elevated. §4.9 item 3 refuses to install an update from an elevated
/// process, so an installer that demanded elevation would make that refusal permanent.
#[test]
fn the_install_needs_no_elevation() {
    let instructions = instructions();
    assert!(
        instructions.contains("RequestExecutionLevel user"),
        "the installer asks for elevation"
    );
    assert!(
        instructions.contains("$LOCALAPPDATA"),
        "the install directory is not one a user can write"
    );
    assert!(
        !instructions.contains("HKLM"),
        "something writes to the machine-wide registry, which needs elevation"
    );
}

/// The artefact keeps the name 1.x has always published under.
///
/// `electron-updater`'s `findFile` picks by extension and then prefers a filename
/// containing `x64` — 1.x published no token and one `.exe`, so any single `.exe` keeps
/// being picked. Changing the extension is the same act as abandoning the installed base.
#[test]
fn the_artefact_is_still_one_exe_under_the_old_name() {
    let script = script();
    assert!(
        script.contains("AnotherCrewLink-Setup-${VERSION}.exe"),
        "the installer's name changed, and `findFile` picks by name and extension"
    );
    assert!(
        script.contains("OutFile "),
        "the script produces no artefact at all"
    );
}

/// The installer's directory constant is `acl_core::paths::APP_DIRECTORY`.
///
/// They are two files that have to agree: the installer decides where the program goes and
/// the client decides where its settings go, and both are named from the same word. If one
/// moves, an installed client reads settings nothing wrote.
#[test]
fn the_directory_name_matches_the_clients() {
    let script = script();
    let paths = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acl-core/src/paths.rs"),
    )
    .expect("acl-core is beside this crate");

    let declared = paths
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("pub const APP_DIRECTORY: &str = \"")?
                .strip_suffix("\";")
                .map(ToOwned::to_owned)
        })
        .expect("acl-core still declares APP_DIRECTORY");

    assert!(
        script.contains(&format!("!define APP_DIRECTORY \"{declared}\"")),
        "the installer and the client disagree about the directory: the client says {declared}"
    );
}

/// The uninstaller leaves the settings alone.
///
/// Two reasons, and the second is the one that bites: a reinstall keeps a player's
/// settings, and an *update* that ran the uninstaller first would otherwise throw away the
/// settings it was meant to carry forward.
#[test]
fn uninstalling_does_not_delete_the_settings() {
    let script = instructions();
    let uninstall = script
        .split("Section \"Uninstall\"")
        .nth(1)
        .expect("there is an uninstall section");
    assert!(
        !uninstall.contains("$APPDATA"),
        "the uninstaller reaches into %APPDATA%, where the settings are"
    );
    assert!(
        !uninstall.contains("RMDir /r \"$LOCALAPPDATA"),
        "the uninstaller removes more than it installed"
    );
}

/// The client is closed before its files are replaced.
///
/// It holds a single-instance lock, a named pipe and an elevated child. Writing over a
/// running one is how an update leaves a half-written directory and a helper with nobody to
/// talk to.
#[test]
fn the_running_client_is_closed_first() {
    let script = instructions();
    let install = script
        .split("Section \"Install\"")
        .nth(1)
        .and_then(|section| section.split("SectionEnd").next())
        .expect("there is an install section");
    let closed = install
        .find("taskkill /IM anothercrewlink.exe")
        .expect("the client is closed");
    let helper = install
        .find("taskkill /IM acl-helper.exe")
        .expect("the helper is closed too");
    let first_file = install.find("File ").expect("something is installed");
    assert!(
        closed < first_file && helper < first_file,
        "files are written before the running client is closed"
    );
}
