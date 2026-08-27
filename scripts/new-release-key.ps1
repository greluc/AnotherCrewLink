<#
.SYNOPSIS
    Generates one minisign release keypair and the passphrase that protects it.

.DESCRIPTION
    The ceremony §4.9 asks for, as one command. It makes the passphrase here rather than
    asking for one, for two reasons: a passphrase somebody invents at a prompt is a
    passphrase they have used before, and a passphrase typed on a command line is in the
    shell's history and in every process listing while it runs. This one is generated from
    the system CSPRNG, handed to `acl-release` through the environment, and shown once.

    Nothing about this script leaves the machine it runs on. That is the point of it.

    RUN IT TWICE, INTO TWO PLACES. §4.9 wants two keys: an operational one that signs
    releases, and a recovery one that never touches a workflow and is what the project
    recovers with if the first is lost or stolen. A client that trusts both can be handed a
    manifest signed by the second without an update having to reach it first — which is
    impossible when the first is the one that has gone. Two keys in one directory are one
    key with two names, so `-Role` only names the key; you choose where each goes.

.PARAMETER Into
    Where the keypair goes. Refused if it is anywhere inside this repository: a key in the
    working tree is one `git add -A` away from being public, permanently, in a place that
    cannot be un-published. Prefer removable media you then remove.

.PARAMETER Role
    `operational` or `recovery`. Names the key in what is printed; it does not change what
    is generated. Both are ordinary minisign keys.

.PARAMETER PassphraseInto
    Optional. A file to write the passphrase to — refused if it is in the same directory as
    the key, because a passphrase stored beside the key it protects is not a second factor,
    it is a longer key. Most people should leave this off and paste into a password manager.

.PARAMETER ToolPath
    Optional. A prebuilt `acl-release.exe`. Without it the script builds one, which needs
    the network the first time — so for an offline ceremony, build first, disconnect, then
    run this with -ToolPath.

.EXAMPLE
    .\scripts\new-release-key.ps1 -Into E:\acl-keys\operational -Role operational

.EXAMPLE
    .\scripts\new-release-key.ps1 -Into F:\backup\acl-recovery -Role recovery -ToolPath .\target\release\acl-release.exe
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Into,

    [Parameter(Mandatory)]
    [ValidateSet('operational', 'recovery')]
    [string] $Role,

    [string] $PassphraseInto,

    [string] $ToolPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repository = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

# --- where it must not go ----------------------------------------------------------------

