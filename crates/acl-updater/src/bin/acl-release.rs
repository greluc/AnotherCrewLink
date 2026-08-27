//! The release ceremony, as a command.
//!
//! §4.9 item 3 leaves `manifest::PUBLIC_KEYS` empty because "generating one is a ceremony
//! the maintainer performs offline". A ceremony that has to be invented each time is one
//! that gets done differently each time — and one of those times will be the time the
//! private key ends up in a workflow secret, which is the exact thing item 3 says it must
//! never be.
//!
//! So the ceremony is this program. Three subcommands, and none of them can be run by
//! accident:
//!
//! ```text
//! ACL_RELEASE_KEY_PASSWORD=… acl-release keys --into <dir>  # once, offline, on a machine
//!                                                           # you trust
//! acl-release write  --version <v> --url <u> --artefact <path> --into <path>
//! ACL_RELEASE_KEY_PASSWORD=… acl-release sign --manifest <path> --key <path> [--public <p>]
//! ```
//!
//! The passphrase comes from the environment in both places, never from an argument: on a
//! command line it is in the shell's history and in every process listing while it runs.
//!
//! # It is not in the client
//!
//! A separate binary behind a non-default feature, so nothing that signs is ever compiled
//! into the thing users run. The client verifies; it has no business being able to sign,
//! and a build that could is a build where a key file in the wrong directory becomes a
//! release nobody made.
//!
//! # What the maintainer still has to do, and nothing can do for them
//!
//! Keep the private keys somewhere the release workflow cannot reach. The operational one
//! in particular: §4.9 says it is "held offline and never in a release-workflow secret",
//! and that is a property of where it is stored, not of any code here.

use std::path::{Path, PathBuf};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.first().map(String::as_str) {
        Some("keys") => keys(&arguments),
        Some("write") => write(&arguments),
        Some("sign") => sign(&arguments),
        Some("feed") => feed(&arguments),
        Some("check") => check(&arguments),
        _ => Err(usage()),
    };
    match outcome {
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

fn usage() -> String {
    "usage:\n  \
     acl-release keys  --into <directory>          (ACL_RELEASE_KEY_PASSWORD required)\n  \
     acl-release write --version <v> --url <u> --artefact <path> --into <path>\n  \
     acl-release sign  --manifest <path> --key <path> [--public <path>]\n  \
     acl-release feed  --version <v> --artefact <path> --released <date> --into <path>\n  \
     acl-release check --key <path>                (ACL_RELEASE_KEY_PASSWORD if encrypted)\n\
     \n\
     The passphrase is read from ACL_RELEASE_KEY_PASSWORD, never from an argument."
        .to_owned()
}

/// The named option, if it is there.
fn option(arguments: &[String], name: &str) -> Option<String> {
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at + 1).cloned()
}

