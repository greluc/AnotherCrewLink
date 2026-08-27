; The 1.0.x installer: this project's own NSIS script, carrying the Electron client.
;
; WHY THIS EXISTS.
;
; §4.9: "Prove the new NSIS script by shipping an **ordinary 1.0.x release** with it, so its
; CLI contract is tested against real 1.x updaters before it carries anything important."
; Decided 2026-08-27 that this is the path.
;
; The thing being proven is the contract in `common.nsh` -- `--updated`, `/S`, `/D=`, the
; per-user install, the uninstall entry -- against the `electron-updater` instances actually
; running on people's machines, which no test here can stand in for. `rust.yml`'s installer
; job compiles all three scripts and runs this project's install and uninstall on a clean
; runner; that catches a script NSIS will not accept, and cannot catch a fleet.
;
; The payload has to be the Electron client, or this would not be an ordinary 1.0.x release
; -- it would be the bridge, which is a different act with a different blast radius. So this
; script packages `dist/win-unpacked`, which is what `electron-builder --dir` leaves behind.
;
; IT INSTALLS WHERE 1.x ALREADY IS.
;
; `APP_DIRECTORY` is `AnotherCrewLink` here and `ACL` in the other two. That is not drift:
; 1.0.2 installs to `%LOCALAPPDATA%\Programs\AnotherCrewLink` and its updater passes that
; path back as `/D=`, so an update lands in the right place either way -- but somebody
; installing 1.0.3 fresh must land there too, or they end up with two 1.x installations.
; `installer_contract.rs` checks the *other* two against `acl_core::paths::APP_DIRECTORY`
; and this one against 1.x's, because they are answering different questions.
;
; IT IS x64-ONLY, AND THAT IS THE DECISION SHOWING UP EARLY.
;
; `electron-builder.yml` has had no ia32 target since 2026-08-25, so a 1.0.3 built from this
; tree is x64. A 32-bit 1.0.2 client whose updater fetches it would be handed x64 binaries --
; the same failure the bridge would cause, arriving one release sooner. The guard in
; `common.nsh` refuses it, and the message here says what is true for a 1.x user rather than
; talking about 2.0: for them this release is where updates stop.

!ifndef VERSION
  !define VERSION "1.0.3"
!endif
; What `electron-builder --dir` produces. Not `target/release`: this carries the Electron
; client, not the Rust one.
!ifndef SOURCE_DIR
  !define SOURCE_DIR "..\dist\win-unpacked"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "AnotherCrewLink-Setup-${VERSION}.exe"
!endif

!define PRODUCT "AnotherCrewLink"
; 1.x's directory, deliberately. See the header.
!define APP_DIRECTORY "AnotherCrewLink"
!define REGISTRY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_DIRECTORY}"
!define DESCRIPTION "${PRODUCT} installer"
!define ARCH_REFUSAL "AnotherCrewLink needs 64-bit Windows.$\r$\n$\r$\nThis machine is running 32-bit Windows. Version 1.0.2 is the last release that supports it and keeps working; nothing here has been changed."

!include "common.nsh"

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_DIRECTORY}"
InstallDirRegKey HKCU "${REGISTRY_KEY}" "InstallLocation"

Function .onInit
  Call ParseArguments
  !insertmacro ACL_ARCHITECTURE_GUARD
FunctionEnd

Section "Install"
  ; The Electron client, closed before its files are replaced underneath it. Its own updater
  ; is what started this on most machines, and it is still running when it does.
  nsExec::Exec 'taskkill /IM AnotherCrewLink.exe /F'
  Pop $0

  ; A tree, not a list. An Electron application is an executable, a `resources` directory
  ; with the asar in it, a `locales` directory of `.pak` files, and a dozen DLLs whose names
  ; change with the Electron version -- naming them here would be a list that goes stale on
  ; the next upgrade, silently, by omitting a file rather than by failing.
  SetOutPath "$INSTDIR"
  File /r "${SOURCE_DIR}\*.*"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${REGISTRY_KEY}" "Publisher" "Lucas Greuloch"
  WriteRegStr HKCU "${REGISTRY_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${REGISTRY_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${REGISTRY_KEY}" "DisplayIcon" '"$INSTDIR\AnotherCrewLink.exe"'
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${REGISTRY_KEY}" "NoRepair" 1

  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\AnotherCrewLink.exe"

  ${If} $IsUpdate == "0"
    ${IfNot} ${Silent}
      Exec '"$INSTDIR\AnotherCrewLink.exe"'
    ${EndIf}
  ${EndIf}
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM AnotherCrewLink.exe /F'
  Pop $0

  ; The mirror of the install: a tree went down, a tree comes back up. `RMDir /r` on
  ; `$INSTDIR` itself would take the uninstaller with it while it is running, so the two
  ; directories and the executables are named and the directory is removed last.
  RMDir /r "$INSTDIR\resources"
  RMDir /r "$INSTDIR\locales"
  Delete "$INSTDIR\*.exe"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\*.pak"
  Delete "$INSTDIR\*.bin"
  Delete "$INSTDIR\*.dat"
  Delete "$INSTDIR\*.json"
  Delete "$INSTDIR\LICENSE*"
  Delete "$INSTDIR\version"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${REGISTRY_KEY}"

  ; `%APPDATA%\AnotherCrewLink` is left alone, exactly as 1.0.2's own uninstaller leaves it:
  ; it is the settings, and it is also what `acl_core::paths::import` reads forward when 2.x
  ; first runs. An uninstall that removed it would quietly cost somebody their settings on a
  ; migration they had not made yet.
SectionEnd
