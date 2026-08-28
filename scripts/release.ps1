<#
.SYNOPSIS
    Performs the whole 2.x release ceremony: tag, wait, sign, publish.
.DESCRIPTION
    `docs/release-ceremony.md` describes the ceremony as five commands a maintainer runs in
    order. This is those commands, in that order, with the checks between them that a person
    doing it by hand would have to remember.

    You are asked for two things and nothing else: the tag, and the passphrase. Everything
    else is derived or checked.

    AND IT CAN BE RUN AGAIN. Every step asks what is already done before doing it: an
    existing tag on this commit is used rather than refused, a build already finished is
    re-used rather than re-triggered, a file already uploaded to the draft is replaced, and
    a release already published stops the script with nothing to do. So when a step fails
    for a reason that has nothing to do with the release -- a network blip, a tool that
    could not work out which repository it was in -- the answer is to run it again, not to
    cut a new version. That was the answer it used to give, and it was the wrong one: the
    tag is the cheap half, and the build it started is still running.

    THE PASSPHRASE NEVER BECOMES AN ARGUMENT. It is read without echo and handed to
    `acl-release` through the environment, which is what the tool itself insists on: a
    passphrase on a command line is in the shell's history and in every process listing
    while it runs. It is cleared from this process's environment on the way out, including
    when a step fails.

    WHAT IT WILL NOT DO WITHOUT YOU. Publishing is the one irreversible act here — releases
    have been immutable since 2026-08-24, so a published mistake is a new version rather
    than an edit. The script stops before it, prints exactly what is about to become public,
    and waits for one word. `-Yes` skips that, and is for somebody who has already read the
    same output on a rehearsal.
.PARAMETER Tag
    The tag to cut, as `v2.0.0-alpha.3`. Prompted for if absent. Refused unless the version
    in it matches `Cargo.toml`, because the release workflow refuses the same thing three
    minutes later and finding out here costs nothing.
.PARAMETER Key
    The private minisign key. Defaults to `sign-cert\release.key`, which is where the
    ceremony of 2026-08-27 put it and which `.gitignore` keeps out of the history. Prompted
    for only if that is not there.
.PARAMETER Public
    The public half. Defaults to the `.pub` beside the key -- `release.pub` next to
    `release.key`, which is how minisign names a pair.

    A *file*, given rather than derived from the private key's contents, on purpose.
    Deriving it would check the signature against the key that made it, which cannot fail
    and therefore checks nothing; read off disk, it checks against the key the fleet
    actually uses, which is the question whose wrong answer bricks an update for everybody.
.PARAMETER Yes
    Skip the confirmation before publishing. See above.
.PARAMETER WorkDir
    Where the manifest and its signature are downloaded to and signed. Defaults to a fresh
    directory under the system temp. Never inside the repository: a signature in the working
    tree is one `git add -A` away from being committed.
.EXAMPLE
    .\scripts\release.ps1
    Asks for the tag and the passphrase, finds the key itself, then does the rest.
.EXAMPLE
    .\scripts\release.ps1 -Tag v2.0.0-alpha.3
.EXAMPLE
    .\scripts\release.ps1 -Tag v2.0.0-alpha.3 -Key E:\acl\operational.key
    For a key kept off this machine's repository, which is the better place for it.
#>
[CmdletBinding()]
param(
    [string] $Tag,
    [string] $Key,
    [string] $Public,
    [switch] $Yes,
    [string] $WorkDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Step([string] $Text) { Write-Host "`n== $Text" -ForegroundColor Cyan }
function Note([string] $Text) { Write-Host "   $Text" -ForegroundColor DarkGray }
function Fail([string] $Text) { Write-Host "`n!! $Text" -ForegroundColor Red; exit 1 }

# --- where we are -----------------------------------------------------------------------
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repo

foreach ($tool in 'git', 'gh', 'cargo') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { Fail "$tool is not on the path." }
}

# Every `gh` call below is told which repository it is about.
#
# `gh` works most of it out from the git remotes, and mostly succeeds -- but not all of its
# subcommands resolve the same way, and `gh run watch` stopped this script dead with "failed
# to determine base repo" one line after `gh run list` had answered fine. A repository
# derived once from the remote is not a thing that can be half configured.
$originUrl = (git remote get-url origin 2>$null)
if (-not $originUrl) { Fail 'There is no `origin` remote to release from.' }
if ($originUrl -notmatch 'github\.com[:/](?<owner>[^/]+)/(?<name>[^/]+?)(\.git)?/?$') {
    Fail "The origin remote is $originUrl, which is not a GitHub repository this can release to."
}
$slug = "$($Matches.owner)/$($Matches.name)"
Note "Releasing into $slug."

