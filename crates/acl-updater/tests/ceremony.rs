#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! The ceremony's output, read by the client that will receive it.
//!
//! §4.9 item 3's blocker is that no release key exists, and the reason a ceremony is worth
//! automating is that a ceremony performed differently each time is performed wrongly once.
//! What this checks is the join between the two halves: that a manifest signed by
//! `acl-release` is one `acl_updater::manifest` accepts, against the public key the tool
//! told the maintainer to paste in.
//!
//! It runs the real binary rather than calling its functions, because the thing being
//! checked is the *ceremony* — the files it leaves on disk, under the names the fetcher
//! derives — and a test that called the functions would check neither.
//!
//! ```text
//! cargo test -p acl-updater --features ceremony --test ceremony
//! ```

#![cfg(feature = "ceremony")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The binary this workspace just built.
fn tool() -> PathBuf {
    // `CARGO_BIN_EXE_<name>` is set by cargo for every binary in the crate under test, so
    // this is the one that was compiled from the source beside it rather than whatever is
    // on the path.
    PathBuf::from(env!("CARGO_BIN_EXE_acl-release"))
}

/// The passphrase these tests use.
///
/// A constant, not a secret: the keys generated here live in a scratch directory and are
/// thrown away. The real one is the maintainer's and never appears in this repository.
const PASSPHRASE: &str = "a passphrase for a key that lasts one test";

