//! Fetching the hat artwork, and keeping it.
//!
//! [`acl_ui::hats`] parses the index and [`acl_ui::sprite::decode_png`] turns a file into
//! pixels. This is what puts a file in front of the decoder: one fetch of `hats.json`, then
//! one fetch per image the players in this lobby are actually wearing, cached on disk so
//! the second session downloads nothing.
//!
//! Shaped after [`acl_game::store`] rather than invented: a `Fetch` trait so the logic is
//! testable without a network, and one implementation over `ureq` behind the same `http`
//! feature. What is different is that this fetches bytes as well as text, and that it has
//! two safeguards `OffsetStore` does not need.
//!
//! # The two safeguards, and why they are not paranoia
//!
//! **Only the pinned origin.** `hatCollection.ts` says the main process "only recolours
//! images from this exact origin", and the pin is a commit precisely so that what ships is
//! what arrives. A URL from anywhere else is refused — the index is a remote document, and
//! a remote document that can name an arbitrary URL is a remote document that can make this
//! client fetch anything.
//!
//! **Never outside the cache.** The file names come from that same remote document, and a
//! name like `../../../config.json` would otherwise be written wherever it pointed.
//! [`cache_path`] rejects anything that is not a plain file name, and there is a test with
//! the traversal in it.
//!
//! Neither has been exploited and neither is likely to be — the repository is this
//! project's own. They are here because "the file is fetched over a CDN and its name comes
//! from a JSON document" describes the problem exactly, and the cost of being careful is
//! two functions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use acl_ui::hats::Collection;
use acl_ui::sprite::Bitmap;

/// Somewhere to fetch from.
///
/// A trait for the reason [`acl_game::store::Fetcher`] is one: everything interesting here
/// is what happens around the request, and a test that needs a network tests the network.
pub(crate) trait Fetch {
    /// Fetches a document.
    ///
    /// # Errors
    ///
    /// Returns a message describing why, which reaches the user only through a log line.
    fn text(&self, url: &str) -> Result<String, String>;

    /// Fetches a file.
    ///
    /// # Errors
    ///
    /// As above.
    fn bytes(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// How long any one request may take.
///
/// Shorter than the offsets store's, and deliberately: the offsets decide whether the
/// client works at all, and a hat decides whether somebody's avatar has a hat.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The real one.
#[derive(Debug, Default, Clone)]
pub(crate) struct Http;

#[cfg(feature = "http")]
impl Fetch for Http {
    fn text(&self, url: &str) -> Result<String, String> {
        let mut response = agent().get(url).call().map_err(|error| error.to_string())?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("{url}: {error}"))
    }

    fn bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = agent().get(url).call().map_err(|error| error.to_string())?;
        response
            .body_mut()
            .read_to_vec()
            .map_err(|error| format!("{url}: {error}"))
    }
}

/// One agent per request, matching `OffsetStore`. These are rare enough that pooling would
/// be an optimisation of something that happens once per hat per installation.
#[cfg(feature = "http")]
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .new_agent()
}

#[cfg(not(feature = "http"))]
impl Fetch for Http {
    fn text(&self, _url: &str) -> Result<String, String> {
        Err("this build has no HTTP client; enable the `http` feature".to_owned())
    }

    fn bytes(&self, _url: &str) -> Result<Vec<u8>, String> {
        Err("this build has no HTTP client; enable the `http` feature".to_owned())
    }
}

/// The index, the artwork, and the directory both live in.
pub(crate) struct Store {
    cache: PathBuf,
    collection: Collection,
    /// Decoded artwork, by the URL it came from.
    ///
    /// Decoding a 270×428 PNG for every player on every frame would be the most expensive
    /// thing this client does. Held by URL rather than by file name because two collections
    /// may name the same file.
    decoded: HashMap<String, Option<Bitmap>>,
    /// What went wrong last, for the window to show.
    trouble: Option<String>,
}

impl Store {
    /// Opens the cache, reading whatever is already in it.
    ///
    /// A cache that is missing or unreadable is an empty collection, not an error: the
    /// avatars draw without their layers until a fetch succeeds, which is what the Electron
    /// client does before `initializedHats`.
    pub(crate) fn open(cache: PathBuf) -> Self {
        let collection = std::fs::read_to_string(cache.join("hats.json"))
            .map_or_else(|_| Collection::default(), |text| Collection::parse(&text));
        Self {
            cache,
            collection,
            decoded: HashMap::new(),
            trouble: None,
        }
    }

