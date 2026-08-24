//! Sabotage, doors and security cameras.
//!
//! Three fields of `AmongUsState` that this reader carried as constants until 2026-08-24:
//! `comsSabotaged` was always false, `closedDoors` always empty, and `currentCamera`
//! always zero — which is not even a neutral value, because zero is the engine room
//! camera and the Electron reader's default is `NONE`.
//!
//! They matter to what a player hears. Comms sabotage cuts positional audio, a closed
//! door is a wall the collider has to know about, and a camera moves where the local
//! player is heard from. A reader that reports none of them sounds like a game in which
//! nothing is ever sabotaged and no door ever shuts.
//!
//! Everything here is read only while the game is in `Tasks`, as `GameReader.ts` does:
//! the systems dictionary and the door table are not meaningful in a lobby or a meeting.

use acl_types::map::MapType;

use crate::dotnet::read_dictionary;
use crate::memory::{ProcessMemory, read_pointer};

/// The Electron reader's `CameraLocation.NONE`, which is a value rather than an absence.
pub const CAMERA_NONE: u32 = 7;

/// The Electron reader's `CameraLocation.Skeld`.
const CAMERA_SKELD: u32 = 6;

/// `SystemTypes.Comms`.
const SYSTEM_COMMS: i32 = 14;

/// `SystemTypes.Decontamination`, which on Mira HQ is a pair of doors.
const SYSTEM_DECONTAMINATION: i32 = 18;

/// How many systems the dictionary is asked for, matching `GameReader.ts`.
const MAX_SYSTEMS: usize = 47;

/// How many doors are read, matching `GameReader.ts`.
const MAX_DOORS: i32 = 16;

/// Mira HQ reports comms sabotaged while fewer than two consoles are complete.
const MIRA_CONSOLES_FOR_CLEAR: u32 = 2;

/// The Skeld's camera console is one place, so proximity to it is the test.
const SKELD_CAMERA: (f32, f32) = (-12.9364, -2.7928);

/// How close the local player must be to count as at The Skeld's camera console.
const SKELD_CAMERA_RANGE: f32 = 0.6;

/// The Polus and Airship camera minigames have six cameras; anything else is a bad read.
const SURVEILLANCE_CAMERAS: u32 = 6;

/// The Skeld's camera minigame filters to four rooms.
const SKELD_FILTERED_ROOMS: u32 = 4;

/// What one frame's systems, doors and cameras came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Systems {
    /// Whether communications are sabotaged, which cuts positional audio.
    pub coms_sabotaged: bool,
    /// Which doors are shut, by index, for the collider.
    pub closed_doors: Vec<u32>,
    /// Which security camera the local player is looking through.
    pub current_camera: u32,
}

impl Default for Systems {
    fn default() -> Self {
        Self {
            coms_sabotaged: false,
            closed_doors: Vec::new(),
            // `CameraLocation.NONE`, not `East`.
            current_camera: CAMERA_NONE,
        }
    }
}

/// Where each field lives, resolved from the bundle by the caller.
///
/// Passed as a struct rather than threaded through thirteen arguments, and every field is
/// optional because a bundle for an older build may not carry all of them — a missing
/// offset means that one reading is skipped, not that the frame fails.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemOffsets {
    /// `shipStatus_systems`, the dictionary of sabotage systems.
    pub systems: Option<i64>,
    /// `HudOverrideSystemType_isActive`, on every map but Mira HQ.
    pub hud_override_active: Option<i64>,
    /// `hqHudSystemType_CompletedConsoles`, Mira HQ's counter.
    pub mira_completed_consoles: Option<i64>,
    /// `deconDoorLowerOpen`.
    pub decon_lower_open: Option<i64>,
    /// `deconDoorUpperOpen`.
    pub decon_upper_open: Option<i64>,
    /// `shipstatus_allDoors`.
    pub all_doors: Option<i64>,
    /// `playerCount`, which the door table reuses as its length field.
    pub count: Option<i64>,
    /// `playerAddrPtr`, which the door table reuses as its first element.
    pub first_element: Option<i64>,
    /// `door_isOpen`.
    pub door_is_open: Option<i64>,
    /// `objectCachePtr`, which says whether a minigame is actually open.
    pub object_cache: Option<i64>,
    /// `planetSurveillanceMinigame_currentCamera`.
    pub current_camera: Option<i64>,
    /// `planetSurveillanceMinigame_camarasCount`.
    pub camera_count: Option<i64>,
    /// `surveillanceMinigame_FilteredRoomsCount`.
    pub filtered_rooms: Option<i64>,
}

