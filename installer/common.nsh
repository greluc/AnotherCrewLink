; What every installer in this project must do identically.
;
; THE CLI CONTRACT IS THE POINT OF THIS FILE.
;
; `electron-updater`'s `NsisUpdater` spawns an installer as
;
;     installer.exe --updated /S /D=<installDirectory>
;
; and every one of those three has to work, because §4.12 publishes a bridge build into
; the 1.x feed and the installed fleet's updater is what runs it. `/S` and `/D=` are NSIS's
; own; `--updated` is electron-builder's and means "this is an update, not a first
; install", which here means: do not open the app afterwards and do not ask anything.
;
; `/D=` must be the last argument and takes no quotes even when the path has spaces. That
; is NSIS, not a choice made here, and it is why the uninstaller records the directory
; rather than the installer being asked to remember it.
;
; PER-USER, NEVER ELEVATED.
;
; 1.x is a one-click per-user install and so is every script here. It matters beyond habit:
; §4.9 item 3 refuses to install an update from an elevated process, and a per-machine
; installer would make every update an elevation prompt -- which trains people to click
; through the one prompt this project actually needs, the helper's.
;
; UNSIGNED, AND THAT IS WRITTEN DOWN.
;
; §4.9 item 2: no Authenticode. Every user sees the unknown-publisher warning on every
; install, exactly as they do with 1.x today. Nothing here hides that or works around it.
;
; WHY AN INCLUDE, SINCE 2026-08-27.
;
; There are three scripts now -- the 2.x installer, the bridge, and the one that packages an
; ordinary 1.0.x release to prove the contract against real updaters, which is §4.9's own
; instruction. Three copies of a contract is three chances for one of them to drift, and the
; one that drifts is discovered by the fleet. `installer_contract.rs` reads this file, so
; every script that includes it is covered by construction rather than by remembering to add
; a test.
;
; What is NOT here: the payload and the uninstall list. Those genuinely differ per script,
; and pushing them in behind `!ifdef`s would trade a real difference for a hidden one.
;
; Define before including: VERSION, PRODUCT, APP_DIRECTORY, REGISTRY_KEY, DESCRIPTION and
; ARCH_REFUSAL.

Unicode true
ManifestDPIAware true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
; For ${RunningX64}, used by ACL_ARCHITECTURE_GUARD.
!include "x64.nsh"

; VIProductVersion takes four numbers and nothing else -- a prerelease suffix aborts
; makensis with "invalid VIProductVersion format". That is not a hypothetical: the release
; workflow triggers on `v2.*`, which matches `v2.0.0-rc.1`, and §4.12's staged rollout is
; exactly the thing that would be tagged that way. So the numeric part is taken here rather
; than assumed of the caller.
;
; `!searchparse` fails when there is no `-` to find, which is the ordinary case; /noerrors
; makes that silent and the !ifndef below supplies the whole string.
!searchparse /noerrors "${VERSION}" "" VERSION_NUMERIC "-"
!ifndef VERSION_NUMERIC
  !define VERSION_NUMERIC "${VERSION}"
!endif

; The numeric four-part version for the resource, and the full string -- suffix and all --
; for the keys that are free text. A user reading the file properties should see the version
; that was released, not a rounded-off one.
VIProductVersion "${VERSION_NUMERIC}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${DESCRIPTION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Lucas Greuloch. GPL-3.0-or-later."

RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ABORTWARNING
; The setup and the uninstaller had no icon of their own and showed NSIS's default, so the
; first thing anybody saw of this project was somebody else's logo. Relative to this file's
; directory, which is how NSIS resolves a path in a script -- the same way the `File` lines
; below reach `..\static\locales`.
!define MUI_ICON "..\assets\icon.ico"
!define MUI_UNICON "..\assets\icon.ico"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var IsUpdate

; Reads `--updated` out of the command line.
;
; electron-updater passes it; a person double-clicking does not. It decides one thing --
; whether the app is started afterwards -- because an update that reopened the window a
; user had closed would be the installer deciding what they were doing.
Function ParseArguments
  ${GetParameters} $R0
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $IsUpdate "1"
  ${Else}
    StrCpy $IsUpdate "0"
  ${EndIf}
FunctionEnd

; Refuses a 32-bit machine, before anything has been changed.
;
; The 32-bit build was removed on 2026-08-25. That matters in an installer and not only in a
; build script, because of how the fleet reaches one: `electron-updater`'s `findFile` prefers
; an artefact whose name contains `x64` or `ia32` and **otherwise takes the first `.exe` in
; the feed**. There is no 32-bit artefact to publish, so there is no name to prefer, so a
; 32-bit client is handed the x64 installer and runs it. NSIS installers are themselves
; 32-bit, so it would run to the end, report success, and leave the machine with binaries
; that cannot start.
;
; Refusing rescues nobody. Decided 2026-08-27: those machines stop here, and 1.0.5 -- the
; last release built with a 32-bit half -- is the last one they get. `CHANGELOG.md`'s 1.0.6
; entry is where that is said to them; this is only the mechanism. It is the difference
; between an install that reports success and leaves nothing working, and one that says
; what happened.
;
; A macro rather than a Function, and that is not style: `Abort` inside a Function called
; from `.onInit` abandons the *function*, and the install carries on. Inlined, it aborts the
; installer, which is the whole point.
!macro ACL_ARCHITECTURE_GUARD
  ${IfNot} ${RunningX64}
    ; No dialog under /S. The updater that spawned this waits on the process and never sees
    ; a window, so a message box here is a hang rather than an explanation -- and it is a
    ; hang on the machines that were already having the worst day, which is why nobody would
    ; find it. The exit code is the only thing the updater can read.
    IfSilent acl_architecture_refused
    MessageBox MB_ICONSTOP "${ARCH_REFUSAL}"
  acl_architecture_refused:
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend
