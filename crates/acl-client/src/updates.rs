//! Whether there is a newer version, and starting the thing that installs it.
//!
//! §4.9 item 3 puts the installing in a separate binary and says why: it replaces the
//! client's files, so it must not be one of them. That binary exists and is complete — fetch,
//! verify, decide, download, hand to the installer — and **nothing had ever started it**.
//! There was no update path at all, only the two halves of one.
//!
//! # What this half does, and does not
//!
//! It *asks*. The check is the same three steps the updater's first three are, and they are
//! library calls with no side effects: fetch the manifest, verify the signature against the
//! keys this build trusts, ask the policy. Nothing is downloaded and no file is touched.
//!
//! Installing stays where it belongs. When somebody presses the button, this starts
//! `acl-updater` beside this executable and closes the window, because a client holding its
//! own files open is a client the installer cannot write over.
//!
//! # Why it is a thread
//!
//! The check is two HTTP requests and the window paints five times a second. Anything that
//! waits on a network from the paint is a window that stops answering.

use std::sync::mpsc::{Receiver, Sender};

/// Where the manifest lives.
///
/// GitHub's `releases/latest` redirects to the newest release that is not a draft, so this
/// URL follows the fleet without anybody editing it. The signature is derived from it by
/// [`acl_updater::fetch::Feed`] and is deliberately not a second setting: a feed that could
/// name a manifest in one place and a signature in another would let whoever controls the
/// manifest choose which signature it is checked against.
pub(crate) const FEED: &str =
    "https://github.com/greluc/AnotherCrewLink/releases/latest/download/manifest.json";

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Nothing to do.
    UpToDate,
    /// A newer version is there, and this is what it calls itself.
    Ready(String),
    /// The check did not finish, and this is why.
    ///
    /// Shown rather than swallowed. An updater that silently never offers anything is
    /// indistinguishable from one that has nothing to offer, and the difference is whether
    /// somebody is stuck on an old version without knowing it.
    Trouble(String),
}

/// The check, running once in the background.
pub(crate) struct Updates {
    answers: Receiver<Offer>,
    latest: Option<Offer>,
}

impl Updates {
    /// Starts the check.
    ///
    /// `running` is this build's own version. A build whose version does not parse asks for
    /// nothing: it cannot be compared against, and offering an update on a comparison that
    /// could not be made is worse than offering none.
    pub(crate) fn start(running: &str) -> Self {
        let (answers, receiver) = std::sync::mpsc::channel();
        match running.parse::<semver::Version>() {
            Ok(running) => {
                std::thread::Builder::new()
                    .name("update-check".to_owned())
                    .spawn(move || check(&running, &answers))
                    // A client that cannot start a thread has larger problems, and there is
                    // nowhere to report them from here.
                    .ok();
            }
            Err(why) => {
                let _ = answers.send(Offer::Trouble(format!("this build's version is {why}")));
            }
        }
        Self {
            answers: receiver,
            latest: None,
        }
    }

    /// Takes whatever the check has said. Cheap, and called once a frame.
    pub(crate) fn pump(&mut self) {
        // Drained rather than read once: the check sends a single answer, and a loop that
        // takes whatever is there cannot leave one behind if that ever becomes two.
        while let Ok(offer) = self.answers.try_recv() {
            acl_core::log_info!("update", "{offer:?}");
            self.latest = Some(offer);
        }
    }

    /// What it found, once it has.
    pub(crate) const fn offer(&self) -> Option<&Offer> {
        self.latest.as_ref()
    }
}

/// The three steps that have no side effects.
fn check(running: &semver::Version, answers: &Sender<Offer>) {
    let network = acl_updater::fetch::Http;
    let feed = acl_updater::fetch::Feed { manifest: FEED };
    let manifest = match acl_updater::fetch::manifest(feed, &network) {
        Ok(manifest) => manifest,
        Err(why) => {
            let _ = answers.send(Offer::Trouble(why.to_string()));
            return;
        }
    };
    let decision = acl_updater::policy::decide(
        running,
        &manifest,
        acl_updater::policy::Circumstances {
            // Never from here. This process is the unelevated half by design, and a client
            // that believed otherwise would refuse its own update.
            elevated: false,
            // The rollback bypass is a support instruction somebody types, not something a
            // background check decides for them.
            asked_for_this_version: false,
        },
    );
    let _ = answers.send(match decision {
        acl_updater::policy::Decision::Install => Offer::Ready(manifest.version.to_string()),
        acl_updater::policy::Decision::AlreadyCurrent => Offer::UpToDate,
        // A server offering something older is the attack this refuses, and it is worth
        // saying rather than reading as "nothing new".
        acl_updater::policy::Decision::Downgrade { running, offered } => {
            Offer::Trouble(format!("the feed offers {offered}, older than {running}"))
        }
        acl_updater::policy::Decision::Elevated => {
            Offer::Trouble("this client is elevated and must not install".to_owned())
        }
    });
}

/// Starts the updater and says whether it started.
///
/// Beside this executable, because that is where the installer put it. Not searched for on
/// the path: a program that runs an installer must not be one that anything on the path can
/// become.
///
/// # Errors
///
/// The reason, for showing. The caller closes the window on success — an installer cannot
/// write over files this process is holding open.
pub(crate) fn install() -> Result<(), String> {
    let beside = std::env::current_exe()
        .map_err(|why| format!("cannot find this program: {why}"))?
        .parent()
        .ok_or_else(|| "this program is not in a directory".to_owned())?
        .join("acl-updater.exe");
    if !beside.is_file() {
        return Err(format!("{} is not there", beside.display()));
    }
    let install_directory = beside
        .parent()
        .ok_or_else(|| "the updater is not in a directory".to_owned())?
        .to_path_buf();

    std::process::Command::new(&beside)
        .arg("--feed")
        .arg(FEED)
        .arg("--running")
        .arg(env!("CARGO_PKG_VERSION"))
        .arg("--install-dir")
        .arg(&install_directory)
        .spawn()
        .map(|_| ())
        .map_err(|why| format!("the updater would not start: {why}"))
}

#[cfg(test)]
mod tests {
    use super::{FEED, Offer, Updates};

    /// A version this build cannot parse asks for nothing, and says so.
    ///
    /// Offering an update on a comparison that could not be made is worse than offering
    /// none: the number on screen would be a guess.
    #[test]
    fn a_version_that_does_not_parse_is_reported_rather_than_compared() {
        let mut updates = Updates::start("not a version");
        updates.pump();
        assert!(
            matches!(updates.offer(), Some(Offer::Trouble(_))),
            "got {:?}",
            updates.offer()
        );
    }

    /// The signature is beside the manifest, and neither is a setting.
    ///
    /// A feed that named the two separately would let whoever controls the manifest choose
    /// which signature it is checked against, which is the whole point of the derivation in
    /// `fetch::Feed`.
    #[test]
    fn the_feed_names_one_thing() {
        let feed = acl_updater::fetch::Feed { manifest: FEED };
        assert_eq!(feed.signature(), format!("{FEED}.minisig"));
        assert!(FEED.starts_with("https://"), "and it is fetched over TLS");
    }

    /// Nothing is asked for before the check has answered.
    #[test]
    fn there_is_no_offer_until_there_is_one() {
        let updates = Updates::start("2.0.0");
        assert_eq!(updates.offer(), None);
    }
}