# Full paths without touching the disk. `Resolve-Path` refuses a path that is not there,
# and none of these exist yet -- but every check below is about *where* a path points, not
# about whether it is already a directory. Doing it the other way round put the "does the
# parent exist" complaint in front of the safety checks, so the likeliest mistake of all
# (writing the passphrase into the key directory) came back as "that directory does not
# exist", which is true, unhelpful, and stops being true on the second run.
$target = [System.IO.Path]::GetFullPath($Into, $PWD.Path).TrimEnd('\')
$repository = $repository.TrimEnd('\')

# Compared as directory prefixes, so `...\BetterCrewLink-keys` is not mistaken for a path
# inside `...\BetterCrewLink`.
function Test-Inside([string] $Path, [string] $Directory) {
    return $Path -eq $Directory -or $Path.StartsWith($Directory + '\', [StringComparison]::OrdinalIgnoreCase)
}

# `Test-Path` on a drive that does not exist raises, and this script runs with
# ErrorActionPreference Stop -- so a mistyped drive letter came back as PowerShell's
# "Cannot find drive" instead of the sentence explaining what to do about it.
function Test-Directory([string] $Path) {
    try { return Test-Path -LiteralPath $Path -PathType Container } catch { return $false }
}

# The guard that matters most. Everything else here is hygiene; this one is the difference
# between a private key and a published one.
if (Test-Inside $target $repository) {
    throw @"
$target is inside the repository.

A private key in the working tree is one 'git add -A' away from being pushed, and a key
that has been pushed is a key that must be replaced -- there is no taking it back out of
a clone somebody already made.

Put it on removable media, or anywhere outside $repository.
"@
}

if ((Test-Path -LiteralPath (Join-Path $target 'release.key')) -or (Test-Path -LiteralPath (Join-Path $target 'release.pub'))) {
    throw "$target already holds a key. Move it aside deliberately: replacing one silently retires every client that trusts it, at the next release, with no step in between where anybody could notice."
}

if ($PassphraseInto) {
    $passphrasePath = [System.IO.Path]::GetFullPath($PassphraseInto, $PWD.Path)
    if (Test-Inside $passphrasePath $target) {
        throw "The passphrase would be written inside $target, beside the key it protects. That is not a second factor; it is a longer key -- anyone who copies the directory has both halves. Choose somewhere else, or leave -PassphraseInto off and paste into a password manager."
    }
    if (Test-Inside $passphrasePath $repository) {
        throw "$passphrasePath is inside the repository. See above: the working tree is not where secrets go."
    }
    $passphraseParent = Split-Path -Parent $passphrasePath
    if (-not (Test-Directory $passphraseParent)) {
        throw "$passphraseParent does not exist. Create it deliberately, so a typo does not put the passphrase somewhere nobody looks."
    }
}

# Last, so the checks about *where* this points come first. A typo in a drive letter should
# not silently make a directory, but it also should not be the first thing complained about
# when the path is somewhere it must never go.
$targetParent = Split-Path -Parent $target
if (-not (Test-Directory $targetParent)) {
    throw "$targetParent does not exist. Create the parent directory deliberately, so a typo in a drive letter does not silently make one."
}

# --- the tool ----------------------------------------------------------------------------

if (-not $ToolPath) {
    $built = Join-Path $repository 'target\release\acl-release.exe'
    if (Test-Path $built) {
        $ToolPath = $built
    }
    else {
        Write-Host 'Building acl-release (this needs the network the first time)...' -ForegroundColor DarkGray
        Push-Location $repository
        try {
            & cargo build --locked --release --features ceremony -p acl-updater --bin acl-release
            if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
        }
        finally { Pop-Location }
        $ToolPath = $built
    }
}
if (-not (Test-Path $ToolPath)) { throw "$ToolPath is not there" }

# --- the passphrase ----------------------------------------------------------------------

# Digits 2-9 and lowercase without i or l. Exactly 32 symbols, and that is not a
# coincidence: 32 divides 256, so mapping a random byte with `-band 31` is unbiased and
# there is no rejection sampling or modulo skew to reason about.
#
# 0 and 1 are absent, which is what makes o, and the letters that get confused with 1,
# safe to keep -- there is no digit left for them to be mistaken for. i and l go anyway:
# they are thin, and this will be read off a phone screen or aloud at some point.
#
# No punctuation. It would buy a few bits and cost an argument with somebody's shell.
$alphabet = '23456789abcdefghjkmnopqrstuvwxyz'.ToCharArray()
if ($alphabet.Count -ne 32) { throw "the alphabet is $($alphabet.Count) symbols, not 32" }
$groups = 6
$perGroup = 5   # 6 x 5 x 5 bits = 150 bits

$bytes = [byte[]]::new($groups * $perGroup)
[System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
$characters = $bytes | ForEach-Object { $alphabet[$_ -band 31] }
$passphrase = (0..($groups - 1) | ForEach-Object {
        -join $characters[($_ * $perGroup)..($_ * $perGroup + $perGroup - 1)]
    }) -join '-'

# --- generate ------------------------------------------------------------------------------

# Through the environment, never as an argument: an argument is in the process list for as
# long as the process runs, readable by anything else on the machine.
$env:ACL_RELEASE_KEY_PASSWORD = $passphrase
try {
    $output = & $ToolPath keys --into $target 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "acl-release refused:`n$output"
    }
}
finally {
    # Out of this process, whatever happened. It is still in the child's memory until it
    # exits, which it already has.
    $env:ACL_RELEASE_KEY_PASSWORD = $null
}

# Read back out of what the tool printed rather than out of the file, so this checks the
# line the maintainer is about to paste is the line that exists.
$found = $output | Select-String -Pattern '"(RW[A-Za-z0-9+/=]+)",' | Select-Object -First 1
if (-not $found) {
    throw "The key was made but the public half could not be read back out of:`n$($output -join [Environment]::NewLine)"
}
$public = $found.Matches[0].Groups[1].Value

if ($PassphraseInto) {
    Set-Content -Path $PassphraseInto -Value $passphrase -NoNewline -Encoding utf8
}

# --- what to do next -----------------------------------------------------------------------

Write-Host ''
Write-Host "  The $Role key is in $target" -ForegroundColor Green
Write-Host ''
Write-Host '  Passphrase (shown once):' -ForegroundColor Yellow
Write-Host ''
Write-Host "      $passphrase" -ForegroundColor White
Write-Host ''
if ($PassphraseInto) {
    Write-Host "  Also written to $PassphraseInto" -ForegroundColor DarkGray
}
else {
    Write-Host '  Put it in a password manager now. It is not recoverable, and a key that' -ForegroundColor DarkGray
    Write-Host '  cannot be opened is a key that has to be replaced.' -ForegroundColor DarkGray
}
Write-Host ''
Write-Host '  Add this to PUBLIC_KEYS in crates/acl-updater/src/manifest.rs:' -ForegroundColor Cyan
Write-Host ''
Write-Host "      `"$public`"," -ForegroundColor White
Write-Host ''
if ($Role -eq 'operational') {
    Write-Host '  Then run this again with -Role recovery, into somewhere else entirely.' -ForegroundColor DarkGray
    Write-Host '  Both public keys go in PUBLIC_KEYS; only this private half ever signs.' -ForegroundColor DarkGray
}
else {
    Write-Host '  This one never signs a release and never goes near a workflow. It is what' -ForegroundColor DarkGray
    Write-Host '  the project recovers with if the operational key is lost or stolen.' -ForegroundColor DarkGray
}
Write-Host ''
Write-Host '  Clear your terminal when you are done reading this.' -ForegroundColor DarkGray
Write-Host ''
