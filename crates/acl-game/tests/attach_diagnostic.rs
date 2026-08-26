//! Where attaching to a running game gives up.
//!
//! `acl-helper` reports one thing about a failed attach -- that it failed -- and then backs
//! off for seven and a half seconds. That is right for a client and useless for finding out
//! why, so this walks the same four steps and says which one stopped.
//!
//! Ignored: it needs Among Us running.
//!
//! ```text
//! cargo test -p acl-game --test attach_diagnostic -- --ignored --nocapture
//! ```

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use acl_game::ProcessMemory;
use acl_game::offsets::Offsets;
use acl_game::reader::{ReadContext, read_state};
use acl_game::resolve::resolve_offsets;

const X86: &str = include_str!("../assets/offsets-x86.json");
const X64: &str = include_str!("../assets/offsets-x64.json");

#[test]
#[ignore = "needs Among Us to be running"]
fn report_where_the_attach_stops() {
    let process = acl_game::windows::WindowsProcess::open_by_name("Among Us.exe")
        .expect("step 1: the game process opens");
    eprintln!(
        "step 1 ok: pid {} is_64bit={}",
        process.pid(),
        process.is_64bit()
    );

    let module = process
        .module("GameAssembly.dll")
        .expect("step 2: GameAssembly.dll is loaded");
    eprintln!(
        "step 2 ok: base 0x{:x} size 0x{:x}",
        module.base, module.size
    );

    let bundle = if process.is_64bit() { X64 } else { X86 };
    let offsets: Offsets = serde_json::from_str(bundle).expect("step 3: the floor parses");
    eprintln!("step 3 ok: the embedded floor parses");

    let resolved = resolve_offsets(&process, &module, &offsets).expect("step 4: resolution runs");
    eprintln!(
        "step 4: {} signature(s) found, {} missing",
        resolved.found.len(),
        resolved.missing.len()
    );
    for name in &resolved.missing {
        eprintln!("    missing: {name}");
    }

    let mut context = ReadContext::new(module.base, acl_game::mods::Mod::None);
    match read_state(&process, &resolved.offsets, &mut context) {
        Ok(state) => eprintln!(
            "step 5 ok: gameState={:?} map={} players={} lobby={:?}",
            state.game_state,
            state.map,
            state.players.len(),
            state.lobby_code
        ),
        Err(error) => panic!("step 5: the reader gave up: {error}"),
    }
}
