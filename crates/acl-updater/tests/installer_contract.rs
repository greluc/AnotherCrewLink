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
//! # What this checks, and what checks the rest
//!
//! This reads the source. It does not prove the installer works; it proves it still claims
//! to do the things that would strand the fleet if it stopped claiming them, and it fails
//! the moment somebody edits them out.
//!
//! Since 2026-08-26 the other half exists: `rust.yml`'s `installer` job compiles the script
//! with `makensis` on every push and then runs what it produced — a silent install with the
//! exact command line above, a check that the files landed, and a silent uninstall. That is
//! the part no amount of reading can do, because a script that compiles but opens a dialog
//! under `/S` hangs the updater forever and looks fine in the source.
//!
//! The two are not redundant. A compile says NSIS accepted it; these say it still says what
//! it must, which is a different question and the one that regresses quietly.

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
    named("anothercrewlink.nsi")
}

/// Every installer script there is.
///
/// Named here rather than globbed, so adding a fourth is a deliberate act that makes a
/// test fail until somebody has said which of these apply to it.
const SCRIPTS: [&str; 3] = ["anothercrewlink.nsi", "bridge.nsi", "legacy.nsi"];

/// One of the installer scripts, by name, with `common.nsh` resolved into it.
///
/// The shared half of the contract moved into an include on 2026-08-27, and a test that
/// read only the `.nsi` would have declared every one of these scripts broken. Resolving it
/// the way `makensis` does keeps the assertions asking about the script *as compiled*,
/// which is the thing that reaches the fleet -- and it means a script that stops including
/// the common file fails these tests rather than quietly losing the contract.
fn named(file: &str) -> String {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../installer");
    let path = directory.join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let Some(at) = text.find("!include \"common.nsh\"") else {
        return text;
    };
    let shared = std::fs::read_to_string(directory.join("common.nsh")).expect("common.nsh");
    // In place, because order matters to some of these checks -- `IfSilent` must come
    // before the `MessageBox` it guards, and that is only true once the include is where
    // the script put it.
    format!(
        "{}\n{shared}\n{}",
        &text[..at],
        &text[at + "!include \"common.nsh\"".len()..]
    )
}

