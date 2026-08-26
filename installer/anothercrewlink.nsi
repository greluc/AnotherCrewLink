; The 2.x installer.
;
; §4.9 item 1: "there is no NSIS backend [in cargo-dist], and MSI would strand every
; installed 1.x client, because `electron-updater`'s `findFile` picks by extension and
; changing artefact type is the same act as abandoning the installed base. So the NSIS
; script is hand-built and keeps its exact CLI contract."
;
; THE CLI CONTRACT IS THE POINT OF THIS FILE.
;
; `electron-updater`'s `NsisUpdater` spawns an installer as
;
;     installer.exe --updated /S /D=<installDirectory>
;
; and every one of those three has to work, because P8 publishes a bridge build into the
; 1.x feed and the installed fleet's updater is what runs it. `/S` and `/D=` are NSIS's
; own; `--updated` is electron-builder's and means "this is an update, not a first
; install", which here means: do not open the app afterwards and do not ask anything.
;
; `/D=` must be the last argument and takes no quotes even when the path has spaces. That
; is NSIS, not a choice made here, and it is why the uninstaller records the directory
; rather than the installer being asked to remember it.
;
; PER-USER, NEVER ELEVATED.
;
; 1.x is a one-click per-user install and so is this. It matters beyond habit: §4.9 item 3
; refuses to install an update from an elevated process, and a per-machine installer would
; make every update an elevation prompt -- which trains people to click through the one
; prompt this project actually needs, the helper's.
;
; UNSIGNED, AND THAT IS WRITTEN DOWN.
;
; §4.9 item 2: no Authenticode. Every user sees the unknown-publisher warning on every
; install, exactly as they do with 1.0.2 today. Nothing here hides that or works around it.

Unicode true
ManifestDPIAware true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

; Supplied by the release job: !define VERSION, !define SOURCE_DIR, !define OUT_FILE.
!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SOURCE_DIR
  !define SOURCE_DIR "..\target\release"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "AnotherCrewLink-Setup-${VERSION}.exe"
!endif

!define PRODUCT "AnotherCrewLink"
; The directory name, which is `acl_core::paths::APP_DIRECTORY`. Not the product name:
; 1.x owns `%APPDATA%\AnotherCrewLink` and the two must not share a directory. See
; `installer_contract.rs`, which fails if the two drift apart.
!define APP_DIRECTORY "ACL"
!define REGISTRY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_DIRECTORY}"

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
; Per-user: `$LOCALAPPDATA` is writable without elevation, which is what makes an update
; installable by the process that downloaded it.
InstallDir "$LOCALAPPDATA\Programs\${APP_DIRECTORY}"
InstallDirRegKey HKCU "${REGISTRY_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Lucas Greuloch. GPL-3.0-or-later."

!define MUI_ABORTWARNING
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

Function .onInit
  Call ParseArguments
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"

  ; The running client holds a single-instance lock and a named pipe. Replacing its files
  ; underneath it is how an update leaves a half-written directory, so it is closed first
  ; -- and the elevated helper with it, since it is a child that outlives an abrupt exit.
  nsExec::Exec 'taskkill /IM anothercrewlink.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM acl-helper.exe /F'
  Pop $0

  File "${SOURCE_DIR}\anothercrewlink.exe"
  File "${SOURCE_DIR}\acl-helper.exe"
  File /nonfatal "${SOURCE_DIR}\acl-updater.exe"
  ; The locale tree, which the client looks for beside its executable before it looks
  ; anywhere else. Without it every string in the window is its own key.
  SetOutPath "$INSTDIR\static\locales"
  File /r "..\static\locales\*.*"
  SetOutPath "$INSTDIR"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Add/Remove Programs. Per-user, so HKCU rather than HKLM -- the same reason the install
  ; needs no elevation.
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "Publisher" "Lucas Greuloch"
  WriteRegStr HKCU "${REGISTRY_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${REGISTRY_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayIcon" '"$INSTDIR\anothercrewlink.exe"'
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoRepair" 1

  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\anothercrewlink.exe"

  ; Only on a first install, and only when a person is watching. An update that reopened a
  ; window the user had closed would be the installer deciding what they were doing, and a
  ; silent install that started a GUI would be a silent install that did not stay silent.
  ${If} $IsUpdate == "0"
    ${IfNot} ${Silent}
      Exec '"$INSTDIR\anothercrewlink.exe"'
    ${EndIf}
  ${EndIf}
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM anothercrewlink.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM acl-helper.exe /F'
  Pop $0

  Delete "$INSTDIR\anothercrewlink.exe"
  Delete "$INSTDIR\acl-helper.exe"
  Delete "$INSTDIR\acl-updater.exe"
  RMDir /r "$INSTDIR\static"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${REGISTRY_KEY}"

  ; `%APPDATA%\ACL` is deliberately left alone: settings, the offsets cache and the
  ; downloaded artwork. A reinstall keeps them, and somebody who wants them gone can
  ; delete one directory. Removing them here would also mean an *update* that ran the
  ; uninstaller first threw away the settings it was meant to carry forward.
SectionEnd
