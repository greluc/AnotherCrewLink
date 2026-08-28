//! Puts the program's icon and its version details into the executable.
//!
//! The third of the three places Windows looks for an icon, and the one `acl_ui::icon`
//! cannot reach: a window icon is set while the program runs, and Explorer, a pinned
//! shortcut, a UAC prompt and Add/Remove Programs all read the executable instead. The
//! installer already points `DisplayIcon` at this binary, so without this it names a file
//! with nothing to show.
//!
//! It comes with the version block, because the same resource section carries both and an
//! executable with no Details tab looks as unfinished as one with no icon.
//!
//! This one runs while the client is closed and replaces its files, so for a minute it is
//! the only `AnotherCrewLink` on the taskbar -- and the only thing the person watching it
//! has to go on that the right program is doing the replacing.
//!
//! # This will stop a build that has no resource compiler, and that is deliberate
//!
//! `winresource` needs `rc.exe`, which arrives with Visual Studio's "Desktop development
//! with C++" workload — the same requirement `CLAUDE.md` already states for building this
//! project at all, and the runners have it. Skipping quietly on a machine without it is how
//! an iconless binary would get released without anybody noticing, which is exactly the
//! failure this file exists to end.

fn main() {
    // The design system's mark, rendered by `scripts/rasterise-icon` from `assets/icon.svg`.
    // Not `resources/icon.ico`, which this used until 2026-08-28 and which is
    // BetterCrewLink's artwork inherited through the fork. `acl_ui::icon` takes the PNG from
    // the same render, so the window and the executable cannot end up showing two things.
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../assets/icon.ico");
    // What the Details tab shows. `ProductName` is the name the product has had since 1.0
    // rather than the crate's, which is `acl-client` and means nothing to anybody reading a
    // file's properties.
    resource.set("ProductName", "AnotherCrewLink");
    resource.set("FileDescription", "AnotherCrewLink updater");
    resource.set("LegalCopyright", "Lucas Greuloch. GPL-3.0-or-later.");

    if let Err(why) = resource.compile() {
        panic!(
            "could not put the icon into the executable: {why}\n\
             This needs rc.exe, from Visual Studio's \"Desktop development with C++\" \
             workload -- the same toolchain the rest of this build needs."
        );
    }
}