/// Generates a keypair, encrypted with a passphrase.
///
/// **Decided 2026-08-27.** §4.9's requirement is where the key lives — "held offline and
/// never in a release-workflow secret" — and that is still the thing that matters most.
/// The passphrase is the second factor on top of it: whoever copies the file has not yet
/// got a signing key. It is only worth having if the passphrase lives somewhere the key
/// file does not, which is a property of the maintainer's habits and not of this tool.
///
/// There is no unencrypted mode here, deliberately. A flag for it is a flag somebody
/// reaches for on the day the passphrase is inconvenient, and the resulting file looks
/// exactly like the encrypted one from the outside. `sign` still *loads* an unencrypted
/// key, because refusing to would be this tool deciding what an existing key may be.
///
/// Refuses to overwrite. A ceremony that silently replaced a key would be one that could
/// quietly retire every client that trusts the old one.
fn keys(arguments: &[String]) -> Result<String, String> {
    let into = PathBuf::from(option(arguments, "--into").ok_or_else(usage)?);
    let public = into.join("release.pub");
    let secret = into.join("release.key");
    for path in [&public, &secret] {
        if path.exists() {
            return Err(format!(
                "{} already exists; move it aside deliberately rather than overwriting a key",
                path.display()
            ));
        }
    }

    // From the environment, for the reason `sign` takes it that way: a passphrase on a
    // command line is in the shell's history and in every process listing while it runs.
    // Refused rather than defaulted -- a key that silently came out unencrypted is worse
    // than no key, because it is a key somebody believes is protected.
    let password = std::env::var("ACL_RELEASE_KEY_PASSWORD").map_err(|_| {
        "set ACL_RELEASE_KEY_PASSWORD to the passphrase for the new key.\n\
         It is read from the environment and not from an argument, so it stays out of the \
         shell history and out of the process list. Keep it somewhere the key file is not: \
         a passphrase stored beside the key it protects is not a second factor."
            .to_owned()
    })?;
    if password.is_empty() {
        return Err("ACL_RELEASE_KEY_PASSWORD is empty, which encrypts nothing".to_owned());
    }
    std::fs::create_dir_all(&into).map_err(|error| error.to_string())?;

    let pair = minisign::KeyPair::generate_encrypted_keypair(Some(password.clone()))
        .map_err(|error| format!("no keypair: {error}"))?;
    std::fs::write(
        &public,
        pair.pk.to_box().map_err(|e| e.to_string())?.to_string(),
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        &secret,
        pair.sk.to_box(None).map_err(|e| e.to_string())?.to_string(),
    )
    .map_err(|error| error.to_string())?;

    // And open it again with the passphrase. The alternative is finding out at the first
    // release that the file cannot be decrypted -- by which point the ceremony has been
    // done, the machine may be gone, and there is nothing to compare against.
    let written = std::fs::read_to_string(&secret).map_err(|error| error.to_string())?;
    minisign::SecretKeyBox::from_string(&written)
        .map_err(|error| error.to_string())
        .and_then(|boxed| {
            boxed
                .into_secret_key(Some(password))
                .map_err(|error| error.to_string())
        })
        .map_err(|error| {
            format!("the key was written but will not open with that passphrase: {error}")
        })?;

    Ok(format!(
        "wrote {} and {}, and opened the secret half again with the passphrase\n\n\
         Put this in `manifest::PUBLIC_KEYS`:\n\n    \"{}\",\n\n\
         Then keep {} somewhere the release workflow cannot reach, and the passphrase \
         somewhere that is not beside it. §4.9: the operational key is \"held offline and \
         never in a release-workflow secret\" -- the passphrase is on top of that, not \
         instead of it.",
        public.display(),
        secret.display(),
        pair.pk.to_base64(),
        secret.display(),
    ))
}

/// Writes a manifest for an artefact that exists.
///
/// The digest and the size are read off the file rather than taken as arguments, because a
/// manifest whose digest was typed is a manifest that can describe a file nobody has.
fn write(arguments: &[String]) -> Result<String, String> {
    let version = option(arguments, "--version").ok_or_else(usage)?;
    let url = option(arguments, "--url").ok_or_else(usage)?;
    let artefact = PathBuf::from(option(arguments, "--artefact").ok_or_else(usage)?);
    let into = PathBuf::from(option(arguments, "--into").ok_or_else(usage)?);

    semver::Version::parse(&version).map_err(|error| format!("--version: {error}"))?;
    let bytes =
        std::fs::read(&artefact).map_err(|error| format!("{}: {error}", artefact.display()))?;
    if bytes.is_empty() {
        return Err(format!("{} is empty", artefact.display()));
    }

    let digest = {
        use sha2::Digest as _;
        use std::fmt::Write as _;

        sha2::Sha512::digest(&bytes)
            .iter()
            .fold(String::with_capacity(128), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            })
    };
    let document = format!(
        "{{\"version\":\"{version}\",\"url\":\"{url}\",\"sha512\":\"{digest}\",\"size\":{}}}",
        bytes.len()
    );
    std::fs::write(&into, &document).map_err(|error| error.to_string())?;
    Ok(format!(
        "wrote {} for {} ({} bytes)\n\nNow: acl-release sign --manifest {} --key <your key>",
        into.display(),
        artefact.display(),
        bytes.len(),
        into.display(),
    ))
}

