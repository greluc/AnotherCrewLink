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
//! It matters most here of the three. This binary asks for elevation, and the UAC prompt
//! shows the icon of the program doing the asking -- so an iconless executable requesting
//! administrator rights looks exactly like the thing people are told to refuse.
//!
//! # This will stop a build that has no resource compiler, and that is deliberate
//!
//! `winresource` needs `rc.exe`, which arrives with Visual Studio's "Desktop development
//! with C++" workload — the same requirement `CLAUDE.md` already states for building this
//! project at all, and the runners have it. Skipping quietly on a machine without it is how
//! an iconless binary would get released without anybody noticing, which is exactly the
//! failure this file exists to end.

fn main() {
    // The icon lives beside the tree rather than in this crate: `resources/icon.ico` is
    // what the 1.x installer ships, and two clients meant to look like the same program
    // should not have two files to keep in step. `acl_ui::icon` reaches for the PNG next to
    // it for the same reason.
    println!("cargo:rerun-if-changed=../../resources/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../resources/icon.ico");
    // What the Details tab shows. `ProductName` is the name the product has had since 1.0
    // rather than the crate's, which is `acl-client` and means nothing to anybody reading a
    // file's properties.
    resource.set("ProductName", "AnotherCrewLink");
    resource.set("FileDescription", "AnotherCrewLink game reader");
    resource.set("LegalCopyright", "Lucas Greuloch. GPL-3.0-or-later.");

    if let Err(why) = resource.compile() {
        panic!(
            "could not put the icon into the executable: {why}\n\
             This needs rc.exe, from Visual Studio's \"Desktop development with C++\" \
             workload -- the same toolchain the rest of this build needs."
        );
    }
}