    /// The index.
    pub(crate) const fn collection(&self) -> &Collection {
        &self.collection
    }

    /// What went wrong last, if anything has.
    pub(crate) fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    /// Fetches the index and writes it to the cache.
    ///
    /// Only when there is nothing already: the pin is a commit, so a cached index cannot be
    /// stale for a build that has not changed its pin. A build whose pin *has* moved writes
    /// its own copy, because the URL it fetches is a different one.
    pub(crate) fn refresh(&mut self, fetch: &dyn Fetch) {
        if !self.collection.is_empty() {
            return;
        }
        let url = format!("{}hats.json", acl_types::cosmetics::HAT_COLLECTION_URL);
        match fetch.text(&url) {
            Ok(text) => {
                let collection = Collection::parse(&text);
                if collection.is_empty() {
                    self.trouble = Some(format!("{url}: the collection came back empty"));
                    return;
                }
                // Written before it is used, so a fetch that succeeds once is a fetch that
                // never has to happen again -- and after it parsed, so a body that is a
                // rate-limit page is not cached as if it were the index.
                let _ = std::fs::create_dir_all(&self.cache);
                let _ = std::fs::write(self.cache.join("hats.json"), &text);
                self.collection = collection;
                self.trouble = None;
            }
            Err(error) => self.trouble = Some(error),
        }
    }

    /// The artwork at a URL, fetching and caching it the first time.
    ///
    /// `None` when it cannot be had — a refused URL, a failed fetch, a file that is not a
    /// PNG. Every one of those costs one cosmetic layer, which is what the Electron client
    /// loses too when an image 404s.
    ///
    /// The failure is remembered as well as the success. Without that, a hat that is
    /// missing from the collection is re-fetched for every player on every frame.
    pub(crate) fn image(&mut self, url: &str, fetch: &dyn Fetch) -> Option<&Bitmap> {
        if !self.decoded.contains_key(url) {
            let bitmap = self.load(url, fetch);
            self.decoded.insert(url.to_owned(), bitmap);
        }
        self.decoded.get(url)?.as_ref()
    }

    /// Reads one image from the cache, or fetches it into the cache first.
    fn load(&mut self, url: &str, fetch: &dyn Fetch) -> Option<Bitmap> {
        let path = cache_path(&self.cache, url)?;
        if let Ok(bytes) = std::fs::read(&path) {
            return acl_ui::sprite::decode_png(&bytes);
        }
        if !is_from_the_pinned_collection(url) {
            self.trouble = Some(format!("{url}: not from the pinned collection"));
            return None;
        }
        let bytes = match fetch.bytes(url) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.trouble = Some(error);
                return None;
            }
        };
        // Decoded before it is written, so a 404 page is not cached as a hat that will
        // never decode for the rest of the installation's life.
        let bitmap = acl_ui::sprite::decode_png(&bytes)?;
        if let Some(directory) = path.parent() {
            let _ = std::fs::create_dir_all(directory);
        }
        let _ = std::fs::write(&path, &bytes);
        Some(bitmap)
    }
}

/// Whether a URL is one this client may fetch.
///
/// The pinned collection and nothing else. See the module documentation.
fn is_from_the_pinned_collection(url: &str) -> bool {
    url.starts_with(acl_types::cosmetics::HAT_COLLECTION_URL)
}

/// Where a URL's file goes in the cache.
///
/// `None` for anything whose last two segments are not plain names. The names come from a
/// remote document, and a document that can name `../../config.json` can otherwise write
/// there.
fn cache_path(cache: &Path, url: &str) -> Option<PathBuf> {
    let tail = url.rsplit('/').take(2).collect::<Vec<&str>>();
    let (file, set) = match tail.as_slice() {
        [file, set] => (*file, *set),
        _ => return None,
    };
    if !is_plain_name(set) || !is_plain_name(file) {
        return None;
    }
    Some(cache.join(set).join(file))
}

/// Whether a path segment is a plain name.
///
/// Deliberately narrow: letters, digits, and the punctuation a percent-encoded segment can
/// contain. A permissive rule here is the whole of the problem — every character that is
/// not on this list is one somebody has to think about.
///
/// `%` is on it because the URLs are encoded — eighteen of the shipped names have a space,
/// one of them an apostrophe too — so the cache file is named `pk01_Captain%27s%20Hat.png`.
/// That is a
/// plain file name with no separator in it, which is exactly what this is checking for: a
/// `%2F` in a name is four characters, not a directory.
fn is_plain_name(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-~%".contains(character))
}

