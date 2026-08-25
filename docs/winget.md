# The WinGet package

`winget install greluc.AnotherCrewLink` fetches the Windows installer from the GitHub
release and verifies its SHA256 against the manifest before running it. That is the
checksum comparison `README.md` otherwise asks a user to make by hand, done by the
package manager instead.

It does **not** make the installer signed, and it is not a route around SmartScreen or
Smart App Control. Those are a code-signing question and
`docs/rust-port/09-technology-migration.md` D9 is where that decision lives. What the
package changes is the download-and-verify step, and nothing after it.

Everything below was checked against the current upstream documents rather than
remembered; each section says which one.

## What is automated and what is not

`.github/workflows/winget.yml` submits every published release. It runs on the `released`
event, so a draft that electron-builder uploaded reaches winget only once the draft is
published, and it can be re-run by hand with `workflow_dispatch` against a tag — which is
what to do when a submission is rejected and needs resending.

**The first version of a new identifier cannot go through it.** komac refuses an
identifier with no version in `microsoft/winget-pkgs`, and the workflow inherits the
refusal:

```
Package greluc.AnotherCrewLink does not exist in the winget-pkgs repository.
```

So the first submission is manual, once. Everything after it is automatic.

## One-time setup

### 1. A fork of winget-pkgs

Fork [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) to the `greluc`
account — the same account that owns this repository, which is what the workflow assumes
when it passes `KOMAC_FORK_OWNER: ${{ github.repository_owner }}`.

The fork has to stay current with upstream or komac branches from a stale tree. komac has
a command for it:

```bash
komac sync-fork
```

### 2. A token

A **classic** personal access token with the `public_repo` scope, stored as the
`WINGET_TOKEN` repository secret. Two upstream documents say so independently: komac's
own README under *GitHub Token Setup*, and `vedantmgoyal9/winget-releaser`'s README, which
adds that fine-grained tokens are not supported
([issue 172](https://github.com/vedantmgoyal9/winget-releaser/issues/172)).

The workflow's own `GITHUB_TOKEN` cannot stand in. It is scoped to this repository and
cannot reach `winget-pkgs` or the fork.

### 3. Generate the manifests, but do not submit yet

komac is the same tool the workflow uses. Its `new` command takes the **identifier** as
its positional argument — not a URL — and every metadata field it would otherwise prompt
for can be passed as a flag, so the whole thing is one non-interactive command. Write the
manifests to a directory first rather than submitting, because Microsoft's checklist wants
them validated and test-installed locally before the pull request exists:

```bash
cargo install komac --locked --version 2.16.0

komac new greluc.AnotherCrewLink \
  --version 1.0.5 \
  --urls https://github.com/greluc/AnotherCrewLink/releases/download/v1.0.5/AnotherCrewLink-Setup-1.0.5.exe \
  --package-locale en-US \
  --publisher "Lucas Greuloch" \
  --publisher-url https://github.com/greluc \
  --publisher-support-url https://github.com/greluc/AnotherCrewLink/issues \
  --package-name AnotherCrewLink \
  --package-url https://github.com/greluc/AnotherCrewLink \
  --moniker anothercrewlink \
  --license GPL-3.0-or-later \
  --license-url https://github.com/greluc/AnotherCrewLink/blob/nightly/LICENSE \
  --short-description "Free, open proximity voice chat for Among Us" \
  --release-notes-url https://github.com/greluc/AnotherCrewLink/releases/tag/v1.0.5 \
  --output ./winget-manifests
```

komac downloads the installer and derives what it can from the binary itself — installer
type, product code, the Apps and Features entry, the SHA256. `komac new` has no `--tags`
flag, so tags are the one field to add by hand afterwards if they are wanted.

The identifier is `greluc.AnotherCrewLink` rather than
`AnotherCrewLink.AnotherCrewLink`: `manifests/g/greluc/` was free when this was written,
it matches the GitHub account, and it matches the `me.greluc.anothercrewlink` application
id the installer already registers. It is **case-sensitive** and cannot be changed later
without orphaning everyone who installed under the old one, which is why `winget.yml`
hard-codes exactly this string.

### 4. Work through Microsoft's pre-submit checklist

From [`doc/FirstContribution.md`](https://github.com/microsoft/winget-pkgs/blob/master/doc/FirstContribution.md).
The parts that bite:

```powershell
winget validate --manifest .\winget-manifests
winget install --manifest .\winget-manifests
```

- One `PackageIdentifier` and `PackageVersion` per pull request.
- **Manifest files only.** Spelling files, `README.md`, tooling — separate PR.
- A multi-file manifest set; singleton manifests are not accepted.
- The `# yaml-language-server: $schema=...` header on every file.
- The latest schema the repository supports. The list lives at
  [`doc/manifest/README.md`](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/README.md)
  and was up to 1.28.0 when this was written. komac emits whatever its `winget-types`
  dependency targets — which version that is has not been verified here, so if validation
  rejects the schema version, the fix is a newer komac and a matching bump to the pinned
  version in `winget.yml`.
- Testing in Windows Sandbox with `Tools\SandboxTest.ps1` if you can.

Then submit — either `komac new … --submit` with the same arguments, or a pull request
from the fork by hand.

## The risk that actually applies to this package

From [`doc/Policies.md`](https://github.com/microsoft/winget-pkgs/blob/master/doc/Policies.md),
*Security Scans and Potentially Unwanted Applications*:

> If a package is flagged by any of the security scans in the validation pipeline, it
> cannot be accepted into the repository, regardless of the application's legitimacy or
> intent.

Validation step 07 runs static analysis, multiple antivirus engines, and Microsoft's PUA
criteria against the installer itself. This app reads another process's memory and
installs a global keyboard hook — the profile heuristics are built to catch. BetterCrewLink
tells its own users to expect antivirus warnings for exactly this reason, and they are in
the repository, so it is passable rather than hopeless.

If it is flagged, the documented route is: submit the installer to
[Microsoft Defender for Business analysis](https://www.microsoft.com/wdsi/filesubmission)
as a potential false positive, **include the pull request URL in the submission**, and once
it is resolved a moderator re-triggers validation by commenting `@wingetbot run`. That is
from [`doc/Validation.md`](https://github.com/microsoft/winget-pkgs/blob/master/doc/Validation.md);
do not close and resubmit the PR instead.

### 5. Prove the automated path

Once the package exists upstream, run `winget.yml` by hand against `v1.0.5` before relying
on it at the next release.

## When a later submission fails

Re-run `winget.yml` through `workflow_dispatch` with the tag. Two failures are worth
recognising on sight:

- **`WINGET_TOKEN is not set`** — the secret is missing or expired. Classic PATs expire;
  this is what that looks like.
- **`expected exactly one Windows installer on <tag>, found 0`** — no asset matches
  `AnotherCrewLink-Setup-*.exe`. Either the release is still a draft, or the installer
  filename changed and the pattern in `winget.yml` has to change with it.

Validation failures on the winget-pkgs side are labelled on the pull request; the
[Validation Failure Guide](https://github.com/microsoft/winget-pkgs/blob/master/doc/ValidationFailureGuide.md)
maps each label to a cause.
