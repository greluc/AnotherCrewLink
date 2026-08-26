//! The updater: what a release says about itself, and whether to act on it.
//!
//! §4.9 item 3. `self_update` 0.44.0 is not shippable — its non-optional `quick-xml ^0.38`
//! carries two CVSS 7.5 advisories the caret cannot escape, and its signature feature
//! verifies nothing about an NSIS `.exe` anyway. The plan offered two ways out: track
//! `self_update`'s 1.0 line and pin once stable, or build around `minisign` verification
//! and `self-replace`.
//!
//! **Measured 2026-08-26: `self_update` is at `1.0.0-rc.6`.** It is not stable, so "pin
//! once stable" is not a thing that can be done today, and the choice is settled by
//! availability rather than by preference.
//!
//! What is here is the half that is all decision and no side effect: [`manifest`] says
//! whether a release is ours, and [`policy`] says whether to install it. Neither downloads
//! anything, runs anything, or touches a file — which is why both are tested rather than
//! argued about.

pub mod manifest;
pub mod policy;