/// Reads the sabotage systems, the cameras and the door table.
///
/// `ship` is the resolved `shipStatus` pointer and `minigame` the resolved `miniGame`
/// one; both are passed in because the caller has already walked them for the map.
#[must_use]
pub fn read_systems(
    memory: &dyn ProcessMemory,
    ship: u64,
    minigame: u64,
    map: MapType,
    local_position: Option<(f32, f32)>,
    offsets: &SystemOffsets,
) -> Systems {
    let mut systems = Systems::default();

    read_sabotage(memory, ship, map, offsets, &mut systems);
    read_camera(memory, minigame, map, local_position, offsets, &mut systems);
    read_doors(memory, ship, map, offsets, &mut systems);

    systems
}

/// The systems dictionary: comms on every map, and Mira HQ's decontamination doors.
fn read_sabotage(
    memory: &dyn ProcessMemory,
    ship: u64,
    map: MapType,
    offsets: &SystemOffsets,
    into: &mut Systems,
) {
    let Some(systems_ptr) = at(memory, ship, offsets.systems, read_ptr) else {
        return;
    };
    if systems_ptr == 0 {
        return;
    }
    let Ok(entries) = read_dictionary(memory, systems_ptr, MAX_SYSTEMS) else {
        return;
    };

    for entry in entries {
        let Some(key) = read_i32(memory, entry.key) else {
            continue;
        };
        let Some(value) = read_ptr(memory, entry.value) else {
            continue;
        };

        if key == SYSTEM_COMMS {
            into.coms_sabotaged = if map == MapType::MiraHq {
                // Mira HQ counts consoles instead of carrying a flag.
                at(memory, value, offsets.mira_completed_consoles, read_u32)
                    .is_some_and(|done| done < MIRA_CONSOLES_FOR_CLEAR)
            } else {
                at(memory, value, offsets.hud_override_active, read_u32) == Some(1)
            };
        } else if key == SYSTEM_DECONTAMINATION && map == MapType::MiraHq {
            // A decontamination door counts as a closed door for the collider, and the
            // two halves are reported separately.
            if at(memory, value, offsets.decon_lower_open, read_i32) == Some(0) {
                into.closed_doors.push(0);
            }
            if at(memory, value, offsets.decon_upper_open, read_i32) == Some(0) {
                into.closed_doors.push(1);
            }
        }
    }
}

/// Which camera the local player is looking through, if any.
fn read_camera(
    memory: &dyn ProcessMemory,
    minigame: u64,
    map: MapType,
    local_position: Option<(f32, f32)>,
    offsets: &SystemOffsets,
    into: &mut Systems,
) {
    // A minigame pointer outlives the minigame it described, which is why the cache
    // pointer is checked: it is what says the console is open now rather than last used.
    let open = at(memory, minigame, offsets.object_cache, read_ptr).unwrap_or(0) != 0;
    let Some(position) = local_position else {
        return;
    };
    if !open {
        return;
    }

    match map {
        MapType::Polus | MapType::Airship => {
            let camera = at(memory, minigame, offsets.current_camera, read_u32);
            let count = at(memory, minigame, offsets.camera_count, read_u32);
            // The count check rejects a stale or half-initialised minigame, whose camera
            // index would otherwise be believed.
            if let Some(camera) = camera
                && count == Some(SURVEILLANCE_CAMERAS)
                && camera < SURVEILLANCE_CAMERAS
            {
                into.current_camera = camera;
            }
        }
        // The Skeld's minigame carries no camera index — there is one console, so
        // standing at it is the test.
        MapType::TheSkeld | MapType::TheSkeldApril
            if at(memory, minigame, offsets.filtered_rooms, read_u32)
                == Some(SKELD_FILTERED_ROOMS) =>
        {
            let dx = position.0 - SKELD_CAMERA.0;
            let dy = position.1 - SKELD_CAMERA.1;
            if dy.mul_add(dy, dx * dx).sqrt() < SKELD_CAMERA_RANGE {
                into.current_camera = CAMERA_SKELD;
            }
        }
        _ => {}
    }
}

