# The installer

`anothercrewlink.nsi` builds `AnotherCrewLink-Setup-<version>.exe`, which is the one
artefact this project publishes and the one `electron-updater` on the installed 1.x fleet
knows how to run.

```bash
makensis -DVERSION=2.0.0 -DSOURCE_DIR=../target/release installer/anothercrewlink.nsi
```

## What is checked automatically, and what is not

Two things, which answer different questions.

`crates/acl-updater/tests/installer_contract.rs` reads this script as text and fails if it
stops claiming any of the things that would strand the fleet: `--updated`, `/S`, `/D=`, the
artefact's name, and — since 2026-08-26 — the architecture guard and the numeric version.
It also checks that the directory name agrees with `acl_core::paths::APP_DIRECTORY`, because
the installer decides where the program goes and the client decides where its settings go.

`rust.yml`'s `installer` job compiles both scripts with `makensis` on every push and then
**runs** what it produced: a silent install with the exact command line 1.x's updater
spawns, a check that the binaries and the locale tree landed, and a silent uninstall. It
found two real defects in its first three runs — a message string NSIS would not parse, and
`VIProductVersion` aborting on a prerelease tag, which would have stopped the first staging
release. Neither was visible to a text check, because every word it looks for was present.

**Neither is a person using it.** Before a release that matters, once, on a machine:

1. Install into a fresh user profile. The window opens, the settings page opens, and
   `%APPDATA%\ACL` appears.
2. Install over it with `--updated /S /D=<the same directory>`. No window opens, the version
   in Add/Remove Programs changes, and `%APPDATA%\ACL\config.json` still has whatever was
   set in step 1.
3. Uninstall. `%LOCALAPPDATA%\Programs\ACL` is gone and `%APPDATA%\ACL` is not.

### If you run the installer from Git Bash

Write `//S`, not `/S`. Git Bash rewrites an argument that looks like a POSIX path into a
Windows one before the program sees it, so `/S` arrives as a path, the installer is not
silent, and it opens a window and waits. `/D=` and `_?=` survive as they are, because an
argument containing `=` is left alone — which is why the files land in the right place and
the whole thing looks like a script that got to the end and hung. This cost five CI runs.

§4.9's own instruction is stronger than any of this and is the real test: *"Prove the new
NSIS script by shipping an **ordinary 1.0.x release** with it, so its CLI contract is tested
against real 1.x updaters before it carries anything important."*

## It is unsigned

§4.9 item 2. Every user sees the unknown-publisher warning on every install, exactly as they
do with 1.x today. SmartScreen reputation is never accumulated, so the warning stays
forever rather than fading; enterprise allow-listing by publisher is unavailable. What
protects the *update* path instead is the minisign signature over the manifest
(`acl_updater::manifest`), and what protects a first download is TLS and nothing else.
