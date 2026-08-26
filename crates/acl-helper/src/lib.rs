//! What the elevated half is, apart from its `main`.
//!
//! One module today. It is a library as well as a binary so that the overlay can be tested
//! against a real window: an integration test cannot reach inside a binary crate, and the
//! thing worth checking about a layered window is what the operating system says its
//! extended styles and geometry actually are.

pub mod overlay;