/// The store, on a thread of its own.
///
/// A fetch is a network round trip and the frame it would block is one of five a second,
/// so nothing on the drawing path may wait for one. This owns the thread that does: the
/// window asks for a URL, gets whatever is already decoded, and the answer turns up a frame
/// or a few hundred milliseconds later.
///
/// **Requests are not repeated while one is outstanding.** Without that, a hat that takes
/// two seconds to arrive is asked for ten times before it does — the compose path runs
/// again long before the answer comes back.
pub(crate) struct Loader {
    wanted: std::sync::mpsc::Sender<String>,
    ready: std::sync::mpsc::Receiver<Loaded>,
    /// What has arrived. `None` is a failure, remembered so it is not asked for again.
    known: HashMap<String, Option<Bitmap>>,
    /// What has been asked for and has not arrived.
    pending: std::collections::HashSet<String>,
    collection: Collection,
    trouble: Option<String>,
}

/// One thing the loader thread has finished.
enum Loaded {
    /// The index, parsed.
    Index(Box<Collection>),
    /// One image, or the fact that it could not be had.
    Image(String, Option<Bitmap>),
    /// Something went wrong, for the window to show.
    Trouble(String),
}

impl Loader {
    /// Starts the thread and asks it for the index.
    pub(crate) fn start(cache: PathBuf) -> Self {
        let (wanted, requests) = std::sync::mpsc::channel::<String>();
        let (answers, ready) = std::sync::mpsc::channel::<Loaded>();
        std::thread::Builder::new()
            .name("hats".to_owned())
            .spawn(move || {
                let mut store = Store::open(cache);
                let fetch = Http;
                store.refresh(&fetch);
                if let Some(trouble) = store.trouble() {
                    let _ = answers.send(Loaded::Trouble(trouble.to_owned()));
                }
                if answers
                    .send(Loaded::Index(Box::new(store.collection().clone())))
                    .is_err()
                {
                    return;
                }
                // Ends when the window drops its sender, which is when the client closes.
                while let Ok(url) = requests.recv() {
                    let bitmap = store.image(&url, &fetch).cloned();
                    if answers.send(Loaded::Image(url, bitmap)).is_err() {
                        return;
                    }
                }
            })
            // A client that cannot start a thread has bigger problems than hats, and this
            // is not the place to report them: the loader simply never answers.
            .ok();
        Self {
            wanted,
            ready,
            known: HashMap::new(),
            pending: std::collections::HashSet::new(),
            collection: Collection::default(),
            trouble: None,
        }
    }

    /// Takes whatever the thread has finished. Cheap, and called once a frame.
    pub(crate) fn pump(&mut self) {
        while let Ok(loaded) = self.ready.try_recv() {
            match loaded {
                Loaded::Index(collection) => self.collection = *collection,
                Loaded::Image(url, bitmap) => {
                    self.pending.remove(&url);
                    self.known.insert(url, bitmap);
                }
                Loaded::Trouble(message) => self.trouble = Some(message),
            }
        }
    }

    /// The index, empty until it has arrived.
    pub(crate) const fn collection(&self) -> &Collection {
        &self.collection
    }

    /// What went wrong, if anything has.
    pub(crate) fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    /// The artwork at a URL, if it is here yet.
    ///
    /// Asks for it if it is not, and returns `None` either way: this frame draws without
    /// that layer and a later one draws with it. A cosmetic that arrives a frame late is
    /// not worth a stalled window.
    pub(crate) fn image(&mut self, url: &str) -> Option<&Bitmap> {
        if let Some(known) = self.known.get(url) {
            return known.as_ref();
        }
        if self.pending.insert(url.to_owned()) {
            // A failed send means the thread is gone, which is a client on its way out.
            let _ = self.wanted.send(url.to_owned());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Fetch, Store, cache_path, is_from_the_pinned_collection};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    const PINNED: &str = acl_types::cosmetics::HAT_COLLECTION_URL;

    /// A network that answers from a table and counts what was asked of it.
    #[derive(Default)]
    struct Fake {
        text: String,
        bytes: Vec<u8>,
        asked: RefCell<Vec<String>>,
        fail: bool,
    }

    impl Fetch for Fake {
        fn text(&self, url: &str) -> Result<String, String> {
            self.asked.borrow_mut().push(url.to_owned());
            if self.fail {
                return Err(format!("{url}: refused"));
            }
            Ok(self.text.clone())
        }

        fn bytes(&self, url: &str) -> Result<Vec<u8>, String> {
            self.asked.borrow_mut().push(url.to_owned());
            if self.fail {
                return Err(format!("{url}: refused"));
            }
            Ok(self.bytes.clone())
        }
    }

    fn artwork() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/hats/ratHat.png"),
        )
        .expect("the vendored artwork")
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join("acl-hat-store").join(name);
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    /// A fetched index is written to the cache, so the next session downloads nothing.
    #[test]
    fn the_index_is_fetched_once_and_kept() {
        let cache = scratch("kept");
        let fake = Fake {
            text: r#"{"NONE":{"hats":{"hat_x":{"image":"x.png"}}}}"#.to_owned(),
            ..Fake::default()
        };

        let mut store = Store::open(cache.clone());
        assert!(store.collection().is_empty());
        store.refresh(&fake);
        assert_eq!(store.collection().len(), 1);
        assert_eq!(fake.asked.borrow().len(), 1);

        // Again, with nothing to fetch from: it comes off the disk.
        let reopened = Store::open(cache);
        assert_eq!(reopened.collection().len(), 1);
    }

