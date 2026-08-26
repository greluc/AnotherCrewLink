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
//! **There is no CPU rung below WARP**, and there was one here until 2026-08-26. §4.8
//! asks for "wgpu/DX12, then WARP through `force_fallback_adapter`, then a CPU
//! rasteriser", which reads as three things and is two: WARP *is* the CPU rasteriser —
//! Windows's own Direct3D 12 implementation, running on the processor.
//! `experiments/gpu-probe` enumerates it as `Cpu` under the name "Microsoft Basic Render
//! Driver", and its driver version is the operating system's build number rather than a
//! vendor's, which is the evidence that it ships with the OS rather than with a card.
//!
//! Nothing was lost by removing it. There is no CPU rasteriser for egui outside a wgpu
//! adapter — no crate provides one — so the third rung named something that could not
//! have been built. What it was there to guarantee still holds, and holds better: the last
//! rung is part of Windows rather than of a driver.
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
/// Deliberately two. Neither a `Glow` rung nor a CPU one below WARP is missing by
/// oversight — see the module documentation — and adding either should be an argued
/// change rather than a convenience.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// wgpu on the platform's own graphics API.
    Hardware,
    /// wgpu on WARP: Windows's Direct3D 12 implementation, running on the processor.
    SoftwareAdapter,
}

/// The renderers to try, best first.
///
/// wgpu over DX12, then WARP. Only the first rung is the setting's to remove: the one
/// below it is what "no GPU is not a failure to launch" means, and it is part of Windows.
#[must_use]
pub fn chain(hardware_acceleration: bool) -> Vec<Renderer> {
    let mut rungs = Vec::with_capacity(2);
    if hardware_acceleration {
        rungs.push(Renderer::Hardware);
    }
    rungs.push(Renderer::SoftwareAdapter);
    rungs
}

/// What one enumerated graphics adapter is.
///
/// Two kinds, because the chain only distinguishes two. wgpu reports five device types;
/// the only question the choice asks is whether an adapter runs on the processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// Real hardware, or something pretending well enough to be worth trying first: a
    /// discrete card, an integrated one, a passed-through device in a virtual machine, or
    /// a driver that declined to say which.
    Gpu,
    /// WARP -- Windows's own Direct3D 12 implementation, running on the processor.
    Cpu,
}

/// Which of the enumerated adapters serves a rung, if any.
///
/// The index rather than the adapter, because the caller holds the real `wgpu::Adapter`
/// values and this crate must not: `acl-ui` has no graphics API dependency and the gates
/// depend on it keeping none. `experiments/gpu-probe` is what maps the real ones onto
/// [`Adapter`].
///
/// The hardware rung takes the *first* GPU rather than the best one. wgpu enumerates in
/// the order DXGI reports, which puts the system's preferred adapter first, and that is
/// already the answer to a question Windows is better placed to answer than this is.
#[must_use]
pub fn choose(rung: Renderer, adapters: &[Adapter]) -> Option<usize> {
    let wanted = match rung {
        Renderer::Hardware => Adapter::Gpu,
        Renderer::SoftwareAdapter => Adapter::Cpu,
    };
    adapters.iter().position(|adapter| *adapter == wanted)
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
    fn windows_tries_hardware_then_warp() {
        assert_eq!(chain(true), [Renderer::Hardware, Renderer::SoftwareAdapter]);
    }

    /// There was a third rung until 2026-08-26, and `experiments/gpu-probe` is what
    /// removed it: WARP enumerates as a `Cpu` adapter called "Microsoft Basic Render
    /// Driver", so the second and third rungs named one adapter twice. There is also no
    /// CPU rasteriser for egui outside a wgpu adapter, so the third could not have been
    /// built.
    #[test]
    fn there_is_no_cpu_rung_below_warp() {
        assert_eq!(chain(true).len(), 2);
        assert_eq!(chain(true).last(), Some(&Renderer::SoftwareAdapter));
    }

    #[test]
    fn there_is_no_gl_rung() {
        // It looks like the obvious middle step and it saves nothing: glow needs GL 3.3 or
        // ES 3.0, and a Windows machine without a vendor driver offers software GL 1.1.
        // The RDP and bare-VM cases a GL rung would exist for are the ones it cannot
        // serve. The chain goes straight from the native API to WARP.
        let windows = chain(true);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1], Renderer::SoftwareAdapter);
    }

    #[test]
    fn turning_acceleration_off_starts_at_software() {
        assert_eq!(chain(false), [Renderer::SoftwareAdapter]);
    }

    #[test]
    fn every_chain_ends_at_something_that_cannot_fail_for_want_of_a_gpu() {
        // "No GPU is not a failure to launch." The last rung is WARP, which is part of
        // Windows rather than of a driver, so there is always somewhere left to go.
        for accelerated in [true, false] {
            let rungs = chain(accelerated);
            assert_eq!(
                rungs.last(),
                Some(&Renderer::SoftwareAdapter),
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

    /// The hardware rung takes the first GPU, not the best one: wgpu enumerates in the
    /// order DXGI reports, which puts the system's preferred adapter first. On the machine
    /// `gpu-probe` ran on that is a discrete card ahead of an integrated one, and Windows
    /// is better placed to make that choice than a list-scan here would be.
    #[test]
    fn the_hardware_rung_takes_the_first_gpu_windows_reports() {
        let machine = [Adapter::Gpu, Adapter::Gpu, Adapter::Cpu];
        assert_eq!(choose(Renderer::Hardware, &machine), Some(0));
        assert_eq!(choose(Renderer::SoftwareAdapter, &machine), Some(2));
    }

    /// A machine with no hardware adapter at all still has somewhere to go, which is the
    /// whole point of the chain. The client demotes; it does not fail to start.
    #[test]
    fn a_machine_with_no_gpu_still_has_a_rung() {
        let bare = [Adapter::Cpu];
        assert_eq!(choose(Renderer::Hardware, &bare), None);
        assert_eq!(choose(Renderer::SoftwareAdapter, &bare), Some(0));
    }

    /// And a machine that enumerates nothing has none, which the caller has to handle
    /// rather than be told a lie about.
    #[test]
    fn an_empty_enumeration_chooses_nothing() {
        for rung in chain(true) {
            assert_eq!(choose(rung, &[]), None, "{rung:?}");
        }
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
