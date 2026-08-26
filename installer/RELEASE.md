# Making a release

Everything here is one command except the two things a command must not do: keeping a key,
and deciding that a build is good.

## Once, and offline

```bash
cargo run -p acl-updater --features ceremony --bin acl-release -- keys --into <somewhere safe>
```

It prints the public key ready to paste into `acl_updater::manifest::PUBLIC_KEYS`, and it
refuses to overwrite an existing key — silently replacing one would retire every client
that trusts the old one, at the next release, with no step in between where anybody could
notice.

**Two keys, and §4.9 says why.** Run it twice, into two places. One signs releases; the
other never touches a workflow and is what the project recovers with if the first is lost
or stolen. A client that trusts both can be handed a manifest signed by the second without
an update having to reach it first — which is impossible when the first is the one that has
gone.

Keep both private halves off the release machine. That is not something this tool can do
for you: it is a property of where the file is, and it is the whole of what protects the
update path.

Until `PUBLIC_KEYS` has something in it, the updater refuses every manifest. That is
deliberate; `no_keys_means_no_updates` is the test.

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
cargo run -p acl-updater --features ceremony --bin acl-release -- sign \
  --manifest release.json --key <somewhere safe>/release.key --public <somewhere safe>/release.pub
```

An encrypted key works too: set `ACL_RELEASE_KEY_PASSWORD`. Not an argument, because a
passphrase on a command line is a passphrase in the shell's history and in every process
listing on the machine while it runs.

Publish `release.json` and `release.json.minisig` side by side. The updater derives the
second name from the first rather than accepting one, so a feed cannot point at a manifest
in one place and a signature somewhere else.

## What is still yours to decide

**Shipping an ordinary 1.0.x release through the new installer first.** §4.9's own
instruction, and it is the only thing that tests the CLI contract against real 1.x
updaters. `crates/acl-updater/tests/installer_contract.rs` checks the script still *claims*
to honour it; nothing here can check that it does.

**The three manual checks in `installer/README.md`**, on a machine, once.

**Whether the build is good.** The release workflow drafts rather than publishes, because a
release that publishes itself is one nobody looked at.
