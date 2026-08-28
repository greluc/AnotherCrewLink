# The release ceremony

> **There is a script for this.** `scripts/release.ps1` performs everything below, asking
> for the tag and the passphrase and nothing else. It stops once, before publishing, because
> that is the irreversible step. What follows is what the script does and why — read it once,
> then use the script.


What a maintainer does by hand when a 2.x version goes out, and why each step is a step.

Everything here needs the private signing key. Nothing in this repository and nothing
reachable from a workflow can sign — §4.9 item 3 — so this is the part no amount of CI
removes.

## Before anything: is the key the one the fleet trusts?

```bash
ACL_RELEASE_KEY_PASSWORD='…' acl-release check --key /path/to/release.key
```

It signs a canned manifest with your key and verifies it against `manifest::PUBLIC_KEYS`,
which is the list compiled into every shipped client. That is the question worth asking:
not "does this key work" but "does this key produce signatures the fleet will accept". A
key that fails here would produce a release nobody can install, and the failure would be
discovered by the fleet at the moment they were meant to be updating.

Run it before every ceremony. It costs a second.

The passphrase comes from the environment in every command that needs one, never from an
argument: on a command line it is in the shell's history and in every process listing while
it runs.

## 1. Tag, and let CI build

```bash
git tag v2.0.0-alpha.3
git push origin v2.0.0-alpha.3
```

`rust-release.yml` then:

- checks the tag and `Cargo.toml` agree, and stops if they do not;
- builds `anothercrewlink.exe`, `acl-helper.exe` and `acl-updater.exe`;
- rebuilds them with `cargo auditable`, so each carries its own dependency list;
- packages the NSIS installer;
- writes `manifest.json` with `acl-release write` and uploads it as an artefact;
- creates the GitHub release **as a draft**, with the notes taken out of `CHANGELOG.md`.

The notes come from the changelog at creation rather than being edited in afterwards.
`gh release edit` clears the draft flag as a side effect, and that published 1.0.6
mid-sentence on 2026-08-27.

## 2. Fetch the manifest and the installer

```bash
gh run download --name "manifest-2.0.0-alpha.3"
gh release download v2.0.0-alpha.3 --pattern '*.exe'
```

The manifest is four fields: the version, the URL the installer will have, its SHA-512 and
its size. CI wrote it from the installer it had just built, so the digest describes the
bytes that exist rather than bytes somebody typed.

## 3. Sign it

```bash
ACL_RELEASE_KEY_PASSWORD='…' acl-release sign \
  --manifest manifest.json \
  --key /path/to/release.key \
  --public /path/to/release.key.pub
```

This writes `manifest.json.minisig` beside it and then **verifies its own work** against the
public key you gave it. Give the public half explicitly: derived from the private one, the
check would verify the signature against the key that made it, which cannot fail and
therefore checks nothing. Given, it checks against the key the fleet uses.

## 4. Upload both, side by side, and publish

```bash
gh release upload v2.0.0-alpha.3 manifest.json manifest.json.minisig
gh release edit v2.0.0-alpha.3 --draft=false
```

Both files, always. `fetch::Feed` derives the signature's URL from the manifest's by
appending `.minisig` rather than accepting it separately, so a manifest published without
its signature is one no client can accept — and one that cannot be told to look somewhere
else, which is the point of deriving it.

Releases are immutable from 2026-08-24, so this cannot be corrected afterwards. A mistake
here is a new version, not an edit.

## The 1.x fleet is a different file, and not part of this

`acl-release feed` writes `latest.yml`, which is what an installed **1.x** client polls.
Publishing it is the act that moves the fleet: every 1.x install takes whatever version it
names and runs it.

The command refuses any version whose major is not 1, on purpose. A `latest.yml` announcing
2.0.0 would be taken by every 1.x client at once — that is the migration of §4.12, with
§4.12's blast radius, and it is not something to do by mistyping a version.

So a 2.x alpha does **not** touch `latest.yml`. Alpha testers install it themselves.

## Known gap: the feed does not see a pre-release

The 2.x client fetches its manifest from

```
https://github.com/greluc/AnotherCrewLink/releases/latest/download/manifest.json
```

`releases/latest` is GitHub's **newest release that is neither a draft nor a pre-release**.
Measured on 2026-08-28:

```
releases/latest/download/manifest.json  →  302  →  .../download/v1.0.6/manifest.json
```

Every 2.0.0 alpha is marked as a pre-release, so the URL resolves to the 1.0.6 release,
which has no manifest. The client reads that as "up to date" and offers nothing. Nothing is
broken and nothing is unsafe — the signature is the whole security boundary, and this is
only about which document is found — but the update path does not fire for a pre-release,
which is the phase it would be most useful in.

Three ways out, none chosen yet:

1. **Publish 2.x alphas as ordinary releases.** They would then be what 1.x users see when
   they open the releases page, which is the reason they are pre-releases now.
2. **Have the client ask the API for the newest release including pre-releases.** Safe:
   picking the wrong release cannot install anything, because the signature and then the
   policy still decide. Costs a JSON parse and one more thing the client does at start-up.
3. **Leave it.** Alpha testers update by hand, which is what they do today, and the path
   starts working by itself when 2.0.0 stops being a pre-release.

## Where the keys live

Two of them, from the ceremony on 2026-08-27: an operational key that signs releases, and a
recovery key that never signs one. The recovery key is compiled into clients so that they
already trust it on the day the operational key is lost — the one day it is any use, and the
one day it could not be delivered, because delivering it would mean an update signed by the
key that has gone.

Keep both private halves off any machine a workflow can reach. That is a property of where
they are stored, and nothing in this repository can enforce it.
