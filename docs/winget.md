# The WinGet package

`winget install greluc.AnotherCrewLink` fetches the Windows installer from the GitHub
release and verifies its SHA256 against the manifest before running it. That is the
checksum comparison `README.md` otherwise asks a user to do by hand, done by the package
manager instead.

It does **not** make the installer signed, and it is not a way around SmartScreen or
Smart App Control. Those are a code-signing question and `README.md` says where that
stands. What the package changes is the download-and-verify step, nothing after it.

## What is automated and what is not

`.github/workflows/winget.yml` submits every published release. It runs on the `released`
event, so a draft that electron-builder uploaded only reaches winget when the draft is
published — and it can be re-run by hand with `workflow_dispatch` against a tag, which is
what to do when a submission is rejected and needs resending.

**The first version of a new identifier cannot go through it.** komac refuses an
identifier that has no version in `microsoft/winget-pkgs` yet, and the workflow inherits
that refusal:

```
Package greluc.AnotherCrewLink does not exist in the winget-pkgs repository.
```

So the first submission is manual, once, and everything after it is automatic.

## One-time setup

### 1. A fork of winget-pkgs

Fork [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) to the `greluc`
account. komac pushes the manifest branch there and opens the pull request from it. The
fork has to stay reasonably current with upstream or the branch komac creates will be
based on a stale tree.

### 2. A token

A **classic** personal access token with the `public_repo` scope, stored as the
`WINGET_TOKEN` repository secret. Fine-grained tokens do not work for this: they cannot
open a pull request against a repository the account has no access to.

The workflow's own `GITHUB_TOKEN` cannot be used. It is scoped to this repository and
cannot touch `winget-pkgs` or the fork.

### 3. The first manifest

komac is the same tool the workflow uses, so use it here too rather than writing the
three YAML files by hand:

```bash
cargo install komac --locked --version 2.16.0
komac new https://github.com/greluc/AnotherCrewLink/releases/download/v1.0.5/AnotherCrewLink-Setup-1.0.5.exe
```

It downloads the installer, works out the installer type, product code and Apps and
Features entry, and prompts for the metadata it cannot derive. The values to give it:

| Field | Value |
| --- | --- |
| PackageIdentifier | `greluc.AnotherCrewLink` |
| PackageName | `AnotherCrewLink` |
| Publisher | `Lucas Greuloch` |
| PublisherUrl | `https://github.com/greluc` |
| PublisherSupportUrl | `https://github.com/greluc/AnotherCrewLink/issues` |
| PackageUrl | `https://github.com/greluc/AnotherCrewLink` |
| License | `GPL-3.0-or-later` |
| LicenseUrl | `https://github.com/greluc/AnotherCrewLink/blob/nightly/LICENSE` |
| ShortDescription | `Free, open proximity voice chat for Among Us` |
| Moniker | `anothercrewlink` |

The identifier is `greluc.AnotherCrewLink` and not `AnotherCrewLink.AnotherCrewLink`:
`manifests/g/greluc/` was free, it matches the GitHub account, and it matches the
`me.greluc.anothercrewlink` application id the installer already registers. It is
**case-sensitive** and cannot be changed later without orphaning everyone who installed
under the old one, so the workflow hard-codes exactly this string.

Then submit, and expect Microsoft's validation to run on the pull request. A first
submission is reviewed by a human as well as by their CI, and a package that reads
another process's memory may draw questions — answer them rather than resubmitting.

### 4. Check the workflow

Once the package exists upstream, run the workflow by hand against `v1.0.5` to prove the
automated path works end to end, before relying on it at the next release.

## When a submission fails

Re-run `winget.yml` through `workflow_dispatch` with the tag. It is idempotent from this
side: komac opens a new pull request, and a superseded one can be closed on the fork.

The two failures worth recognising:

- **`WINGET_TOKEN is not set`** — the secret is missing or expired. Classic PATs expire;
  this is what it looks like when one does.
- **`expected exactly one Windows installer on <tag>, found 0`** — the release has no
  asset matching `AnotherCrewLink-Setup-*.exe`. Either the release is still a draft, or
  the installer filename changed and the pattern in `winget.yml` has to change with it.
