# Making a release

Everything here is one command except the two things a command must not do: keeping a key,
and deciding that a build is good.

## Once, and offline

```bash
.\scripts\new-release-key.ps1 -Into <somewhere safe> -Role operational
.\scripts\new-release-key.ps1 -Into <somewhere else>  -Role recovery
```

The script **generates the passphrase** and shows it once. That is deliberate: a passphrase
somebody invents at a prompt is a passphrase they have used before, and one typed on a
command line is in the shell's history and in every process listing while it runs. This one
comes from the system CSPRNG, 150 bits, in six groups of five from an alphabet with no
character that can be misread for another — it will be read off a phone screen at some
point.

It refuses to put a key anywhere inside this repository, and refuses to write the passphrase
into the same directory as the key. The first is the one that matters: a private key in the
working tree is one `git add -A` away from being public, permanently, and a key that has
been pushed is a key that must be replaced.

For an offline ceremony, build the tool while you still have a network and then pass it in:

```bash
cargo build --locked --release --features ceremony -p acl-updater --bin acl-release
# ...disconnect...
.\scripts\new-release-key.ps1 -Into E:\keys -Role operational -ToolPath .\target\release\acl-release.exe
```

Or without the script, if you would rather choose the passphrase yourself:

```bash
ACL_RELEASE_KEY_PASSWORD='...' \
  cargo run -p acl-updater --features ceremony --bin acl-release -- keys --into <somewhere safe>
```

**The key is encrypted, and the passphrase is required** — decided 2026-08-27. There is no
unencrypted mode: a flag for one is a flag somebody reaches for on the day the passphrase
is inconvenient, and the file it produces looks identical from the outside. Without the
variable the tool refuses and writes nothing.

The passphrase comes from the environment and never from an argument, because a command
line is in the shell's history and in every process listing while it runs. In an
interactive shell, prefix the command with a space if your shell is set to skip those from
history — or read it from a password manager rather than typing it at all.

**Keep the passphrase somewhere the key file is not.** A passphrase stored beside the key
it protects is not a second factor; it is a longer key. The whole value of this choice is
that copying the file gets nobody a signing key.

The tool prints the public key ready to paste into `acl_updater::manifest::PUBLIC_KEYS`. It
opens the secret half again with the passphrase before reporting success — a key that
cannot be decrypted is otherwise discovered at the first release, by which point the
ceremony machine may be gone. And it refuses to overwrite an existing key: silently
replacing one would retire every client that trusts the old one, at the next release, with
no step in between where anybody could notice.

**Two keys, and §4.9 says why.** Run it twice, into two places. One signs releases; the
other never touches a workflow and is what the project recovers with if the first is lost
or stolen. A client that trusts both can be handed a manifest signed by the second without
an update having to reach it first — which is impossible when the first is the one that has
gone.

Keep both private halves off the release machine. That is not something this tool can do
for you: it is a property of where the file is, and it is the whole of what protects the
update path.

Both public halves go into `manifest::PUBLIC_KEYS`. `there_are_two_keys` fails on one as
well as on none: one key is the state where somebody performed half the ceremony and moved
on, and reading the file does not distinguish it from the finished state.

Then check that the build agrees, which is a different question from the one `sign` answers:

```bash
ACL_RELEASE_KEY_PASSWORD='...'   cargo run -p acl-updater --features ceremony --bin acl-release -- check --key <key>/release.key
```

`sign` checks a signature against a public key *file* — that the two halves on your disk
agree. `check` signs a throwaway manifest and verifies it with the client's own verifier
against the list compiled into what people run. **Do this for the recovery key especially.**
A wrong entry for the operational key is found at the next release; a wrong entry for the
recovery key is found on the day the operational one is gone, and on that day there is no
second chance and no way to send a fix — sending one would mean shipping an update signed by
the key that has gone.

## Per release

```bash
# 1. Build and package. `rust-release.yml` does this on a tag.
makensis -DVERSION=2.0.0 -DSOURCE_DIR=../target/release installer/anothercrewlink.nsi

# 2. Describe the artefact. The digest and the size are read off the file, never typed.
cargo run -p acl-updater --features ceremony --bin acl-release -- write \
  --version 2.0.0 \
  --url https://github.com/greluc/AnotherCrewLink/releases/download/v2.0.0/AnotherCrewLink-Setup-2.0.0.exe \
  --artefact installer/AnotherCrewLink-Setup-2.0.0.exe \
  --into release.json

# 3. Sign it, and check the signature against the key the *fleet* has.
ACL_RELEASE_KEY_PASSWORD='...' \
  cargo run -p acl-updater --features ceremony --bin acl-release -- sign \
  --manifest release.json --key <somewhere safe>/release.key --public <somewhere safe>/release.pub
```

Publish `release.json` and `release.json.minisig` side by side. The updater derives the
second name from the first rather than accepting one, so a feed cannot point at a manifest
in one place and a signature somewhere else.

## Releasing 1.0.6, which comes first

§4.9: "Prove the new NSIS script by shipping an **ordinary 1.0.x release** with it, so its
CLI contract is tested against real 1.x updaters before it carries anything important."
Decided 2026-08-27; 1.0.6 is that release, and `CHANGELOG.md` already carries its notes.

`.github/workflows/release.yml` does all of it — `electron-builder --dir`, then
`installer/legacy.nsi`, then a silent install and uninstall of what it produced, then
`latest.yml`, then a **draft** release. Run it from the Actions tab.

The manual equivalent, if you would rather watch it happen:

```bash
npm run compile && npx electron-builder --win --x64 --dir
makensis -DVERSION=1.0.6 -DSOURCE_DIR=../dist/win-unpacked installer/legacy.nsi

# latest.yml, which electron-builder used to write and no longer does. The digest and the
# size are read off the artefact; the date is passed in so two builds of one commit agree.
cargo run -p acl-updater --features ceremony --bin acl-release -- feed \
  --version 1.0.6 \
  --artefact installer/AnotherCrewLink-Setup-1.0.6.exe \
  --released "$(git log -1 --format=%cI)" \
  --into latest.yml
```

**Publishing the draft is what moves the fleet.** Every 1.x install polls for `latest.yml`
and runs whatever version it names. Nothing before that step is visible to anybody, and
nothing after it is reversible — a release can be deleted, but not un-downloaded. So the
workflow drafts and stops, and a person presses the button.

`acl-release feed` refuses a 2.x version, because a 1.x feed announcing 2.0.0 migrates the
entire installed base the moment it goes up. That is §4.12's bridge, which announces itself
as 1.1.0 and has a staged rollout; it is not something to reach by mistyping a version here.

## What is still yours

**The keys, and the passphrase.** Nothing can do that step for you, and nothing should:
what protects the key is where it is kept and where the passphrase is not.

**The three manual checks in `installer/README.md`**, on a machine, once. CI compiles all
three scripts and runs an install and uninstall on a clean runner, which catches a script
NSIS will not accept. It cannot catch a machine with an installation already on it.

**Whether the build is good.** Both release workflows draft rather than publish, because a
release that publishes itself is one nobody looked at.
