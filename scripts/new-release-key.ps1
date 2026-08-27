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

# A refusal is an answer, not a crash. `throw` prints the offending line and a row of
# tildes, which reads as "this script is broken" when it is doing exactly its job -- and
# this one is run by somebody performing a ceremony they will only perform twice.
function Stop-With([string] $Message) {
    Write-Host ''
    Write-Host $Message -ForegroundColor Red
    Write-Host ''
    exit 1
}

# Compared as directory prefixes, so `...\BetterCrewLink-keys` is not mistaken for a path
# inside `...\BetterCrewLink`.
function Test-Inside([string] $Path, [string] $Directory) {
    return $Path -eq $Directory -or $Path.StartsWith($Directory + '\', [StringComparison]::OrdinalIgnoreCase)
}

# Anything that touches the disk has to survive a drive that is not there. This script runs
# with ErrorActionPreference Stop, and on a missing drive PowerShell's path cmdlets raise
# rather than returning false -- so an unplugged USB stick came back as "Cannot find drive.
# A drive with the name 'E' does not exist", from whichever line happened to look first,
# instead of the sentence saying what to do about it.
#
# `Join-Path` is one of those cmdlets: it resolves through the provider, so it raises too.
# `[IO.Path]::Combine` is string work and does not care.
function Test-Anything([string] $Path) {
    try { return Test-Path -LiteralPath $Path } catch { return $false }
}
function Test-Directory([string] $Path) {
    try { return Test-Path -LiteralPath $Path -PathType Container } catch { return $false }
}

# The guard that matters most. Everything else here is hygiene; this one is the difference
# between a private key and a published one.
if (Test-Inside $target $repository) {
    Stop-With @"
$target is inside the repository.

A private key in the working tree is one 'git add -A' away from being pushed, and a key
that has been pushed is a key that must be replaced -- there is no taking it back out of
a clone somebody already made.

Put it on removable media, or anywhere outside $repository.
"@
}

if ($PassphraseInto) {
    $passphrasePath = [System.IO.Path]::GetFullPath($PassphraseInto, $PWD.Path)
    if (Test-Inside $passphrasePath $target) {
        Stop-With "The passphrase would be written inside $target, beside the key it protects. That is not a second factor; it is a longer key -- anyone who copies the directory has both halves. Choose somewhere else, or leave -PassphraseInto off and paste into a password manager."
    }
    if (Test-Inside $passphrasePath $repository) {
        Stop-With "$passphrasePath is inside the repository. See above: the working tree is not where secrets go."
    }
    $passphraseParent = Split-Path -Parent $passphrasePath
    if (-not (Test-Directory $passphraseParent)) {
        Stop-With "$passphraseParent does not exist. Create it deliberately, so a typo does not put the passphrase somewhere nobody looks."
    }
}

# After the checks about *where* this points, and before anything else that looks at the
# disk. A typo in a drive letter should not silently make a directory -- but it also should
# not be reported by whichever later line happened to touch the disk first, which is how an
# unplugged E: came back as a complaint from the "is there already a key here" check.
$targetParent = Split-Path -Parent $target
if (-not (Test-Directory $targetParent)) {
    Stop-With @"
$targetParent does not exist.

Create it deliberately, so a mistyped drive letter does not silently make one -- and if
that was meant to be removable media, plug it in.
"@
}

if ((Test-Anything ([System.IO.Path]::Combine($target, 'release.key'))) -or
    (Test-Anything ([System.IO.Path]::Combine($target, 'release.pub')))) {
    Stop-With "$target already holds a key. Move it aside deliberately: replacing one silently retires every client that trusts it, at the next release, with no step in between where anybody could notice."
}

# --- the tool ----------------------------------------------------------------------------

if (-not $ToolPath) {
    $built = Join-Path $repository 'target\release\acl-release.exe'
    if (Test-Anything $built) {
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
# Tolerant too: -ToolPath is a path a person typed, and it can name a drive that is not
# there just as easily as -Into can.
if (-not (Test-Anything $ToolPath)) { Stop-With "$ToolPath is not there" }

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
        Stop-With "acl-release refused:`n$output"
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