    /// A second refresh asks for nothing. The pin is a commit, so a cached index cannot be
    /// stale for a build that has not moved it.
    #[test]
    fn a_second_refresh_asks_for_nothing() {
        let fake = Fake {
            text: r#"{"NONE":{"hats":{"hat_x":{"image":"x.png"}}}}"#.to_owned(),
            ..Fake::default()
        };
        let mut store = Store::open(scratch("second"));
        store.refresh(&fake);
        store.refresh(&fake);
        store.refresh(&fake);
        assert_eq!(fake.asked.borrow().len(), 1);
    }

    /// A body that does not parse is not written to the cache. Otherwise a rate-limit page
    /// is kept as the collection, and the client never fetches the real one again.
    #[test]
    fn a_body_that_is_not_the_collection_is_not_cached() {
        let cache = scratch("garbage");
        let fake = Fake {
            text: "<html>rate limited</html>".to_owned(),
            ..Fake::default()
        };
        let mut store = Store::open(cache.clone());
        store.refresh(&fake);
        assert!(store.collection().is_empty());
        assert!(store.trouble().is_some(), "and it says so");
        assert!(!cache.join("hats.json").exists(), "it was cached anyway");
    }

    /// A fetched image is decoded, cached, and not fetched again.
    #[test]
    fn an_image_is_fetched_once_and_decoded() {
        let cache = scratch("image");
        let url = format!("{PINNED}NONE/ratHat.png");
        let fake = Fake {
            bytes: artwork(),
            ..Fake::default()
        };
        let mut store = Store::open(cache.clone());

        let first = store.image(&url, &fake).expect("a bitmap");
        assert_eq!((first.width, first.height), (270, 428));
        assert!(cache.join("NONE").join("ratHat.png").exists());

        store.image(&url, &fake).expect("still a bitmap");
        assert_eq!(fake.asked.borrow().len(), 1, "it was fetched twice");
    }

    /// And a failure is remembered too. Without that, a hat that 404s is re-fetched for
    /// every player on every frame.
    #[test]
    fn a_failure_is_remembered_rather_than_retried_every_frame() {
        let url = format!("{PINNED}NONE/missing.png");
        let fake = Fake {
            fail: true,
            ..Fake::default()
        };
        let mut store = Store::open(scratch("failure"));
        for _ in 0..5 {
            assert!(store.image(&url, &fake).is_none());
        }
        assert_eq!(fake.asked.borrow().len(), 1);
        assert!(store.trouble().is_some());
    }

    /// Bytes that are not a PNG are not cached, for the same reason a rate-limit page is
    /// not: a 404 body kept on disk is a hat that never decodes again.
    #[test]
    fn bytes_that_are_not_a_png_are_not_cached() {
        let cache = scratch("notpng");
        let url = format!("{PINNED}NONE/x.png");
        let fake = Fake {
            bytes: b"<html>not found</html>".to_vec(),
            ..Fake::default()
        };
        let mut store = Store::open(cache.clone());
        assert!(store.image(&url, &fake).is_none());
        assert!(!cache.join("NONE").join("x.png").exists());
    }

