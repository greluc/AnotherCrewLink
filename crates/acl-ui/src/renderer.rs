//! Which renderer to try, in what order, and what to remember about it.
//!
//! §4.8: "No GPU is not a failure to launch." Chromium gives every user `SwiftShader` for
//! free today, and this project has already met the problem in the field — hardware
//! acceleration is switchable through a shipped setting.
//!
//! There was a `Platform` enum here until 2026-08-25. Its `Linux` arm never offered
//! hardware at all, matching an Electron client that disabled acceleration
//! unconditionally there. Both went with the client's Linux support, and the Electron
//! side lost the unconditional arm in the same change.
//!
//! Three decisions, and two of them are about what *not* to do.
//!
//! **There is no glow rung.** It looks like an obvious middle step and it saves nothing:
//! glow needs GL 3.3 or ES 3.0, and a Windows machine without a vendor driver offers
//! software GL 1.1. The cases a GL rung would exist for — RDP sessions, bare virtual
//! machines — are exactly the cases it cannot serve.
//!
//! **An automatic demotion is not written down.** A settings key written by a process in
//! the act of crashing pins that user to the slow rung forever, for a reason that had
//! nothing to do with their GPU. The user's own answer persists; the client's guess does
//! not.
//!
//! And the setting is [`HARDWARE_ACCELERATION_KEY`], which already exists in 1.x's
//! `config.json` and is migrated forward rather than replaced by a new one.

/// The settings key 1.x already writes.
///
/// §4.8: "Migrate the existing `hardware_acceleration` answer forward rather than
/// inventing a key." `the_settings_key_still_exists_in_the_electron_client` reads
/// `ISettings.d.ts` and fails if it is renamed, because a renamed key reads as absent and
/// an absent key means every player who turned acceleration off gets it back on.
pub const HARDWARE_ACCELERATION_KEY: &str = "hardware_acceleration";

/// One rung of the chain.
///
/// Deliberately three. A `Glow` variant is not missing by oversight — see the module
/// documentation — and adding one should be an argued change rather than a convenience.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// wgpu on the platform's own graphics API.
    Hardware,
    /// wgpu's fallback adapter: WARP.
    SoftwareAdapter,
    /// A CPU rasteriser, with no graphics API beneath it at all.
    CpuRasteriser,
}

/// The renderers to try, best first.
///
/// wgpu over DX12, then WARP, then the CPU. Only the first rung is the setting's to
/// remove: the two below it are what "no GPU is not a failure to launch" means.
#[must_use]
pub fn chain(hardware_acceleration: bool) -> Vec<Renderer> {
    let mut rungs = Vec::with_capacity(3);
    if hardware_acceleration {
        rungs.push(Renderer::Hardware);
    }
    rungs.push(Renderer::SoftwareAdapter);
    rungs.push(Renderer::CpuRasteriser);
    rungs
}

/// Why the client is on the rung it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demotion {
    /// The user turned acceleration off.
    UserAsked,
    /// A rung failed and the client stepped down by itself.
    Automatic,
}

/// Whether the choice should be written to the settings.
///
/// Only the user's. A key written by a process in the act of crashing outlives whatever
/// made it crash — a driver update, a machine the game was moved off, one bad session —
/// and the player has no way to connect the slow client they have now to the moment it
/// happened.
#[must_use]
pub const fn should_persist(demotion: Demotion) -> bool {
    matches!(demotion, Demotion::UserAsked)
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn windows_tries_hardware_then_warp_then_the_cpu() {
        assert_eq!(
            chain(true),
            [
                Renderer::Hardware,
                Renderer::SoftwareAdapter,
                Renderer::CpuRasteriser
            ]
        );
    }

    #[test]
    fn there_is_no_gl_rung() {
        // It looks like the obvious middle step and it saves nothing: glow needs GL 3.3 or
        // ES 3.0, and a Windows machine without a vendor driver offers software GL 1.1.
        // The RDP and bare-VM cases a GL rung would exist for are the ones it cannot
        // serve. The chain goes straight from the native API to WARP.
        let windows = chain(true);
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[1], Renderer::SoftwareAdapter);
    }

    #[test]
    fn turning_acceleration_off_starts_at_software() {
        assert_eq!(
            chain(false),
            [Renderer::SoftwareAdapter, Renderer::CpuRasteriser]
        );
    }

    #[test]
    fn every_chain_ends_at_something_that_cannot_fail_for_want_of_a_gpu() {
        // "No GPU is not a failure to launch." The last rung has no graphics API beneath
        // it, so there is always somewhere left to go.
        for accelerated in [true, false] {
            let rungs = chain(accelerated);
            assert_eq!(
                rungs.last(),
                Some(&Renderer::CpuRasteriser),
                "hardware_acceleration={accelerated}"
            );
        }
    }

    #[test]
    fn an_automatic_demotion_is_not_written_down() {
        // A key written by a process in the act of crashing outlives whatever made it
        // crash, and the player has no way to connect the slow client they have now to
        // the moment it happened.
        assert!(!should_persist(Demotion::Automatic));
        assert!(should_persist(Demotion::UserAsked));
    }

    #[test]
    fn the_settings_key_still_exists_in_the_electron_client() {
        // §4.8: migrate the existing answer forward rather than inventing a key. A rename
        // reads as absent, and an absent key gives acceleration back to every player who
        // turned it off -- on the machines least able to run it.
        let settings = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/common/ISettings.d.ts"),
        )
        .expect("the Electron client is beside the crates");
        assert!(
            settings.contains(&format!("{HARDWARE_ACCELERATION_KEY}: boolean")),
            "`{HARDWARE_ACCELERATION_KEY}` is no longer a boolean in ISettings.d.ts"
        );
    }
}
