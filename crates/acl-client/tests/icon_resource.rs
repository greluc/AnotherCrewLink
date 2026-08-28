#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! The icon is in the executable that was linked, not just in the build script that meant to
//! put it there.
//!
//! `build.rs` panics when the resource compiler is missing, so the loud failure is covered.
//! What is not is the quiet one: a `.res` that compiles and never reaches the linker, a
//! resource compiler that produces an empty file, a path that stops resolving after somebody
//! moves a directory. Every one of those leaves a build that succeeds and an executable with
//! a blank icon, which is the state this whole change started from and which nobody reports
//! as a bug.
//!
//! So this reads the resource directory back out of the binary on disk. It is the artefact
//! that ships, and it either has the icon in it or it does not.

use std::path::{Path, PathBuf};

/// The resource types this looks for. The numbers are Windows', from `winuser.h`.
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_VERSION: u32 = 16;

/// What a PE file's resource directory holds.
///
/// A deliberately small reader: it walks the optional header to the resource data directory,
/// maps that address back to a file offset through the section table, and reads one level of
/// the resource tree. Anything it cannot follow is `None` rather than a panic, so a failure
/// here reads as "this file has no resources" and the assertion below says which file.
fn resources(path: &Path) -> Option<Resources> {
    let file = std::fs::read(path).ok()?;
    let at = |offset: usize, len: usize| -> Option<&[u8]> { file.get(offset..offset + len) };
    let u16_at = |offset: usize| -> Option<u16> {
        Some(u16::from_le_bytes(at(offset, 2)?.try_into().ok()?))
    };
    let u32_at = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(at(offset, 4)?.try_into().ok()?))
    };

    let header = u32_at(0x3c)? as usize;
    if at(header, 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(header + 6)? as usize;
    let optional = u16_at(header + 20)? as usize;
    // 0x20b is PE32+, whose optional header is sixteen bytes longer before the directories.
    let directories = header
        + 24
        + if u16_at(header + 24)? == 0x20b {
            112
        } else {
            96
        };
    // Entry two of the data directory is the resources.
    let address = u32_at(directories + 16)?;
    if address == 0 {
        return None;
    }

    // The address is virtual; the bytes are at a file offset. The section that contains it
    // carries the difference between the two.
    let table = header + 24 + optional;
    let mut section = None;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8)?;
        let virtual_address = u32_at(entry + 12)?;
        let raw = u32_at(entry + 20)?;
        if virtual_address <= address && address < virtual_address + virtual_size.max(1) {
            section = Some((virtual_address, raw));
        }
    }
    let (virtual_base, raw) = section?;
    let root = (raw + (address - virtual_base)) as usize;

    // Each level of the tree has the same header: two counts, then that many eight-byte
    // entries of (name, offset).
    let children = |node: usize| -> Option<Vec<(u32, u32)>> {
        let named = u16_at(node + 12)? as usize;
        let numbered = u16_at(node + 14)? as usize;
        (0..named + numbered)
            .map(|index| {
                let entry = node + 16 + index * 8;
                Some((u32_at(entry)?, u32_at(entry + 4)?))
            })
            .collect()
    };

    let mut found = Resources::default();
    for (name, child) in children(root)? {
        // The high bit means the type has a string name rather than one of Windows'
        // numbers, and none of the three this looks for does.
        if name & 0x8000_0000 != 0 {
            continue;
        }
        let by_id = root + (child & 0x7fff_ffff) as usize;
        let ids = children(by_id)?;
        found.counts.insert(name, ids.len());
        if name != RT_ICON {
            continue;
        }
        // Three levels: type, then id, then language. The leaf is not another directory --
        // it is an address and a length.
        for (_, id) in ids {
            for (_, language) in children(root + (id & 0x7fff_ffff) as usize)? {
                let leaf = root + (language & 0x7fff_ffff) as usize;
                let leaf_address = u32_at(leaf)?;
                let length = u32_at(leaf + 4)? as usize;
                let start = (raw + (leaf_address - virtual_base)) as usize;
                found.icons.push(at(start, length)?.to_vec());
            }
        }
    }
    Some(found)
}

/// What one PE file's resource directory holds.
#[derive(Default)]
struct Resources {
    /// How many of each type, keyed by Windows' type number.
    counts: std::collections::BTreeMap<u32, usize>,
    /// The image bytes of every `RT_ICON`, in the order the directory lists them.
    icons: Vec<Vec<u8>>,
}

/// Every image inside an `.ico` file, in the order its directory lists them.
///
/// The container is a six-byte header, one sixteen-byte entry per image, then the images.
/// Only the last two fields of an entry are needed here: how long each image is and where.
fn ico_images(path: &Path) -> Vec<Vec<u8>> {
    let file = std::fs::read(path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
    let count = u16::from_le_bytes([file[4], file[5]]) as usize;
    (0..count)
        .map(|index| {
            let entry = 6 + index * 16;
            let length =
                u32::from_le_bytes(file[entry + 8..entry + 12].try_into().unwrap()) as usize;
            let at = u32::from_le_bytes(file[entry + 12..entry + 16].try_into().unwrap()) as usize;
            file[at..at + length].to_vec()
        })
        .collect()
}

fn check(path: &Path) {
    let found = resources(path)
        .unwrap_or_else(|| panic!("{} carries no resource directory at all", path.display()));
    let found_counts = &found.counts;

    // Six, because `assets/icon.ico` holds six sizes -- 16 up to 256 -- and Windows picks per
    // place it draws them. Each is drawn from the vector at its own size rather than scaled
    // down from the largest, which is the difference between a 16-pixel icon and a smudge.
    assert_eq!(
        found_counts.get(&RT_ICON).copied().unwrap_or(0),
        6,
        "{} should carry all six sizes out of the .ico, and carries {:?}",
        path.display(),
        found_counts.get(&RT_ICON)
    );
    // The group is what Windows actually asks for; the images on their own are not an icon.
    assert_eq!(
        found_counts.get(&RT_GROUP_ICON).copied().unwrap_or(0),
        1,
        "{} has icon images and no group to select them by",
        path.display()
    );
    assert_eq!(
        found_counts.get(&RT_VERSION).copied().unwrap_or(0),
        1,
        "{} has no version block, so its Details tab is empty",
        path.display()
    );

    // And it is *this* icon. Counting them says the build script ran; comparing the bytes
    // says which artwork it ran with, and that is the check that was missing when all three
    // of these shipped carrying BetterCrewLink's logo -- inherited through the fork, a
    // perfectly valid icon, and the wrong one. It also catches the duller version of the
    // same thing: a binary left over from before the artwork changed.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon.ico");
    let wanted: std::collections::BTreeSet<Vec<u8>> = ico_images(&source).into_iter().collect();
    let carried: std::collections::BTreeSet<Vec<u8>> = found.icons.into_iter().collect();
    assert_eq!(
        carried,
        wanted,
        "{} does not carry the images in assets/icon.ico",
        path.display()
    );
}

#[test]
fn the_client_carries_its_icon() {
    check(Path::new(env!("CARGO_BIN_EXE_anothercrewlink")));
}

/// The other two, when a build has produced them.
///
/// `CARGO_BIN_EXE_` only names this package's binaries, and `cargo test -p acl-client` does
/// not build the other crates. So they are checked when they are there and named when they
/// are not, rather than being asserted about and failing for having not been built. A
/// workspace run has all three.
#[test]
fn the_helper_and_the_updater_carry_theirs() {
    let beside = PathBuf::from(env!("CARGO_BIN_EXE_anothercrewlink"))
        .parent()
        .expect("a binary is in a directory")
        .to_path_buf();

    let mut checked = Vec::new();
    for name in ["acl-helper.exe", "acl-updater.exe"] {
        let path = beside.join(name);
        if path.exists() {
            check(&path);
            checked.push(name);
        }
    }
    println!("checked beside the client: {checked:?}");
}
