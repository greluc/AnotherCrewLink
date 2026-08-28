<#
.SYNOPSIS
    Performs the whole 2.x release ceremony: tag, wait, sign, publish.
.DESCRIPTION
    `docs/release-ceremony.md` describes the ceremony as five commands a maintainer runs in
    order. This is those commands, in that order, with the checks between them that a person
    doing it by hand would have to remember.

    You are asked for two things and nothing else: the tag, and the passphrase. Everything
    else is derived or checked.

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
$notes = Select-String -Path (Join-Path $repo 'CHANGELOG.md') -Pattern "^## v$([regex]::Escape($version))$" -Quiet
if (-not $notes) { Fail "CHANGELOG.md has no '## v$version' section. The release would go out with no notes." }

# A tag on a commit nobody else has is a build of something no one can look at.
$dirty = git status --porcelain
if ($dirty) { Fail 'The working tree has changes. Commit or stash them first.' }
git rev-parse --verify --quiet "refs/tags/$Tag" > $null
if ($LASTEXITCODE -eq 0) { Fail "The tag $Tag already exists here. Releases are immutable; cut a new version." }
$head = (git rev-parse HEAD).Trim()
$onRemote = git branch -r --contains $head 2>$null
if (-not $onRemote) { Fail 'HEAD is not on any remote branch. Push it first.' }

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
    Step "Tagging $Tag and pushing"
    git tag $Tag
    if ($LASTEXITCODE -ne 0) { Fail 'git tag failed.' }
    git push origin $Tag
    if ($LASTEXITCODE -ne 0) {
        git tag -d $Tag | Out-Null
        Fail 'Pushing the tag failed. The local tag has been removed so this can be run again.'
    }

    # --- 2. wait for the build ------------------------------------------------------------
    Step 'Waiting for the release build'
    Note 'This takes a few minutes. Ctrl+C is safe here: the tag is pushed and the build carries on.'
    $run = $null
    foreach ($attempt in 1..30) {
        $found = gh run list --branch $Tag --workflow 'Rust Release' --limit 1 --json databaseId,status |
            ConvertFrom-Json
        if ($found) { $run = $found[0]; break }
        Start-Sleep -Seconds 4
    }
    if (-not $run) { Fail "No release run appeared for $Tag. Look at the Actions tab; the tag is pushed." }
    gh run watch $run.databaseId --exit-status
    if ($LASTEXITCODE -ne 0) { Fail 'The release build failed. Fix it, then cut a new version -- this tag is spent.' }

    # --- 3. fetch and sign ------------------------------------------------------------------
    Step 'Fetching the manifest'
    gh run download $run.databaseId --name "manifest-$version" --dir $work
    if ($LASTEXITCODE -ne 0) { Fail "The manifest artefact was not there. Expected 'manifest-$version'." }
    $manifest = Join-Path $work 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifest)) { Fail "No manifest.json in $work." }

    Step 'Signing'
    & $tool sign --manifest $manifest --key $Key --public $Public
    if ($LASTEXITCODE -ne 0) { Fail 'Signing failed. Nothing has been published.' }
    $signature = "$manifest.minisig"
    if (-not (Test-Path -LiteralPath $signature)) { Fail 'No signature was written.' }

    # --- 4. publish -------------------------------------------------------------------------
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
            Note "To finish by hand: gh release upload $Tag `"$manifest`" `"$signature`" && gh release edit $Tag --draft=false"
            exit 0
        }
    }

    Step 'Uploading both files'
    gh release upload $Tag $manifest $signature
    if ($LASTEXITCODE -ne 0) { Fail 'The upload failed. The release is still a draft.' }

    Step 'Publishing'
    gh release edit $Tag --draft=false
    if ($LASTEXITCODE -ne 0) { Fail 'The release did not publish. Both files are uploaded; finish it by hand.' }

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
