# The installer

`anothercrewlink.nsi` builds `AnotherCrewLink-Setup-<version>.exe`, which is the one
artefact this project publishes and the one `electron-updater` on the installed 1.x fleet
knows how to run.

```bash
makensis -DVERSION=2.0.0 -DSOURCE_DIR=../target/release installer/anothercrewlink.nsi
```

`makensis` is not on the Rust CI runners and is not installed by any job here. The release
workflow installs it for one step; nothing else needs it.

## What is checked automatically, and what is not

`crates/acl-updater/tests/installer_contract.rs` reads this script as text and fails if it
stops claiming any of the four things that would strand the fleet: `--updated`, `/S`, `/D=`,
and the artefact's name. It also checks that the directory name agrees with
`acl_core::paths::APP_DIRECTORY`, because the installer decides where the program goes and
the client decides where its settings go.

**That is a check on the source, not on the installer.** It cannot run `makensis` and it
cannot run the result. Before a release that matters, the following has to be done by a
person, on a machine, once:

1. Install into a fresh user profile. The window opens, the settings page opens, and
   `%APPDATA%\ACL` appears.
2. Install over it with `--updated /S /D=<the same directory>`. No window opens, the version
   in Add/Remove Programs changes, and `%APPDATA%\ACL\config.json` still has whatever was
   set in step 1.
3. Uninstall. `%LOCALAPPDATA%\Programs\ACL` is gone and `%APPDATA%\ACL` is not.

§4.9's own instruction is stronger than any of this and is the real test: *"Prove the new
NSIS script by shipping an **ordinary 1.0.x release** with it, so its CLI contract is tested
against real 1.x updaters before it carries anything important."*

## It is unsigned

§4.9 item 2. Every user sees the unknown-publisher warning on every install, exactly as they
do with 1.0.2 today. SmartScreen reputation is never accumulated, so the warning stays
forever rather than fading; enterprise allow-listing by publisher is unavailable. What
protects the *update* path instead is the minisign signature over the manifest
(`acl_updater::manifest`), and what protects a first download is TLS and nothing else.
