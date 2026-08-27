; The 2.x installer.
;
; §4.9 item 1: "there is no NSIS backend [in cargo-dist], and MSI would strand every
; installed 1.x client, because `electron-updater`'s `findFile` picks by extension and
; changing artefact type is the same act as abandoning the installed base. So the NSIS
; script is hand-built and keeps its exact CLI contract."
;
; The contract itself, and why each half of it exists, is in `common.nsh`. This file is the
; payload: which binaries go down, what the uninstaller takes away again, and the one thing
; that is specific to a deliberate install -- starting the app afterwards.

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
!define DESCRIPTION "${PRODUCT} installer"
!define ARCH_REFUSAL "AnotherCrewLink 2.0 needs 64-bit Windows.$\r$\n$\r$\nThis machine is running 32-bit Windows, which this version no longer supports. Version 1.0.2 remains available and keeps working."

!include "common.nsh"

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
; Per-user: `$LOCALAPPDATA` is writable without elevation, which is what makes an update
; installable by the process that downloaded it.
InstallDir "$LOCALAPPDATA\Programs\${APP_DIRECTORY}"
InstallDirRegKey HKCU "${REGISTRY_KEY}" "InstallLocation"

Function .onInit
  Call ParseArguments
  !insertmacro ACL_ARCHITECTURE_GUARD
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
