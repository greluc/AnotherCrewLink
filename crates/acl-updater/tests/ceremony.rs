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