/// The door table. Mira HQ has none: its doors come from the decontamination system.
fn read_doors(
    memory: &dyn ProcessMemory,
    ship: u64,
    map: MapType,
    offsets: &SystemOffsets,
    into: &mut Systems,
) {
    if map == MapType::MiraHq {
        return;
    }
    let Some(all_doors) = at(memory, ship, offsets.all_doors, read_ptr) else {
        return;
    };
    let Some(count) = at(memory, all_doors, offsets.count, read_i32) else {
        return;
    };
    let count = count.clamp(0, MAX_DOORS);
    let Some(first) = offsets.first_element else {
        return;
    };
    let stride = if memory.is_64bit() { 8 } else { 4 };

    for index in 0..count {
        let Some(slot) = element(all_doors, first, index, stride) else {
            continue;
        };
        let Some(door) = read_ptr(memory, slot) else {
            continue;
        };
        // Only a door that reads as explicitly open is open. An unreadable one counts as
        // shut, which errs towards blocking audio rather than leaking it through a wall.
        if at(memory, door, offsets.door_is_open, read_i32) != Some(1)
            && let Ok(index) = u32::try_from(index)
        {
            into.closed_doors.push(index);
        }
    }
}

/// Address of one element of the door table, refusing an overflow rather than wrapping.
fn element(base: u64, first: i64, index: i32, stride: i64) -> Option<u64> {
    let step = i64::from(index).checked_mul(stride)?;
    base.checked_add_signed(first.checked_add(step)?)
}

/// Reads through an optional offset, or gives `None` if the bundle does not carry it.
fn at<T>(
    memory: &dyn ProcessMemory,
    base: u64,
    offset: Option<i64>,
    read: fn(&dyn ProcessMemory, u64) -> Option<T>,
) -> Option<T> {
    if base == 0 {
        return None;
    }
    read(memory, base.checked_add_signed(offset?)?)
}

fn read_ptr(memory: &dyn ProcessMemory, address: u64) -> Option<u64> {
    read_pointer(memory, address).ok()
}