fn run(arguments: &[&str]) -> String {
    let output = Command::new(tool())
        .args(arguments)
        // Set for every invocation, because `keys` requires it and `sign` needs it to open
        // what `keys` wrote. Passed as an environment variable here for the same reason the
        // tool insists on it: an argument would be in the process list.
        .env("ACL_RELEASE_KEY_PASSWORD", PASSPHRASE)
        .output()
        .expect("the ceremony tool runs");
    assert!(
        output.status.success(),
        "{arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join("acl-ceremony-test").join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

/// The whole ceremony, and then the client's own verifier reading what it produced.
#[test]
fn what_the_ceremony_signs_is_what_the_client_accepts() {
    let directory = scratch("whole");
    let artefact = directory.join("AnotherCrewLink-Setup-2.0.0.exe");
    std::fs::write(&artefact, b"pretend this is an installer").expect("an artefact");
    let manifest = directory.join("release.json");

    let announced = run(&["keys", "--into", &directory.to_string_lossy()]);
    // The tool tells the maintainer exactly what to paste. If that stops being a line they
    // can paste, the ceremony has a step that needs interpreting.
    let key = announced
        .lines()
        .find_map(|line| line.trim().strip_prefix('"')?.strip_suffix("\","))
        .expect("the public key is printed ready to paste");

    run(&[
        "write",
        "--version",
        "2.0.0",
        "--url",
        "https://example.invalid/AnotherCrewLink-Setup-2.0.0.exe",
        "--artefact",
        &artefact.to_string_lossy(),
        "--into",
        &manifest.to_string_lossy(),
    ]);
    run(&[
        "sign",
        "--manifest",
        &manifest.to_string_lossy(),
        "--key",
        &directory.join("release.key").to_string_lossy(),
        "--public",
        &directory.join("release.pub").to_string_lossy(),
    ]);

    // The signature is where the fetcher will look for it: `Feed::signature` derives the
    // name rather than accepting one, so these two have to agree by construction.
    let signature_path = Path::new(&format!("{}.minisig", manifest.display())).to_path_buf();
    assert!(
        signature_path.exists(),
        "the signature is not beside the manifest"
    );

    let document = std::fs::read(&manifest).expect("the manifest");
    let signature = std::fs::read_to_string(&signature_path).expect("the signature");
    let read = acl_updater::manifest::Manifest::verified_with(&document, &signature, &[key])
        .expect("the client refuses what the ceremony produced");

    assert_eq!(read.version, semver::Version::new(2, 0, 0));
    assert_eq!(read.size, 28);
    assert!(
        read.matches(&std::fs::read(&artefact).expect("the artefact")),
        "the manifest does not describe the file it was written from"
    );
}

/// The digest is read off the artefact, not taken on trust.
///
/// A manifest whose digest was typed is a manifest that can describe a file nobody has --
/// and it would be discovered by the fleet, at the moment they were meant to be updating.
#[test]
fn the_manifest_describes_the_file_and_not_the_arguments() {
    let directory = scratch("digest");
    let artefact = directory.join("Setup.exe");
    std::fs::write(&artefact, b"the real bytes").expect("an artefact");
    let manifest = directory.join("release.json");

    run(&[
        "write",
        "--version",
        "2.0.1",
        "--url",
        "https://example.invalid/Setup.exe",
        "--artefact",
        &artefact.to_string_lossy(),
        "--into",
        &manifest.to_string_lossy(),
    ]);

    let document = std::fs::read_to_string(&manifest).expect("the manifest");
    assert!(document.contains("\"size\":14"), "{document}");
    // And it changes when the file does, which is the property that makes it worth reading
    // rather than declaring.
    std::fs::write(&artefact, b"different bytes entirely").expect("a second artefact");
    let second = directory.join("second.json");
    run(&[
        "write",
        "--version",
        "2.0.1",
        "--url",
        "https://example.invalid/Setup.exe",
        "--artefact",
        &artefact.to_string_lossy(),
        "--into",
        &second.to_string_lossy(),
    ]);
    assert_ne!(
        document,
        std::fs::read_to_string(&second).expect("the second manifest"),
        "two different files produced the same manifest"
    );
}

/// A key is never overwritten.
///
/// Silently replacing one would retire every client that trusts the old one, at the next
/// release, with no step in between where anybody could notice.
#[test]
fn the_ceremony_refuses_to_overwrite_a_key() {
    let directory = scratch("overwrite");
    run(&["keys", "--into", &directory.to_string_lossy()]);
    let first = std::fs::read_to_string(directory.join("release.pub")).expect("a key");

    let output = Command::new(tool())
        .args(["keys", "--into", &directory.to_string_lossy()])
        .env("ACL_RELEASE_KEY_PASSWORD", PASSPHRASE)
        .output()
        .expect("the tool runs");
    assert!(!output.status.success(), "it overwrote a key");
    assert_eq!(
        std::fs::read_to_string(directory.join("release.pub")).expect("still a key"),
        first,
        "the key changed"
    );
}

/// Without a passphrase there is no key at all.
///
/// Decided 2026-08-27: the signing key is encrypted. The failure this guards against is not
/// somebody forgetting the variable — that is loud and self-correcting — but a tool that
/// helpfully fell back to generating an unencrypted key, which looks the same from the
/// outside and leaves the maintainer believing in a factor they do not have.
#[test]
fn no_passphrase_means_no_key() {
    let directory = scratch("no-passphrase");
    let output = Command::new(tool())
        .args(["keys", "--into", &directory.to_string_lossy()])
        .env_remove("ACL_RELEASE_KEY_PASSWORD")
        .output()
        .expect("the tool runs");

    assert!(!output.status.success(), "it made a key with no passphrase");
    assert!(
        !directory.join("release.key").exists(),
        "it refused and wrote a key anyway"
    );
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("ACL_RELEASE_KEY_PASSWORD"),
        "the refusal does not say what is missing: {complaint}"
    );

    // And an empty one is not a passphrase, though it is a value.
    let output = Command::new(tool())
        .args(["keys", "--into", &directory.to_string_lossy()])
        .env("ACL_RELEASE_KEY_PASSWORD", "")
        .output()
        .expect("the tool runs");
    assert!(!output.status.success(), "an empty passphrase was accepted");
    assert!(
        !directory.join("release.key").exists(),
        "an empty passphrase still wrote a key"
    );
}

/// The key on disk really is encrypted, and the passphrase really is the one that opens it.
///
/// Asserted against the file rather than against the tool's own report, because "I wrote an
/// encrypted key" is exactly the sentence a tool that wrote an unencrypted one would also
/// print. The wrong passphrase must fail; without that half, the first assertion would pass
/// for a key that any passphrase opens.
#[test]
fn the_key_on_disk_needs_the_passphrase() {
    let directory = scratch("encrypted");
    run(&["keys", "--into", &directory.to_string_lossy()]);

    let written = std::fs::read_to_string(directory.join("release.key")).expect("a key");
    let boxed = minisign::SecretKeyBox::from_string(&written).expect("a secret key box");
    assert!(
        boxed.into_unencrypted_secret_key().is_err(),
        "the key opened with no passphrase, so it is not encrypted"
    );

    let boxed = minisign::SecretKeyBox::from_string(&written).expect("a secret key box");
    assert!(
        boxed
            .into_secret_key(Some("not the passphrase".to_owned()))
            .is_err(),
        "the wrong passphrase opened it"
    );

    let boxed = minisign::SecretKeyBox::from_string(&written).expect("a secret key box");
    assert!(
        boxed.into_secret_key(Some(PASSPHRASE.to_owned())).is_ok(),
        "the right passphrase did not open it"
    );
}

/// The 1.x feed, written from the artefact rather than from arguments.
///
/// `latest.yml` is the file that moves the fleet: every 1.x install polls for it and runs
/// whatever version it names. `acl_updater::legacy_feed` had produced this document since it
/// was written and had no caller until 2026-08-27 — a generator nothing calls is a generator
/// whose output nobody has seen.
#[test]
fn the_feed_describes_the_artefact_it_was_given() {
    let directory = scratch("feed");
    let artefact = directory.join("AnotherCrewLink-Setup-1.0.6.exe");
    std::fs::write(&artefact, b"pretend this is an installer").expect("an artefact");
    let feed = directory.join("latest.yml");

    run(&[
        "feed",
        "--version",
        "1.0.6",
        "--artefact",
        &artefact.to_string_lossy(),
        "--released",
        "2026-08-27T06:00:00.000Z",
        "--into",
        &feed.to_string_lossy(),
    ]);

    let document = std::fs::read_to_string(&feed).expect("the feed");
    assert!(document.contains("version: 1.0.6"), "{document}");
    assert!(document.contains("size: 28"), "{document}");
    // The name comes off the path, so a feed cannot name a file that was never built.
    assert!(
        document.contains("path: AnotherCrewLink-Setup-1.0.6.exe"),
        "{document}"
    );
    // Base64, which is what electron-updater reads. Hex is what the 2.x manifest uses, and
    // a digest in the wrong encoding is a fleet that refuses every download.
    assert!(
        !document.contains("sha512: 0")
            && document
                .lines()
                .any(|line| line.starts_with("sha512: ") && line.contains('=')),
        "the digest does not look like base64: {document}"
    );

    // And it changes when the file does.
    std::fs::write(&artefact, b"a different installer entirely").expect("a second artefact");
    let second = directory.join("second.yml");
    run(&[
        "feed",
        "--version",
        "1.0.6",
        "--artefact",
        &artefact.to_string_lossy(),
        "--released",
        "2026-08-27T06:00:00.000Z",
        "--into",
        &second.to_string_lossy(),
    ]);
    assert_ne!(
        document,
        std::fs::read_to_string(&second).expect("the second feed"),
        "two different artefacts produced the same feed"
    );
}

/// It refuses to announce a 2.x version to the 1.x fleet.
///
/// Every 1.x client takes what it considers newer than what it is running, so a `latest.yml`
/// saying `2.0.0` migrates the entire installed base the moment it is published. That is
/// §4.12's act, with §4.12's blast radius and its staged rollout — not something that should
/// be reachable by mistyping a version at a release.
#[test]
fn the_feed_will_not_move_the_fleet_to_two_x() {
    let directory = scratch("feed-2x");
    let artefact = directory.join("AnotherCrewLink-Setup-2.0.0.exe");
    std::fs::write(&artefact, b"the 2.x installer").expect("an artefact");

    let output = Command::new(tool())
        .args([
            "feed",
            "--version",
            "2.0.0",
            "--artefact",
            &artefact.to_string_lossy(),
            "--released",
            "2026-08-27T06:00:00.000Z",
            "--into",
            &directory.join("latest.yml").to_string_lossy(),
        ])
        .env("ACL_RELEASE_KEY_PASSWORD", PASSPHRASE)
        .output()
        .expect("the tool runs");

    assert!(
        !output.status.success(),
        "it wrote a 1.x feed announcing 2.0.0, which moves every installed client"
    );
    assert!(
        !directory.join("latest.yml").exists(),
        "it refused and wrote the feed anyway"
    );
}

/// The key script still refuses to write into the repository.
///
/// A guard nobody can see failing is a guard that gets deleted in a tidy-up. This one is
/// the difference between a private key and a published one: a key in the working tree is
/// one `git add -A` from a push, and a pushed key must be replaced — there is no taking it
/// back out of a clone somebody already made.
///
/// Text, because the behaviour needs PowerShell and a filesystem, and the five refusal
/// paths were exercised by hand when the script was written. What this catches is the
/// deletion, which is the realistic failure.
#[test]
fn the_key_script_still_refuses_the_working_tree() {
    let script = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/new-release-key.ps1"),
    )
    .expect("scripts/new-release-key.ps1");

    assert!(
        script.contains("Test-Inside $target $repository"),
        "the script no longer checks whether the key would land inside the repository"
    );
    assert!(
        script.contains("Test-Inside $passphrasePath $target"),
        "the script no longer stops the passphrase being written beside the key"
    );
    // Through the environment, never as an argument: an argument is in the process list
    // for as long as the process runs.
    assert!(
        script.contains("$env:ACL_RELEASE_KEY_PASSWORD = $passphrase"),
        "the passphrase no longer reaches the tool through the environment"
    );
    assert!(
        !script.contains("--password") && !script.contains("-Passphrase "),
        "the passphrase is being passed as an argument, which puts it in the process list"
    );
    // The alphabet has to divide 256 or the mapping is biased, and the script asserts its
    // own size -- this checks that assertion is still there to fail.
    assert!(
        script.contains("-ne 32) { throw"),
        "nothing checks the passphrase alphabet is still 32 symbols"
    );
}

/// `check` consults the compiled-in keys, and a key nobody put there is refused.
///
/// The positive direction cannot be tested here: `PUBLIC_KEYS` holds the real release keys,
/// whose private halves are the maintainer's and are not in this repository — which is the
/// property the whole design rests on. What can be tested is the direction that would make
/// the command worthless, which is a `check` that says yes to anything.
///
/// The command exists for the recovery key. A wrong entry for the operational one is found
/// at the next release; a wrong entry for the recovery one is found on the day the
/// operational key is gone, when there is no second chance and no way to send a fix.
#[test]
fn a_key_the_build_does_not_know_is_refused() {
    let directory = scratch("stranger");
    run(&["keys", "--into", &directory.to_string_lossy()]);

    let output = Command::new(tool())
        .args([
            "check",
            "--key",
            &directory.join("release.key").to_string_lossy(),
        ])
        .env("ACL_RELEASE_KEY_PASSWORD", PASSPHRASE)
        .output()
        .expect("the tool runs");

    assert!(
        !output.status.success(),
        "a key generated seconds ago was reported as one the shipped client trusts"
    );
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("PUBLIC_KEYS"),
        "the refusal does not say where the key would have to be: {complaint}"
    );
}
