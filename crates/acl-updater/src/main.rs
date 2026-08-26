//! The updater, as a program.
//!
//! §4.9 item 3 puts this in "a separate `acl-updater` binary", and separate is the point:
//! it replaces the client's files, so it must not be one of them. A client updating itself
//! in place is a client holding open the handles the installer needs to write.
//!
//! It does five things in an order that is the whole design, and stops at the first that
//! says no:
//!
//! 1. fetch the manifest and its detached signature,
//! 2. verify the signature against the keys this build trusts,
//! 3. ask the policy whether to install what it offers,
//! 4. fetch the artefact and check it against the manifest's digest,
//! 5. write it somewhere and run it.
//!
//! Nothing reaches a disk before step 4 has passed, and the artefact is not *fetched*
//! before step 3 — a client that downloaded eighty megabytes and then discovered it was a
//! downgrade would have spent somebody's data allowance proving a point.
//!
//! # It does nothing today, and says so
//!
//! `manifest::PUBLIC_KEYS` is empty because no release key exists yet, so step 2 refuses
//! everything. That is not a stub: it is the shape a fail-closed updater has before its
//! ceremony, and it will keep refusing until somebody puts a real key in.

use std::path::PathBuf;

use acl_updater::{fetch, install, policy};

/// What the updater was asked to do.
struct Arguments {
    /// Where the manifest is.
    feed: String,
    /// The version running now, which the policy compares against.
    running: semver::Version,
    /// Where the client is installed, which the installer is told with `/D=`.
    install_directory: PathBuf,
    /// Whether the user asked for this version whatever it is — the rollback bypass.
    asked_for_this_version: bool,
}

fn main() -> std::process::ExitCode {
    let Some(arguments) = read_arguments() else {
        eprintln!(
            "usage: acl-updater --feed <url> --running <version> --install-dir <path> \
             [--allow-downgrade]"
        );
        return std::process::ExitCode::from(2);
    };

    match update(&arguments) {
        Ok(message) => {
            println!("{message}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The five steps, in order.
fn update(arguments: &Arguments) -> Result<String, String> {
    let network = fetch::Http;
    let feed = fetch::Feed {
        manifest: &arguments.feed,
    };

    let manifest = fetch::manifest(feed, &network).map_err(|error| error.to_string())?;

    let decision = policy::decide(
        &arguments.running,
        &manifest,
        policy::Circumstances {
            elevated: install::elevated(),
            asked_for_this_version: arguments.asked_for_this_version,
        },
    );
    match decision {
        policy::Decision::AlreadyCurrent => {
            return Ok(format!(
                "{} is the version already running",
                manifest.version
            ));
        }
        policy::Decision::Downgrade { running, offered } => {
            return Err(format!(
                "refusing to go from {running} back to {offered}; pass --allow-downgrade if \
                 that is what you meant"
            ));
        }
        policy::Decision::Elevated => {
            return Err(
                "refusing to install an update from an elevated process; start the client \
                 normally and try again"
                    .to_owned(),
            );
        }
        policy::Decision::Install => {}
    }

    let artefact = fetch::artefact(&manifest, &network).map_err(|error| error.to_string())?;

    // Into the temporary directory rather than beside the installation: the installation is
    // what is about to be overwritten, and a downloaded executable sitting in it is one
    // more file the installer has to reason about.
    let into = std::env::temp_dir().join(format!("AnotherCrewLink-Setup-{}.exe", manifest.version));
    let plan = install::run(&artefact, &into, &arguments.install_directory)
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "installing {} with {}",
        manifest.version,
        plan.arguments.join(" ")
    ))
}

/// Reads the command line.
///
/// Hand-parsed rather than through a crate. Four options, one of them a flag, in a binary
/// nobody types by hand — a parser dependency here would be more code to audit than the
/// thing it parses.
fn read_arguments() -> Option<Arguments> {
    let mut feed = None;
    let mut running = None;
    let mut install_directory = None;
    let mut asked_for_this_version = false;

    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--feed" => feed = arguments.next(),
            "--running" => running = arguments.next(),
            "--install-dir" => install_directory = arguments.next().map(PathBuf::from),
            "--allow-downgrade" => asked_for_this_version = true,
            _ => return None,
        }
    }

    Some(Arguments {
        feed: feed?,
        running: semver::Version::parse(&running?).ok()?,
        install_directory: install_directory?,
        asked_for_this_version,
    })
}