# --- what to cut ------------------------------------------------------------------------
if (-not $Tag) { $Tag = Read-Host 'Tag to release (for example v2.0.0-alpha.3)' }
$Tag = $Tag.Trim()
if ($Tag -notmatch '^v\d+\.\d+\.\d+') { Fail "'$Tag' does not look like a tag. It should start with v and a version." }
$version = $Tag.Substring(1)

$declared = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if ($declared -ne $version) {
    Fail "Cargo.toml says $declared and the tag says $version. The release workflow refuses this too; fix one of them."
}

# The notes come out of CHANGELOG.md at release creation. An entry that is not there yields
# a release with an empty body, and `gh release edit` cannot add one later without clearing
# the draft flag as a side effect -- which is how 1.0.6 published itself mid-sentence.
#
# The tag's own message comes out of the same section, so the two cannot disagree and nobody
# writes the release twice: the heading line, then the section's opening paragraph. Both 2.x
# tags so far are annotated and read that way.
$changelog = @(Get-Content -LiteralPath (Join-Path $repo 'CHANGELOG.md'))
$opens = -1
for ($line = 0; $line -lt $changelog.Count; $line++) {
    if ($changelog[$line] -eq "## v$version") { $opens = $line + 1; break }
}
if ($opens -lt 0) { Fail "CHANGELOG.md has no '## v$version' section. The release would go out with no notes." }
$paragraph = @()
for ($line = $opens; $line -lt $changelog.Count; $line++) {
    if ($changelog[$line] -match '^#{2,6} ') { break }
    if ($changelog[$line].Trim() -eq '') {
        # The blank between the heading and the prose is not the end of the paragraph.
        if ($paragraph.Count -gt 0) { break } else { continue }
    }
    $paragraph += $changelog[$line]
}
if ($paragraph.Count -eq 0) {
    Fail "The '## v$version' section in CHANGELOG.md opens with a heading and no prose, so there is nothing to put in the tag."
}
$annotation = "AnotherCrewLink $version`n`n" + ($paragraph -join "`n") + "`n"

# A tag on a commit nobody else has is a build of something no one can look at.
$dirty = git status --porcelain
if ($dirty) { Fail 'The working tree has changes. Commit or stash them first.' }
$head = (git rev-parse HEAD).Trim()
$onRemote = git branch -r --contains $head 2>$null
if (-not $onRemote) { Fail 'HEAD is not on any remote branch. Push it first.' }

# An existing tag is where a previous run got to, not a reason to start again.
#
# It used to be a wall: "cut a new version". That is the wrong answer to every way this
# script can stop after the tag is pushed -- a network blip, a Ctrl+C, gh failing to resolve
# a repository -- because the tag is the *cheap* half. What is expensive is the build it
# started, which is still running, and the release it drafted, which is still there. So the
# run picks up from whatever is already done: it re-uses the build, and it skips the upload
# of anything already uploaded. What it will not do is move a tag, because a tag that has
# been built from is a tag somebody may have installed.
$tagged = $false
git rev-parse --verify --quiet "refs/tags/$Tag" > $null
if ($LASTEXITCODE -eq 0) {
    $points = (git rev-list -n 1 $Tag).Trim()
    if ($points -ne $head) {
        Fail "The tag $Tag is on $($points.Substring(0, 12)) and HEAD is $($head.Substring(0, 12)). Releases are immutable; cut a new version rather than moving a tag."
    }
    $tagged = $true
    Note "$Tag already exists here, on this commit. Carrying on from there."
}

# --- the key ----------------------------------------------------------------------------
# Where the ceremony put it. Defaulted rather than asked for, because a path typed at a
# prompt is a path that can be typed wrong, and the way it goes wrong is by handing over the
# public half -- which used to fail with a message about a missing file rather than about
# the mistake that was made.
$here = Join-Path $repo 'sign-cert' | Join-Path -ChildPath 'release.key'
if (-not $Key) {
    $Key = if (Test-Path -LiteralPath $here) { $here } else { Read-Host 'Path to the private signing key' }
}
$Key = $Key.Trim('"').Trim()
if ($Key.EndsWith('.pub', [StringComparison]::OrdinalIgnoreCase)) {
    Fail "$Key is the public half. -Key wants the private one: the .key file beside it."
}
if (-not (Test-Path -LiteralPath $Key)) { Fail "No key at $Key." }

# minisign names a pair `<name>.key` and `<name>.pub`, siblings rather than one suffixed
# onto the other. `<key>.pub` is tried as well, because that is the only thing this script
# looked for until 2026-08-28 and somebody may have named a file to suit it.
if (-not $Public) {
    $beside = [System.IO.Path]::ChangeExtension($Key, '.pub')
    $Public = if (Test-Path -LiteralPath $beside) { $beside } else { "$Key.pub" }
}
if (-not (Test-Path -LiteralPath $Public)) {
    Fail "No public key at $Public. Pass -Public if it is elsewhere."
}

