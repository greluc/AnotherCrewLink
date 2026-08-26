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
//! acl-release keys   --into <directory>            # once, offline, on a machine you trust
//! acl-release write  --version <v> --url <u> --artefact <path> --into <path>
//! acl-release sign   --manifest <path> --key <path> [--public <path>]
//! ```
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
     acl-release keys  --into <directory>\n  \
     acl-release write --version <v> --url <u> --artefact <path> --into <path>\n  \
     acl-release sign  --manifest <path> --key <path> [--public <path>]"
        .to_owned()
}

/// The named option, if it is there.
fn option(arguments: &[String], name: &str) -> Option<String> {
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at + 1).cloned()
}

/// Generates a keypair.
///
/// Unencrypted, because what protects it is where it is kept -- which is what §4.9 asks
/// for: "held offline and never in a release-workflow secret". A passphrase typed at every
/// signing is a passphrase that ends up in a note beside the key.
///
/// That is a default rather than a rule: `sign` loads an encrypted key too, given
/// `ACL_RELEASE_KEY_PASSWORD`. A maintainer who wants one is not blocked by this tool.
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
    std::fs::create_dir_all(&into).map_err(|error| error.to_string())?;

    let pair = minisign::KeyPair::generate_unencrypted_keypair()
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

    Ok(format!(
        "wrote {} and {}\n\n\
         Put this in `manifest::PUBLIC_KEYS`:\n\n    \"{}\",\n\n\
         Then keep {} somewhere the release workflow cannot reach. §4.9: the operational \
         key is \"held offline and never in a release-workflow secret\".",
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
    let secret_text =
        std::fs::read_to_string(&key).map_err(|error| format!("{}: {error}", key.display()))?;
    let boxed = minisign::SecretKeyBox::from_string(&secret_text)
        .map_err(|error| format!("{}: {error}", key.display()))?;
    // Encrypted or not, and the password comes from the environment rather than from an
    // argument: a passphrase on a command line is a passphrase in the shell's history and
    // in every process listing on the machine while it runs.
    let secret = match std::env::var("ACL_RELEASE_KEY_PASSWORD") {
        Ok(password) => boxed.into_secret_key(Some(password)),
        Err(_) => boxed.into_unencrypted_secret_key(),
    }
    .map_err(|error| {
        format!(
            "{}: {error}
(an encrypted key needs ACL_RELEASE_KEY_PASSWORD set)",
            key.display()
        )
    })?;

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