/// Signs a manifest, and checks its own work.
///
/// The verification afterwards is not ceremony: it is the difference between a release that
/// is signed and one that has a file beside it. A signature nobody checked is discovered by
/// the fleet, at the moment they were meant to be updating.
fn sign(arguments: &[String]) -> Result<String, String> {
    let manifest = PathBuf::from(option(arguments, "--manifest").ok_or_else(usage)?);
    let key = PathBuf::from(option(arguments, "--key").ok_or_else(usage)?);
    // The public half, given rather than derived. Deriving it would check the signature
    // against the key that made it, which cannot fail and therefore checks nothing; given,
    // it checks the signature against the key the *fleet* will use -- which is the question
    // worth asking, and the one whose wrong answer bricks an update for everybody.
    let public = PathBuf::from(
        option(arguments, "--public").unwrap_or_else(|| format!("{}.pub", key.display())),
    );

    let document =
        std::fs::read(&manifest).map_err(|error| format!("{}: {error}", manifest.display()))?;
    let secret = secret_key(&key)?;

    let signature = minisign::sign(None, &secret, std::io::Cursor::new(&document), None, None)
        .map_err(|error| format!("no signature: {error}"))?
        .to_string();
    let beside = signature_path(&manifest);
    std::fs::write(&beside, &signature).map_err(|error| error.to_string())?;

    // And read it back. A signature nobody checked is discovered by the fleet, at the
    // moment they were meant to be updating.
    let public_text = std::fs::read_to_string(&public)
        .map_err(|error| format!("{}: {error}", public.display()))?;
    let public_key = minisign::PublicKeyBox::from_string(&public_text)
        .and_then(minisign::PublicKeyBox::into_public_key)
        .map_err(|error| format!("{}: {error}", public.display()))?;
    acl_updater::manifest::Manifest::verified_with(
        &document,
        &signature,
        &[&public_key.to_base64()],
    )
    .map_err(|error| format!("the signature does not verify: {error}"))?;

    Ok(format!(
        "wrote {} and verified it\n\nPublish both, side by side.",
        beside.display()
    ))
}

/// Where a manifest's signature goes.
///
/// Beside it, with `.minisig` appended — minisign's own convention, and the one
/// `acl_updater::fetch::Feed::signature` derives rather than accepts, so the two cannot be
/// told to disagree.
fn signature_path(manifest: &Path) -> PathBuf {
    let mut name = manifest.as_os_str().to_owned();
    name.push(".minisig");
    PathBuf::from(name)
}

/// Writes `latest.yml`, the document the **1.x** fleet's updater follows.
///
/// Not the same file as `write` produces, and the difference matters. `write` makes the
/// minisign manifest 2.x verifies; this makes the electron-updater feed 1.x polls. A release
/// that goes out through `legacy.nsi` needs this one, because electron-builder is no longer
/// the thing packaging it -- and electron-builder was what used to write this file.
///
/// `acl_updater::legacy_feed` has done the hard part since it was written and had no caller
/// until 2026-08-27. A generator nothing calls is a generator whose output nobody has seen.
///
/// # This is the file that moves the fleet
///
/// Publishing it is the act. Every 1.x install polls for it, and whatever version it names
/// is what they download and run. So the digest and the size are read off the artefact here
/// rather than accepted as arguments: a feed whose digest was typed is one that either
/// describes a file nobody has, or -- worse -- passes.
fn feed(arguments: &[String]) -> Result<String, String> {
    let version = option(arguments, "--version").ok_or_else(usage)?;
    let artefact = PathBuf::from(option(arguments, "--artefact").ok_or_else(usage)?);
    // Passed in, never read from the clock. This runs in a release job whose output should
    // be a function of its inputs, and a timestamp taken here differs between two builds of
    // the same commit.
    let released = option(arguments, "--released").ok_or_else(usage)?;
    let into = PathBuf::from(option(arguments, "--into").ok_or_else(usage)?);

    let parsed = semver::Version::parse(&version).map_err(|error| format!("--version: {error}"))?;
    // A 1.x updater takes what it considers newer than what it is running. A feed announcing
    // 2.0.0 would be taken by every 1.x client -- which is the fleet migration, and that is
    // §4.12's act with §4.12's blast radius, not something to do by mistyping a version.
    if parsed.major != 1 {
        return Err(format!(
            "{version} is not a 1.x version.\n\
             This file is what the 1.x fleet follows: publishing a 2.x version here moves \
             every installed client at once. If that is the intention, it is §4.12's bridge \
             and it announces itself as 1.1.0."
        ));
    }

    let bytes =
        std::fs::read(&artefact).map_err(|error| format!("{}: {error}", artefact.display()))?;
    if bytes.is_empty() {
        return Err(format!("{} is empty", artefact.display()));
    }
    let file_name = artefact
        .file_name()
        .ok_or_else(|| format!("{} has no file name", artefact.display()))?
        .to_string_lossy()
        .into_owned();

    let digest = {
        use sha2::Digest as _;

        sha2::Sha512::digest(&bytes).to_vec()
    };
    let document = acl_updater::legacy_feed::LegacyRelease {
        version,
        file_name: file_name.clone(),
        sha512: digest,
        size: bytes.len() as u64,
        released,
    }
    .to_yaml();
    std::fs::write(&into, &document).map_err(|error| error.to_string())?;

    Ok(format!(
        "wrote {} for {} ({} bytes)\n\n\
         Publish it beside the installer, in the same release. Every 1.x client polls for \
         this file and runs what it names -- so it goes up last, after the artefact it \
         describes is already downloadable.",
        into.display(),
        file_name,
        bytes.len(),
    ))
}