# A private key inside the working tree is one `git add -f` away from being in a public
# history. Living there is allowed -- it is where the ceremony put it -- but only for as
# long as git is ignoring it, and that is worth confirming before rather than after.
if ($Key.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)) {
    git check-ignore --quiet -- $Key
    if ($LASTEXITCODE -ne 0) {
        Fail "$Key is inside the repository and git is not ignoring it. Put it back in .gitignore before signing anything."
    }
}

$secure = Read-Host 'Key passphrase' -AsSecureString
if ($secure.Length -eq 0) { Fail 'No passphrase given.' }

$work = if ($WorkDir) { $WorkDir } else { Join-Path ([System.IO.Path]::GetTempPath()) "acl-release-$version" }
if ($work.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)) {
    Fail 'The work directory is inside the repository. A signature there is one `git add -A` from being committed.'
}
New-Item -ItemType Directory -Force -Path $work | Out-Null

try {
    $env:ACL_RELEASE_KEY_PASSWORD =
        [System.Net.NetworkCredential]::new('', $secure).Password

    # --- the tool -------------------------------------------------------------------------
    Step 'Building acl-release'
    cargo build --locked --release --features ceremony -p acl-updater --bin acl-release
    if ($LASTEXITCODE -ne 0) { Fail 'acl-release did not build.' }
    $tool = Join-Path $repo 'target\release\acl-release.exe'

    # --- 0. is this the key the fleet trusts ----------------------------------------------
    # Before the tag, not after. A key that cannot produce signatures the shipped clients
    # accept makes a release nobody can install, and a tag pushed for it is a tag that has
    # to be explained rather than deleted.
    Step 'Checking the key against the keys clients trust'
    & $tool check --key $Key
    if ($LASTEXITCODE -ne 0) { Fail 'The key did not verify. Nothing has been tagged.' }

    # --- 1. tag ---------------------------------------------------------------------------
    if ($tagged) {
        Step "$Tag is already tagged"
    }
    else {
        Step "Tagging $Tag and pushing"
        Note 'Signing the tag will ask for your GPG passphrase, which is not the signing key passphrase.'
        # Through a file, and with a message. `tag.gpgSign` is set in this repository, so a
        # bare `git tag` makes a signed annotated tag and stops with "no tag message?" when
        # the editor hands back nothing -- which is what it does when nobody is sitting at
        # one. A file rather than `-m` because the message is UTF-8 prose out of the
        # changelog, and a command line is one more place for an em dash to arrive as
        # something else.
        $annotationFile = Join-Path $work 'tag-message.txt'
        Set-Content -LiteralPath $annotationFile -Value $annotation -Encoding utf8NoBOM -NoNewline
        git tag -F $annotationFile $Tag
        if ($LASTEXITCODE -ne 0) { Fail 'git tag failed.' }
    }
    # Pushed unconditionally: a tag made by a previous run that then failed to push is
    # exactly the state this has to be able to leave. Git says "Everything up-to-date" when
    # it is already there.
    git push origin $Tag
    if ($LASTEXITCODE -ne 0) {
        if (-not $tagged) {
            git tag -d $Tag | Out-Null
            Fail 'Pushing the tag failed. The local tag has been removed so this can be run again.'
        }
        Fail 'Pushing the tag failed. The tag is still here; run this again when the network is back.'
    }

    # --- 2. wait for the build ------------------------------------------------------------
    Step 'Waiting for the release build'
    Note 'This takes a few minutes. Ctrl+C is safe here: the tag is pushed and the build carries on.'
    $run = $null
    foreach ($attempt in 1..30) {
        # By file name rather than by title: a workflow's `name:` is prose somebody can
        # reword, and the file it lives in is what the tag actually triggered.
        $found = gh run list --repo $slug --branch $Tag --workflow rust-release.yml --limit 1 --json databaseId,status,conclusion |
            ConvertFrom-Json
        if ($found) { $run = $found[0]; break }
        Start-Sleep -Seconds 4
    }
    if (-not $run) { Fail "No release run appeared for $Tag. Look at the Actions tab; the tag is pushed, so run this again once it starts." }

    if ($run.status -eq 'completed') {
        Note "The build has already finished: $($run.conclusion)."
        if ($run.conclusion -ne 'success') {
            Fail "That run ended as $($run.conclusion). A build that failed cannot be re-signed into a release -- fix it and cut a new version, because this tag has been built from."
        }
    }
    else {
        gh run watch --repo $slug $run.databaseId --exit-status
        # A watch that could not report is not a build that failed, and saying so sent
        # somebody looking for a broken build that was in fact green. The run is asked
        # again, and it is the run's own answer that decides.
        if ($LASTEXITCODE -ne 0) {
            $after = gh run view --repo $slug $run.databaseId --json status,conclusion | ConvertFrom-Json
            if ($after.status -ne 'completed') {
                Fail "Lost sight of the build. It is still going: $($after.status). The tag is pushed, so run this again when it has finished."
            }
            if ($after.conclusion -ne 'success') {
                Fail "The release build ended as $($after.conclusion). Fix it, then cut a new version -- this tag has been built from."
            }
            Note 'The watch dropped out, but the build finished green. Carrying on.'
        }
    }

    # --- 3. fetch and sign ------------------------------------------------------------------
    Step 'Fetching the manifest'
    gh run download --repo $slug $run.databaseId --name "manifest-$version" --dir $work
    if ($LASTEXITCODE -ne 0) { Fail "The manifest artefact was not there. Expected 'manifest-$version'." }
    $manifest = Join-Path $work 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifest)) { Fail "No manifest.json in $work." }

    Step 'Signing'
    & $tool sign --manifest $manifest --key $Key --public $Public
    if ($LASTEXITCODE -ne 0) { Fail 'Signing failed. Nothing has been published.' }
    $signature = "$manifest.minisig"
    if (-not (Test-Path -LiteralPath $signature)) { Fail 'No signature was written.' }

    # --- 4. publish -------------------------------------------------------------------------
    # What is already there, so a second run does not try to upload a file twice or ask
    # whether to publish something that is published.
    $release = gh release view --repo $slug $Tag --json isDraft,assets 2>$null | ConvertFrom-Json
    if ($null -eq $release) { Fail "The build finished but there is no $Tag release to publish. Look at the Actions tab." }
    if (-not $release.isDraft) {
        Write-Host "`n== $Tag is already published." -ForegroundColor Green
        $there = @($release.assets | ForEach-Object { $_.name })
        foreach ($wanted in 'manifest.json', 'manifest.json.minisig') {
            if ($there -notcontains $wanted) {
                Fail "...but $wanted is not on it, and a published release is immutable. The clients cannot accept this one; cut a new version."
            }
        }
        Note 'Both the manifest and its signature are on it. There is nothing left to do.'
        exit 0
    }
    $already = @($release.assets | ForEach-Object { $_.name })

    $described = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
    Write-Host "`n-- about to publish ------------------------------------" -ForegroundColor Yellow
    Write-Host "   release  $Tag"
    Write-Host "   version  $($described.version)"
    Write-Host "   artefact $($described.url)"
    Write-Host "   size     $($described.size) bytes"
    Write-Host "   sha512   $($described.sha512.Substring(0, 32))…"
    Write-Host "   files    manifest.json, manifest.json.minisig"
    Write-Host "   Releases are immutable. This cannot be edited afterwards." -ForegroundColor Yellow

    if (-not $Yes) {
        $answer = Read-Host "`nType 'publish' to go ahead"
        if ($answer -ne 'publish') {
            Note "Stopped. The draft release exists with its installer; the signed manifest is in $work."
            Note 'Nothing is lost: run this again and it picks up from here.'
            exit 0
        }
    }

    Step 'Uploading both files'
    # `--clobber`, because a previous run may have uploaded one of them before it stopped.
    # An asset on a *draft* is not yet published and replacing it changes nothing anybody
    # has seen; the immutability that matters begins one step below.
    if ($already.Count -gt 0) { Note "Replacing what a previous run left: $($already -join ', ')." }
    gh release upload --repo $slug --clobber $Tag $manifest $signature
    if ($LASTEXITCODE -ne 0) { Fail 'The upload failed. The release is still a draft, so run this again.' }

    Step 'Publishing'
    gh release edit --repo $slug $Tag --draft=false
    if ($LASTEXITCODE -ne 0) { Fail 'The release did not publish. Both files are uploaded; run this again.' }

    Write-Host "`n== $Tag is published." -ForegroundColor Green
    # `releases/latest` is GitHub's newest release that is neither a draft nor a pre-release.
    # Every 2.0.0 alpha is a pre-release, so the feed the client polls does not resolve to
    # this one -- see the "Known gap" section of docs/release-ceremony.md.
    if ($version -match '-') {
        Write-Host "   Note: $version is a pre-release, so the client's update feed will not find it." -ForegroundColor Yellow
        Write-Host '   docs/release-ceremony.md, "Known gap", has the three ways out.' -ForegroundColor DarkGray
    }
}
finally {
    # Whatever happened above, including Ctrl+C.
    $env:ACL_RELEASE_KEY_PASSWORD = $null
    if ($secure) { $secure.Dispose() }
}