fn read_u32(memory: &dyn ProcessMemory, address: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    memory.read_exact(address, &mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_i32(memory: &dyn ProcessMemory, address: u64) -> Option<i32> {
    read_u32(memory, address).map(u32::cast_signed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::sparse::SparseProcess;

    const BASE: u64 = 0x1000;
    const SIZE: usize = 0x8000;

    const SHIP: u64 = 0x2000;
    const DICT: u64 = 0x3000;
    const ENTRIES: u64 = 0x4000;
    const SYSTEM: u64 = 0x5000;
    const DOORS: u64 = 0x6000;
    const MINIGAME: u64 = 0x7000;

    /// A 32-bit process with one writable region, so a layout can be poked into place.
    ///
    /// 32-bit on purpose: it is the pointer width the surviving Among Us builds use, and
    /// the dictionary's entry stride differs between the two widths.
    struct Layout {
        bytes: Vec<u8>,
    }

    impl Layout {
        fn new() -> Self {
            Self {
                bytes: vec![0u8; SIZE],
            }
        }

        fn at(&mut self, address: u64, value: u32) -> &mut Self {
            let start = usize::try_from(address - BASE).unwrap();
            self.bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
            self
        }

        fn process(&self) -> SparseProcess {
            SparseProcess::new(false).with_region(BASE, self.bytes.clone())
        }
    }

    fn offsets() -> SystemOffsets {
        SystemOffsets {
            systems: Some(0x30),
            hud_override_active: Some(0x8),
            mira_completed_consoles: Some(0xc),
            decon_lower_open: Some(0x10),
            decon_upper_open: Some(0x14),
            all_doors: Some(0x40),
            count: Some(0x8),
            first_element: Some(0x10),
            door_is_open: Some(0x14),
            object_cache: Some(0x4),
            current_camera: Some(0x20),
            camera_count: Some(0x24),
            filtered_rooms: Some(0x28),
        }
    }

    /// Lays out a one-entry dictionary holding `key`, whose value points at [`SYSTEM`].
    fn with_system(layout: &mut Layout, key: u32) {
        layout.at(SHIP + 0x30, u32::try_from(DICT).unwrap());
        // 32-bit dictionary: entries pointer at +0xc, count at +0x10.
        layout.at(DICT + 0xc, u32::try_from(ENTRIES).unwrap());
        layout.at(DICT + 0x10, 1);
        // The first entry sits 0x10 into the array; its value is 0xc further on.
        layout.at(ENTRIES + 0x10, key);
        layout.at(ENTRIES + 0x10 + 0xc, u32::try_from(SYSTEM).unwrap());
    }

    #[test]
    fn reads_comms_sabotage_from_the_override_flag() {
        let mut layout = Layout::new();
        with_system(&mut layout, 14);
        layout.at(SYSTEM + 0x8, 1);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            0,
            MapType::TheSkeld,
            None,
            &offsets(),
        );
        assert!(systems.coms_sabotaged);
    }

    #[test]
    fn a_clear_override_flag_is_not_sabotage() {
        let mut layout = Layout::new();
        with_system(&mut layout, 14);
        layout.at(SYSTEM + 0x8, 0);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            0,
            MapType::TheSkeld,
            None,
            &offsets(),
        );
        assert!(!systems.coms_sabotaged);
    }

    #[test]
    fn mira_counts_consoles_instead_of_carrying_a_flag() {
        // Mira HQ has no override flag: comms are down while fewer than two consoles are
        // complete, which is why the map must be known before the field is read.
        for (completed, sabotaged) in [(0u32, true), (1, true), (2, false), (3, false)] {
            let mut layout = Layout::new();
            with_system(&mut layout, 14);
            layout.at(SYSTEM + 0xc, completed);

            let systems = read_systems(
                &layout.process(),
                SHIP,
                0,
                MapType::MiraHq,
                None,
                &offsets(),
            );
            assert_eq!(
                systems.coms_sabotaged, sabotaged,
                "{completed} consoles complete"
            );
        }
    }

    #[test]
    fn miras_decontamination_doors_are_closed_doors() {
        let mut layout = Layout::new();
        with_system(&mut layout, 18);
        // Lower shut, upper open.
        layout.at(SYSTEM + 0x10, 0);
        layout.at(SYSTEM + 0x14, 1);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            0,
            MapType::MiraHq,
            None,
            &offsets(),
        );
        assert_eq!(systems.closed_doors, vec![0]);
    }

    #[test]
    fn reads_the_door_table_and_reports_the_shut_ones() {
        let mut layout = Layout::new();
        layout.at(SHIP + 0x40, u32::try_from(DOORS).unwrap());
        layout.at(DOORS + 0x8, 3);
        for (index, open) in [1u32, 0, 1].into_iter().enumerate() {
            let index = u64::try_from(index).unwrap();
            let door = DOORS + 0x100 + index * 0x20;
            layout.at(DOORS + 0x10 + index * 4, u32::try_from(door).unwrap());
            layout.at(door + 0x14, open);
        }

        let systems = read_systems(&layout.process(), SHIP, 0, MapType::Polus, None, &offsets());
        assert_eq!(systems.closed_doors, vec![1]);
    }

    #[test]
    fn mira_has_no_door_table() {
        // Its doors come from the decontamination system, and reading the table anyway
        // would report doors that do not exist.
        let mut layout = Layout::new();
        layout.at(SHIP + 0x40, u32::try_from(DOORS).unwrap());
        layout.at(DOORS + 0x8, 4);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            0,
            MapType::MiraHq,
            None,
            &offsets(),
        );
        assert!(systems.closed_doors.is_empty());
    }

    #[test]
    fn a_door_that_cannot_be_read_counts_as_shut() {
        // Erring towards blocking audio rather than leaking a voice through a wall.
        let mut layout = Layout::new();
        layout.at(SHIP + 0x40, u32::try_from(DOORS).unwrap());
        layout.at(DOORS + 0x8, 1);
        // The slot points outside the mapped region.
        layout.at(DOORS + 0x10, 0xffff_0000);

        let systems = read_systems(&layout.process(), SHIP, 0, MapType::Polus, None, &offsets());
        assert_eq!(systems.closed_doors, vec![0]);
    }

    #[test]
    fn no_camera_without_an_open_minigame() {
        // The default is NONE, and NONE is 7 rather than 0 — zero is the engine room.
        let systems = read_systems(
            &Layout::new().process(),
            SHIP,
            MINIGAME,
            MapType::Polus,
            Some((0.0, 0.0)),
            &offsets(),
        );
        assert_eq!(systems.current_camera, CAMERA_NONE);
    }

    #[test]
    fn reads_the_camera_index_on_polus() {
        let mut layout = Layout::new();
        layout.at(MINIGAME + 0x4, 0xdead_beef);
        layout.at(MINIGAME + 0x20, 3);
        layout.at(MINIGAME + 0x24, 6);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            MINIGAME,
            MapType::Polus,
            Some((0.0, 0.0)),
            &offsets(),
        );
        assert_eq!(systems.current_camera, 3);
    }

    #[test]
    fn a_wrong_camera_count_is_a_stale_minigame() {
        // The count is what says the minigame is the one being looked at rather than a
        // leftover, whose index would otherwise be believed.
        let mut layout = Layout::new();
        layout.at(MINIGAME + 0x4, 0xdead_beef);
        layout.at(MINIGAME + 0x20, 3);
        layout.at(MINIGAME + 0x24, 2);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            MINIGAME,
            MapType::Polus,
            Some((0.0, 0.0)),
            &offsets(),
        );
        assert_eq!(systems.current_camera, CAMERA_NONE);
    }

    #[test]
    fn the_skeld_decides_by_where_the_player_is_standing() {
        // Its minigame carries no camera index, so proximity to the one console is the
        // test — and a player elsewhere on the map is not at it.
        let mut layout = Layout::new();
        layout.at(MINIGAME + 0x4, 0xdead_beef);
        layout.at(MINIGAME + 0x28, 4);
        let process = layout.process();

        let at_console = read_systems(
            &process,
            SHIP,
            MINIGAME,
            MapType::TheSkeld,
            Some((-12.9364, -2.7928)),
            &offsets(),
        );
        assert_eq!(at_console.current_camera, CAMERA_SKELD);

        let elsewhere = read_systems(
            &process,
            SHIP,
            MINIGAME,
            MapType::TheSkeld,
            Some((0.0, 0.0)),
            &offsets(),
        );
        assert_eq!(elsewhere.current_camera, CAMERA_NONE);
    }

    #[test]
    fn a_bundle_without_the_offsets_reads_nothing_rather_than_failing() {
        // An older bundle may not carry these fields. Skipping one reading is the
        // behaviour; refusing the frame would cost every other field with it.
        let mut layout = Layout::new();
        with_system(&mut layout, 14);
        layout.at(SYSTEM + 0x8, 1);

        let systems = read_systems(
            &layout.process(),
            SHIP,
            MINIGAME,
            MapType::TheSkeld,
            Some((0.0, 0.0)),
            &SystemOffsets::default(),
        );
        assert_eq!(systems, Systems::default());
    }
}
