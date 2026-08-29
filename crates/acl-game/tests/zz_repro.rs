use acl_game::mods::Mod;
use acl_game::offsets::Offsets;
use acl_game::reader::{ReadContext, read_state};
use acl_game::sparse::SparseProcess;
use acl_game::state::GameState;

const BASE: u64 = 0x0100_0000;
const INNER: u64 = 0x0200_0000;
const APPTR: u64 = 0x0300_0000;
const TABLE: u64 = 0x0400_0000;

fn offsets() -> Offsets {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/offsets/offsets__x86__V2026.8.18__offsets.json"),
    )
    .expect("a fixture");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("parses");
    value["innerNetClient"]["base"] = serde_json::json!([0]);
    value["allPlayersPtr"] = serde_json::json!([64]);
    serde_json::from_value(value).expect("still parses")
}

fn process(game_state: u32, game_id: u32) -> SparseProcess {
    SparseProcess::new(false)
        .with_pointer(BASE, INNER)
        .with_region(INNER + 100, game_state.to_le_bytes().to_vec())
        .with_region(INNER + 44, game_id.to_le_bytes().to_vec())
        .with_pointer(BASE + 64, APPTR)
        .with_pointer(APPTR + 8, TABLE)
        .with_region(APPTR + 12, 0u32.to_le_bytes().to_vec())
}

#[test]
fn the_menu_frame_forgets_the_table_the_lobby_frame_remembers() {
    let offsets = offsets();

    // Same process bytes, same readable player table at APPTR+8 -> TABLE.
    let mut menu_ctx = ReadContext::new(BASE, Mod::None);
    let menu = read_state(&process(0, 0x4142_4344), &offsets, &mut menu_ctx).expect("a frame");
    assert_eq!(menu.game_state, GameState::Menu);

    let mut lobby_ctx = ReadContext::new(BASE, Mod::None);
    let lobby = read_state(&process(1, 0x4142_4344), &offsets, &mut lobby_ctx).expect("a frame");
    assert_eq!(lobby.game_state, GameState::Lobby);

    // The lobby frame stores the live table pointer...
    assert_eq!(lobby_ctx.last_player_ptr, TABLE);
    // ...and the menu frame stores zero, from the same bytes.
    assert_eq!(menu_ctx.last_player_ptr, 0);
}