/// Loads a secret key, encrypted or not.
///
/// The passphrase comes from the environment rather than from an argument: one on a command
/// line is in the shell's history and in every process listing while the process runs.
fn secret_key(path: &Path) -> Result<minisign::SecretKey, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let boxed = minisign::SecretKeyBox::from_string(&text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    match std::env::var("ACL_RELEASE_KEY_PASSWORD") {
        Ok(password) => boxed.into_secret_key(Some(password)),
        Err(_) => boxed.into_unencrypted_secret_key(),
    }
    .map_err(|error| {
        format!(
            "{}: {error}
(an encrypted key needs ACL_RELEASE_KEY_PASSWORD set)",
            path.display()
        )
    })
}

/// Asks whether the shipped client would trust something this key signed.
///
/// # The question nothing else answers
///
/// `sign` checks a signature against a public key *file*. That proves the two halves on the
/// maintainer's disk agree. It says nothing about `manifest::PUBLIC_KEYS`, which is the list
/// compiled into what people are running — and a key transcribed into that list wrongly is a
/// build that refuses every update.
///
/// For the operational key the transcription can be checked by eye against `release.pub`.
/// For the **recovery** key it cannot usefully be: it is only ever used on the day the
/// operational key is gone, so a wrong entry sits there looking correct until the one moment
/// it has to work, and on that day there is no second chance and no way to send a fix.
///
/// So this signs a throwaway document and verifies it with the client's own verifier against
/// the compiled-in list. It answers "would the fleet accept this key", which is the question,
/// and it is worth asking again before any release rather than only once.
fn check(arguments: &[String]) -> Result<String, String> {
    let key = PathBuf::from(option(arguments, "--key").ok_or_else(usage)?);
    let secret = secret_key(&key)?;

    // A manifest rather than arbitrary bytes, because the verifier parses before it trusts
    // and a document it rejects for its shape would look like a key it rejects.
    let document = concat!(
        r#"{"version":"0.0.0","url":"https://example.invalid/none","sha512":""#,
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce",
        "47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
        r#"","size":0}"#
    );
    let signature = minisign::sign(None, &secret, std::io::Cursor::new(document), None, None)
        .map_err(|error| format!("no signature: {error}"))?
        .to_string();

    acl_updater::manifest::Manifest::verified(document.as_bytes(), &signature).map_err(
        |error| {
            format!(
                "{}\n\n\
                 This key is NOT one the shipped client trusts. Either it is not in \
                 `manifest::PUBLIC_KEYS`, or what is in there is not this key. On the day \
                 that matters there is no way to correct it, because correcting it means \
                 shipping an update.",
                error
            )
        },
    )?;

    Ok(format!(
        "{} is trusted by this build.\n\n\
         A manifest signed by it was accepted by the client's own verifier, against the \
         {} key(s) compiled into `manifest::PUBLIC_KEYS`.",
        key.display(),
        acl_updater::manifest::PUBLIC_KEYS.len(),
    ))
}