/// One of them with its comments removed, for the checks that must not match prose.
fn instructions_of(file: &str) -> String {
    let mut kept = String::new();
    for line in named(file).lines() {
        kept.push_str(line.split(';').next().unwrap_or_default());
        kept.push('\n');
    }
    kept
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

/// A prerelease version still compiles.
///
/// `VIProductVersion` takes four numbers and aborts on anything else — `makensis` says
/// "invalid `VIProductVersion` format" and stops. The release workflow triggers on `v2.*`,
/// which matches `v2.0.0-rc.1`, and §4.12's staged rollout is the thing that would be
/// tagged that way. So the scripts take the numeric part themselves rather than trusting
/// the caller to have stripped it.
///
/// Found by the `installer` job in `rust.yml` on its second run, which is the argument for
/// having it: what NSIS will accept is not visible to a test that reads the source.
#[test]
fn a_prerelease_version_does_not_abort_the_build() {
    for file in SCRIPTS {
        let instructions = instructions_of(file);
        assert!(
            instructions.contains("!searchparse"),
            "{file} does not derive a numeric version, so a -rc tag will not build"
        );
        assert!(
            instructions.contains("VIProductVersion \"${VERSION_NUMERIC}"),
            "{file} still passes the unstripped version to VIProductVersion"
        );
    }
}

/// A 32-bit machine is refused, rather than quietly given something that cannot run.
///
/// This is not a hypothetical. The 32-bit build was removed on 2026-08-25, and §4.12's
/// bridge publishes into the 1.x feed, where `electron-updater`'s `findFile` prefers a name
/// containing `x64` or `ia32` and **otherwise takes the first `.exe`**. There is no 32-bit
/// artefact to publish, so there is no name to prefer, so a 32-bit 1.0.2 client is handed
/// this installer. NSIS installers are themselves 32-bit, so it would run to the end and
/// lay down x64 binaries that cannot start.
///
/// Refusing does not rescue those users; nothing in this repository can. It is the
/// difference between an install that reports success and leaves nothing working, and one
/// that says what happened.
#[test]
fn a_thirty_two_bit_machine_is_refused_rather_than_broken() {
    let instructions = instructions();
    assert!(
        instructions.contains("${RunningX64}"),
        "no architecture guard: a 32-bit client would install x64 binaries and succeed"
    );
    assert!(
        instructions.contains("Abort"),
        "the guard does not stop the install"
    );
    assert!(
        instructions.contains("SetErrorLevel"),
        "a silent install that aborts with exit code 0 tells the updater it worked"
    );
}

/// And it refuses without a dialog when it was started silently.
///
/// The same rule as `a_silent_install_starts_nothing`, in the one place it is easiest to
/// forget: an error path. The updater that spawned this waits on the process and never sees
/// a window, so a message box on the way out is a hang rather than an explanation -- and it
/// is a hang that only happens on the machines that were already going to have the worst
/// day, which is why nobody would find it.
#[test]
fn the_refusal_shows_no_dialog_when_silent() {
    let instructions = instructions();
    let guard = instructions
        .find("${RunningX64}")
        .expect("the architecture guard");
    let after = &instructions[guard..];
    let silent = after.find("IfSilent").expect("no IfSilent in the guard");
    // A `MessageBox` before the `IfSilent` that skips it is one every silent install runs.
    if let Some(dialog) = after.find("MessageBox") {
        assert!(
            silent < dialog,
            "the guard opens a dialog before checking whether it is allowed to"
        );
    }
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

/// The bridge installer, which is the one artefact a large number of machines execute
/// without anybody choosing to.
mod bridge {
    use super::instructions_of;

    fn bridge() -> String {
        instructions_of("bridge.nsi")
    }

    /// It renames rather than deletes. §4.12 item 4, and it is the whole difference between
    /// a migration somebody can walk back and one nobody can.
    #[test]
    fn the_electron_installation_is_renamed_and_not_deleted() {
        let bridge = bridge();
        assert!(
            bridge.contains("Rename \"$INSTDIR\\AnotherCrewLink.exe\""),
            "the Electron executable is not moved aside"
        );
        assert!(
            bridge.contains("Rename \"$INSTDIR\\resources\""),
            "the Electron resources are not moved aside"
        );
        let install = bridge
            .split("Section \"Install\"")
            .nth(1)
            .and_then(|section| section.split("SectionEnd").next())
            .expect("an install section");
        assert!(
            !install.contains("Delete "),
            "the bridge deletes something in its install section"
        );
        assert!(
            !install.contains("RMDir"),
            "the bridge removes a directory in its install section"
        );
    }

    /// It does not touch 1.x's settings. Since the two versions keep separate directories,
    /// `acl_core::paths::import` reads 1.x's `config.json` forward on first run -- and
    /// renaming it would break the import it exists to enable, leaving 2.x on defaults with
    /// the settings sitting under a name nothing looks for.
    #[test]
    fn the_old_settings_are_left_exactly_where_the_importer_looks() {
        let bridge = bridge();
        assert!(
            !bridge.contains("$APPDATA"),
            "the bridge reaches into %APPDATA%, where both versions keep their settings"
        );
    }

    /// It never opens a window. Every machine runs this because its updater decided to, not
    /// because somebody asked -- a window appearing would be a program the user did not open
    /// turning up while they were doing something else.
    #[test]
    fn the_bridge_opens_no_window_at_all() {
        let bridge = bridge();
        assert!(
            !bridge.contains("Exec '\"$INSTDIR"),
            "the bridge starts the client, on a machine nobody asked"
        );
    }

    /// The same three arguments as the plain installer, because the fleet's updater sends
    /// them and will not be persuaded otherwise.
    #[test]
    fn it_honours_the_same_contract_as_the_plain_installer() {
        let bridge = bridge();
        assert!(bridge.contains("--updated"), "no --updated");
        assert!(bridge.contains("${GetOptions}"), "--updated is not parsed");
        assert!(
            !bridge.contains("StrCpy $INSTDIR"),
            "something overwrites $INSTDIR, which /D= had already set"
        );
        assert!(
            bridge.contains("RequestExecutionLevel user"),
            "the bridge asks for elevation, on every machine at once"
        );
        assert!(
            !bridge.contains("HKLM"),
            "the bridge writes machine-wide state"
        );
    }

    /// It kills the Electron client by its own name as well as ours. On most of these
    /// machines the running process is `AnotherCrewLink.exe`, and it is the one whose
    /// updater started this installer.
    #[test]
    fn the_electron_client_is_closed_by_its_own_name() {
        let bridge = bridge();
        assert!(
            bridge.contains("taskkill /IM AnotherCrewLink.exe"),
            "the Electron client is left running while its files are moved"
        );
    }

    /// The uninstaller leaves the backup. Somebody uninstalling 2.x may be doing exactly
    /// that in order to go back to 1.x.
    /// The bridge refuses a 32-bit machine too, and refuses it before it breaks anything.
    ///
    /// This is the guard that will actually meet one. The plain installer is downloaded
    /// deliberately by somebody who chose it; the bridge is *pushed* to the installed fleet
    /// by 1.x's own updater, which hands the single `.exe` to every client because there is
    /// no 32-bit build for `findFile` to prefer.
    ///
    /// And the ordering matters here in a way it does not there: this script renames the
    /// 1.x installation before it writes. Aborting in `.onInit` is aborting before that, so
    /// a refused user still has a working 1.0.2 — rather than having traded it for binaries
    /// their machine cannot execute.
    #[test]
    fn a_thirty_two_bit_machine_keeps_the_client_it_has() {
        let instructions = instructions_of("bridge.nsi");
        assert!(
            instructions.contains("${RunningX64}"),
            "the bridge has no architecture guard, and it is the one the fleet runs"
        );
        let guard = instructions
            .find("${RunningX64}")
            .expect("the architecture guard");
        let rename = instructions
            .find("Rename")
            .expect("the bridge no longer renames the 1.x installation");
        assert!(
            guard < rename,
            "the guard runs after the 1.x installation has been renamed, so a refused              32-bit user loses the client that worked"
        );
    }

    #[test]
    fn uninstalling_leaves_the_way_back() {
        let bridge = bridge();
        let uninstall = bridge
            .split("Section \"Uninstall\"")
            .nth(1)
            .expect("an uninstall section");
        assert!(
            !uninstall.contains("1.x-backup"),
            "the uninstaller removes the way back to 1.x"
        );
    }
}

/// Every script honours the contract, whichever one the fleet ends up running.
///
/// The point of `common.nsh` is that there is one copy of this. The point of this test is
/// that a script can still fail to include it — and the failure would be a script that
/// compiles, installs, and ignores `/S`, which is discovered by whoever is waiting on the
/// process.
#[test]
fn all_three_scripts_keep_the_same_contract() {
    for file in SCRIPTS {
        let raw = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../installer")
                .join(file),
        )
        .expect("the script");
        assert!(
            raw.contains("!include \"common.nsh\""),
            "{file} does not include common.nsh, so it has its own copy of the contract"
        );

        let instructions = instructions_of(file);
        for required in [
            "--updated",
            "${GetOptions}",
            "RequestExecutionLevel user",
            "ACL_ARCHITECTURE_GUARD",
            "WriteUninstaller",
        ] {
            assert!(
                instructions.contains(required),
                "{file} is missing {required}"
            );
        }
        assert!(
            !instructions.contains("RequestExecutionLevel admin"),
            "{file} asks for elevation, which makes every update a UAC prompt"
        );
    }
}

/// The script that carries an ordinary 1.0.x release.
///
/// §4.9: "Prove the new NSIS script by shipping an **ordinary 1.0.x release** with it, so
/// its CLI contract is tested against real 1.x updaters before it carries anything
/// important." Decided 2026-08-27 that this is the path, and this is the artefact.
mod legacy {
    use super::instructions_of;

    fn legacy() -> String {
        instructions_of("legacy.nsi")
    }

    /// It installs where 1.x already is, not where 2.x goes.
    ///
    /// `the_directory_name_matches_the_clients` holds the other two to
    /// `acl_core::paths::APP_DIRECTORY`, which is `ACL`. This one must not match it: an
    /// ordinary 1.0.3 that installed into 2.x's directory would leave somebody with two 1.x
    /// installations, the old one still on the Start menu.
    #[test]
    fn it_lands_in_the_directory_one_x_already_uses() {
        let instructions = legacy();
        assert!(
            instructions.contains("!define APP_DIRECTORY \"AnotherCrewLink\""),
            "the 1.0.x installer no longer targets 1.x's own directory"
        );
        assert!(
            !instructions.contains("!define APP_DIRECTORY \"ACL\""),
            "it targets 2.x's directory, which would be a second installation"
        );
    }

    /// It carries the Electron client, and carries it as a tree.
    ///
    /// Naming the files would be a list that goes stale on the next Electron upgrade —
    /// silently, by omitting a DLL rather than by failing. And if it carried the Rust
    /// binaries it would not be an ordinary 1.0.x release; it would be the bridge, which is
    /// a different act with a different blast radius.
    #[test]
    fn it_carries_the_electron_client_and_not_the_rust_one() {
        let instructions = legacy();
        assert!(
            instructions.contains("File /r \"${SOURCE_DIR}\\*.*\""),
            "the payload is not the unpacked Electron tree"
        );
        assert!(
            instructions.contains("win-unpacked"),
            "SOURCE_DIR does not default to what `electron-builder --dir` produces"
        );
        assert!(
            !instructions.contains("anothercrewlink.exe"),
            "it ships the Rust client, which would make this the bridge and not a 1.0.x \
             release"
        );
    }

    /// The uninstaller takes away what the installer put down.
    ///
    /// A tree went in; a tree has to come out. `RMDir /r "$INSTDIR"` cannot be the answer —
    /// it would delete the uninstaller while it is running — so the pieces are named, and
    /// the two that hold everything of any size are the ones worth asserting.
    #[test]
    fn what_it_installs_it_can_remove() {
        let instructions = legacy();
        for required in [
            "RMDir /r \"$INSTDIR\\resources\"",
            "RMDir /r \"$INSTDIR\\locales\"",
        ] {
            assert!(
                instructions.contains(required),
                "the uninstaller leaves {required} behind"
            );
        }
        assert!(
            !instructions.contains("RMDir /r \"$INSTDIR\""),
            "it deletes the directory the running uninstaller is in"
        );
    }

    /// The settings survive it, because 2.x reads them forward.
    ///
    /// `%APPDATA%\AnotherCrewLink` is 1.x's config and also what `acl_core::paths::import`
    /// reads on 2.x's first run. An uninstaller that removed it would cost somebody their
    /// settings on a migration they had not made yet.
    #[test]
    fn it_leaves_the_settings_for_the_importer() {
        let instructions = legacy();
        assert!(
            !instructions.contains("$APPDATA"),
            "the 1.0.x installer touches %APPDATA%, where both the old settings and the \
             importer's source live"
        );
    }
}
