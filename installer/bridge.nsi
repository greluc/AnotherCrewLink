; The bridge installer: published into the 1.x feed as 1.1.0, run by the installed fleet.
;
; §4.12 items 1 and 4. This is the one artefact in the project that a large number of
; machines execute without anybody choosing to — `electron-updater` on every 1.x install
; polls `latest.yml`, sees 1.1.0, downloads this, and runs it with
;
;     bridge.exe --updated /S /D=<the 1.x install directory>
;
; So `/D=` points at Electron's installation, and this installs 2.x over it.
;
; IT RENAMES. IT DOES NOT DELETE.
;
; §4.12 item 4: "The first bridge installer renames rather than deletes the Electron install
; and its config, and 2.x ships a documented way back. Only after the bridge has sat at full
; rollout for a cycle does it begin deleting."
;
; The Electron files are moved into `1.x-backup` beside them. Renaming is reversible by a
; person with a file manager; deleting is reversible by nobody, on a number of machines
; nobody chose.
;
; THE CONFIG IS NOT TOUCHED AT ALL, AND THAT IS A DEPARTURE.
;
; Item 4 says "the Electron install *and its config*". Since 2026-08-26 the two versions
; keep their settings in separate directories — `%APPDATA%\AnotherCrewLink` is 1.x's and
; `%APPDATA%\ACL` is 2.x's (§4.9 item 4) — and `acl_core::paths::import` reads the first
; forward into the second on first run, once, without writing back.
;
; Renaming 1.x's config would therefore break the import it exists to enable: 2.x would
; start with defaults and the settings would be sitting in a directory with a different
; name. Leaving it untouched is strictly more conservative than renaming it, serves the same
; purpose — a documented way back — and is what the split made possible. Recorded here
; rather than done quietly.

Unicode true
ManifestDPIAware true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "1.1.0"
!endif
!ifndef SOURCE_DIR
  !define SOURCE_DIR "..\target\release"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "AnotherCrewLink-Setup-${VERSION}.exe"
!endif

!define PRODUCT "AnotherCrewLink"
!define APP_DIRECTORY "ACL"
!define REGISTRY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_DIRECTORY}"
; Where the Electron files are put, inside the installation `/D=` names.
!define BACKUP "1.x-backup"

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_DIRECTORY}"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} bridge installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Lucas Greuloch. GPL-3.0-or-later."

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var IsUpdate

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
  ; Both clients, and the helper. The Electron one is what is being replaced and is running
  ; on most of these machines: its own updater is what started this installer.
  nsExec::Exec 'taskkill /IM AnotherCrewLink.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM anothercrewlink.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM acl-helper.exe /F'
  Pop $0

  ; The Electron installation, moved aside rather than removed. Each is renamed rather than
  ; deleted, and a failure to rename is not fatal: a machine where one file could not be
  ; moved is still a machine that should end up with a working 2.x.
  CreateDirectory "$INSTDIR\${BACKUP}"
  Rename "$INSTDIR\AnotherCrewLink.exe" "$INSTDIR\${BACKUP}\AnotherCrewLink.exe"
  Rename "$INSTDIR\resources" "$INSTDIR\${BACKUP}\resources"
  Rename "$INSTDIR\locales" "$INSTDIR\${BACKUP}\locales"
  Rename "$INSTDIR\Uninstall AnotherCrewLink.exe" "$INSTDIR\${BACKUP}\Uninstall AnotherCrewLink.exe"

  SetOutPath "$INSTDIR"
  File "${SOURCE_DIR}\anothercrewlink.exe"
  File "${SOURCE_DIR}\acl-helper.exe"
  File /nonfatal "${SOURCE_DIR}\acl-updater.exe"
  SetOutPath "$INSTDIR\static\locales"
  File /r "..\static\locales\*.*"
  SetOutPath "$INSTDIR"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "Publisher" "Lucas Greuloch"
  WriteRegStr HKCU "${REGISTRY_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${REGISTRY_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayIcon" '"$INSTDIR\anothercrewlink.exe"'
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoRepair" 1

  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\anothercrewlink.exe"

  ; Never, on this installer. Every machine that runs it is running it because its updater
  ; decided to, not because somebody asked — so starting a window would be a program the
  ; user did not open appearing while they were doing something else.
  ;
  ; The plain installer starts the app on a first install; this one has no first install.
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

  ; `1.x-backup` is left. It is the documented way back, and an uninstall is not the moment
  ; to take it away -- somebody uninstalling 2.x may be doing exactly that in order to
  ; return to 1.x.
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${REGISTRY_KEY}"
SectionEnd
