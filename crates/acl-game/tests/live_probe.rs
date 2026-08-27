#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! What the reader sees in a game that is actually running.
//!
//! Not a test: a window onto the values a frame is built from, for the times when the
//! derived state is wrong and the question is which read produced it. It prints and asserts
//! nothing, because there is nothing here to be right about — the game is whatever it is.
//!
//! ```text
//! cargo test -p acl-game --test live_probe -- --ignored --nocapture
//! ```
//!
//! # Why it exists
//!
//! On 2026-08-27 the client showed an empty lobby while a game was plainly running, and the
//! parity corpus — twelve and a half thousand frames — was green. Nothing in the suite could
//! see the difference, because every recorded frame replays reads the *Electron* reader
//! made, and the bug was in how this reader decides which address to ask for.
//!
//! Three lines of output from this told the story: every signature resolved, no field was
//! missing, and the state still came out `Menu` with no players. That is the shape of a
//! reader looking in the wrong place rather than one that cannot look.

use acl_game::memory::ProcessMemory as _;

#[test]
#[ignore = "needs Among Us to be running"]
fn what_the_reader_sees_right_now() {
    let Ok(process) = acl_game::windows::WindowsProcess::open_by_name("Among Us.exe") else {
        eprintln!("Among Us is not running, or cannot be opened");
        return;
    };
    let Some(module) = process.module("GameAssembly.dll") else {
        eprintln!("GameAssembly.dll is not loaded");
        return;
    };
    let sixty_four = process.is_64bit();
    eprintln!(
        "pid {}  64-bit {sixty_four}  GameAssembly at {:#x}",
        process.pid(),
        module.base
    );

    // The vendored bundle, not the one a client would have fetched. Different by design:
    // this asks what *this tree* can read, which is the question when the tree is suspect.
    let bundle = if sixty_four {
        include_str!("../assets/offsets-x64.json")
    } else {
        include_str!("../assets/offsets-x86.json")
    };
    let Ok(parsed) = serde_json::from_str::<acl_game::offsets::Offsets>(bundle) else {
        eprintln!("the vendored bundle did not parse");
        return;
    };

    let resolved = match acl_game::resolve::resolve_offsets(&process, &module, &parsed) {
        Ok(resolved) => {
            // Both halves matter. A missing signature explains a blind reader; *no* missing
            // signature and a blind reader anyway is a different and worse thing.
            eprintln!("signatures found: {:?}", resolved.found);
            eprintln!("signatures missing: {:?}", resolved.missing);
            resolved.offsets
        }
        Err(error) => {
            eprintln!("could not resolve the bundle: {error:?}");
            return;
        }
    };

    let mut context = acl_game::reader::ReadContext::new(module.base, acl_game::mods::Mod::None);
    for round in 0..3 {
        match acl_game::reader::read_state(&process, &resolved, &mut context) {
            Ok(state) => {
                // The first frame in full, because a wrong *name* or a wrong colour is
                // invisible in the counts and is exactly what a mis-walked chain produces.
                if round == 0 {
                    for player in &state.players {
                        eprintln!(
                            "    name={:?} colour={} hat={:?} skin={:?} visor={:?} dead={} local={}",
                            player.name,
                            player.color_id,
                            player.hat_id,
                            player.skin_id,
                            player.visor_id,
                            player.is_dead,
                            player.is_local,
                        );
                    }
                }
                eprintln!(
                    "[{round}] state={:?} code={:?} map={:?} players={} local={}",
                    state.game_state,
                    state.lobby_code,
                    state.map,
                    state.players.len(),
                    state
                        .players
                        .iter()
                        .filter(|player| player.is_local)
                        .count(),
                );
            }
            Err(error) => eprintln!("[{round}] read failed: {error:?}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