    /// The index is a remote document. A URL from anywhere else is refused rather than
    /// fetched, which is the constraint `hatCollection.ts` states and nothing enforced.
    #[test]
    fn only_the_pinned_collection_is_fetched_from() {
        assert!(is_from_the_pinned_collection(&format!(
            "{PINNED}NONE/x.png"
        )));
        for elsewhere in [
            "https://example.invalid/NONE/x.png",
            "http://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@main/NONE/x.png",
            "https://cdn.jsdelivr.net/gh/somebody-else/Hats@abc/NONE/x.png",
            "file:///C:/Windows/System32/config/SAM",
            "",
        ] {
            assert!(
                !is_from_the_pinned_collection(elsewhere),
                "{elsewhere} was allowed"
            );
        }

        let fake = Fake {
            bytes: artwork(),
            ..Fake::default()
        };
        let mut store = Store::open(scratch("origin"));
        assert!(
            store
                .image("https://example.invalid/NONE/x.png", &fake)
                .is_none()
        );
        assert!(
            fake.asked.borrow().is_empty(),
            "a refused URL was fetched anyway"
        );
    }

    /// The file names come from that same remote document, so a name that climbs out of the
    /// cache is refused before anything is written.
    #[test]
    fn a_name_that_climbs_out_of_the_cache_is_refused() {
        let cache = Path::new("/cache");
        assert_eq!(
            cache_path(cache, &format!("{PINNED}NONE/ratHat.png")),
            Some(cache.join("NONE").join("ratHat.png"))
        );
        for hostile in [
            "https://x/../../config.json",
            "https://x/NONE/../../../config.json",
            "https://x/NONE/..%2f..%2fconfig.json",
            "https://x/NONE/",
            "https://x/NONE/C:\\Windows\\System32\\drivers\\etc\\hosts",
            "https://x/NONE/a/b.png",
            "nofile",
        ] {
            let path = cache_path(cache, hostile);
            assert!(
                path.as_ref().is_none_or(|path| path.starts_with(cache)
                    && path.components().count() == cache.components().count() + 2),
                "{hostile} resolved to {path:?}"
            );
        }
        assert_eq!(cache_path(cache, "https://x/../../config.json"), None);
        assert_eq!(cache_path(cache, "https://x/NONE/"), None);
    }

    /// Every file name in the shipped collection passes the name rule. A rule so narrow
    /// that it rejects the real artwork is a rule that quietly disables every hat.
    #[test]
    fn every_shipped_file_name_is_accepted() {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/hats/hats.json"),
        )
        .expect("the vendored collection");
        let collection = acl_ui::hats::Collection::parse(&text);
        let cache = Path::new("/cache");
        let mut checked = 0;
        for id in collection.ids() {
            let Some(found) = collection.find(id, acl_ui::hats::BASE) else {
                continue;
            };
            for back in [false, true] {
                let Some(url) = found.image_url(PINNED, back) else {
                    continue;
                };
                assert!(
                    cache_path(cache, &url).is_some(),
                    "{url} was refused by the name rule"
                );
                checked += 1;
            }
        }
        assert!(checked > 1000, "only {checked} names were checked");
    }

    /// The one that needs the internet, so it is not run by default -- the same rule the
    /// reader's and the session's live tests follow.
    ///
    /// It fetches the name that made the encoding necessary: `pk01_Captain's Hat.png` has
    /// both a space and an apostrophe, and is the file that would fail first if the
    /// encoding regressed.
    ///
    /// ```text
    /// cargo test -p acl-client -- --ignored against_the_real_collection
    /// ```
    #[test]
    #[ignore = "fetches from the hat CDN"]
    fn against_the_real_collection() {
        let cache = scratch("live");
        let mut store = Store::open(cache.clone());
        store.refresh(&super::Http);
        assert!(
            store.trouble().is_none(),
            "the index did not arrive: {:?}",
            store.trouble()
        );
        assert_eq!(store.collection().len(), 983);

        let found = store
            .collection()
            .ids()
            .find(|id| {
                store
                    .collection()
                    .find(id, acl_ui::hats::BASE)
                    .and_then(|found| found.hat.image.clone())
                    .is_some_and(|image| image.contains(' '))
            })
            .and_then(|id| store.collection().find(id, acl_ui::hats::BASE))
            .expect("a name with a space in it");
        let url = found
            .image_url(PINNED, false)
            .expect("that name has an image");
        assert!(url.contains("%20"), "{url}");

        let bitmap = store.image(&url, &super::Http).expect("the artwork");
        assert!(bitmap.width > 0 && bitmap.height > 0);
        assert!(cache.join("NONE").exists(), "it was decoded but not cached");
    }
}
