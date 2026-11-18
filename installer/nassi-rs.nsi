; Nassi RS installer (NSIS 3, Unicode) — the same installer as nassi-rs.iss, but it can be
; built on Linux too (makensis from the "nsis" package):
;   installer/build-installer.sh   (cross-builds both exe with mingw, then runs makensis)
;
; One setup.exe for Windows 7 SP1 … 11: the installer is 32-bit and installs
;   64-bit Windows -> x86_64-win7-windows-gnu build
;   32-bit Windows -> i686-win7-windows-gnu build
;
; No administrator rights needed: installed for the current user only
; (%LOCALAPPDATA%\Programs\Nassi RS, HKCU, this user's Start menu / desktop / autostart).
; Only the optional firewall rule asks for administrator rights (UAC), and only if chosen.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestSupportedOS all

!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define APPNAME "Nassi RS"
!ifndef VERSION
  !define VERSION "0.2.6"
!endif
!define APPEXE "NassiRS.exe"
!define ROOT ".."
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\NassiRS"
; the uninstall entry of the older Inno Setup installer
!define INNO_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F3A2C61-5B7E-4D2A-9C1F-3E6B7A9D0C42}_is1"

Name "${APPNAME}"
OutFile "${ROOT}\dist\NassiRS-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
BrandingText "${APPNAME} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1049 "ProductName" "${APPNAME}"
VIAddVersionKey /LANG=1049 "FileDescription" "${APPNAME} — локальный мессенджер и удалённое администрирование"
VIAddVersionKey /LANG=1049 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1049 "ProductVersion" "${VERSION}"

!define MUI_ICON "${ROOT}\assets\app.ico"
!define MUI_UNICON "${ROOT}\assets\app.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "Будет установлен ${APPNAME} ${VERSION}.$\r$\n$\r$\nЛокальный мессенджер (личный, общий и групповой чат, файлы) и удалённое администрирование для системного администратора.$\r$\n$\r$\nРекомендуется закрыть все другие программы перед продолжением."
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Запустить ${APPNAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"

; auto-update (src/update.rs runs "setup.exe /S ... /relaunch=1|2"): start the new version
; again; 2 = it was in the tray, start it there
Function .onInstSuccess
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/relaunch=" $1
  ${IfNot} ${Errors}
    ${If} $1 == "1"
      Exec '"$INSTDIR\${APPEXE}" --delay=500'
    ${ElseIf} $1 == "2"
      Exec '"$INSTDIR\${APPEXE}" --delay=500 --tray'
    ${EndIf}
  ${EndIf}
FunctionEnd

Function .onInit
  ${IfNot} ${AtLeastWin7}
    MessageBox MB_ICONSTOP "${APPNAME} работает на Windows 7 SP1 и новее."
    Abort
  ${EndIf}
  ${If} ${IsWin7}
  ${AndIfNot} ${AtLeastServicePack} 1
    MessageBox MB_ICONSTOP "Для Windows 7 нужен Service Pack 1."
    Abort
  ${EndIf}
  ; always the same folder ($LOCALAPPDATA\Programs\Nassi RS, no directory page): every
  ; update replaces the program in place, never several copies in different folders
  ;
  ; older versions were installed for all users (Program Files, administrator rights) and start
  ; at logon for everybody: offer to run the old uninstaller (Windows asks for the rights once)
  ${If} ${RunningX64}
    SetRegView 64
  ${EndIf}
  StrCpy $2 "/S"
  ReadRegStr $1 HKLM "${UNINST_KEY}" "UninstallString"
  ${If} $1 == ""
    StrCpy $2 "/SILENT /SUPPRESSMSGBOXES /NORESTART"
    ReadRegStr $1 HKLM "${INNO_KEY}" "UninstallString"
  ${EndIf}
  SetRegView default
  ${If} $1 != ""
  ${AndIfNot} ${Silent}
    MessageBox MB_YESNOCANCEL|MB_ICONQUESTION "Найдена старая копия ${APPNAME}, установленная для всех пользователей:$\r$\n$1$\r$\n$\r$\nТеперь программа ставится в одну папку пользователя и обновляется там же, без прав администратора. Старую копию лучше удалить, иначе при входе в Windows будут запускаться две программы. Windows один раз попросит права администратора. Настройки и переписка сохранятся.$\r$\n$\r$\nДа: удалить старую копию и продолжить.$\r$\nНет: оставить её и продолжить." IDYES old_remove IDNO old_keep
    Abort
    old_remove:
      ; UninstallString is quoted: strip the quotes for ExecShellWait
      StrCpy $3 $1 1
      ${If} $3 == '"'
        StrCpy $1 $1 "" 1
        StrCpy $1 $1 -1
      ${EndIf}
      ExecShellWait "runas" "$1" "$2"
    old_keep:
  ${EndIf}
FunctionEnd

Section "${APPNAME}" SecMain
  SectionIn RO
  ; close the running program (it may be in the tray)
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM ${APPEXE}'
  Sleep 500
  ; remove a per-user version installed by the Inno Setup installer (user data in %APPDATA% stays)
  ReadRegStr $0 HKCU "${INNO_KEY}" "UninstallString"
  ${If} $0 != ""
    ExecWait '$0 /VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
  ${EndIf}

  SetOutPath "$INSTDIR"
  ${If} ${RunningX64}
    File "/oname=${APPEXE}" "${ROOT}\target\dist\x86_64-win7-windows-gnu\release\nassi-rs.exe"
  ${Else}
    File "/oname=${APPEXE}" "${ROOT}\target\dist\i686-win7-windows-gnu\release\nassi-rs.exe"
  ${EndIf}
  File "/oname=README.txt" "${ROOT}\README.md"
  SetOutPath "$INSTDIR\Emoticons"
  File /r "${ROOT}\Emoticons\*"
  SetOutPath "$INSTDIR\Sounds"
  File "${ROOT}\Sounds\*"
  SetOutPath "$INSTDIR"

  ; Start menu (of this user)
  SetShellVarContext current
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${APPEXE}"
  ; the window is drawn by the CPU by default; this one tries the video card (OpenGL)
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME} (программная отрисовка).lnk"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\${APPNAME} (OpenGL).lnk" "$INSTDIR\${APPEXE}" "--opengl"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\Удалить ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APPEXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Ярлык на рабочем столе" SecDesktop
  SetShellVarContext current
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${APPEXE}"
SectionEnd

Section "Запускать при входе в Windows" SecAutostart
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}" '"$INSTDIR\${APPEXE}"'
SectionEnd

; the only step that needs administrator rights: unticked by default, Windows asks (UAC) only if
; chosen; one elevated cmd for both netsh commands, so the question comes only once
Section /o "Разрешить в брандмауэре Windows (попросит права администратора)" SecFirewall
  ExecShellWait "runas" "$SYSDIR\cmd.exe" '/c netsh advfirewall firewall delete rule name="${APPNAME}" >nul & netsh advfirewall firewall add rule name="${APPNAME}" dir=in action=allow program="$INSTDIR\${APPEXE}" enable=yes profile=private,domain' SW_HIDE
  WriteRegDWORD HKCU "${UNINST_KEY}" "FirewallRule" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM ${APPEXE}'
  Sleep 500
  ; the firewall rule was added with administrator rights: removing it asks for them again
  ReadRegDWORD $0 HKCU "${UNINST_KEY}" "FirewallRule"
  ${If} $0 == 1
    ExecShellWait "runas" "$SYSDIR\netsh.exe" 'advfirewall firewall delete rule name="${APPNAME}"' SW_HIDE
  ${EndIf}
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}"
  SetShellVarContext current
  Delete "$DESKTOP\${APPNAME}.lnk"
  RMDir /r "$SMPROGRAMS\${APPNAME}"
  Delete "$INSTDIR\${APPEXE}"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\Emoticons"
  RMDir /r "$INSTDIR\Sounds"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST_KEY}"
  ; the chat history in %APPDATA%\NassiRS stays
SectionEnd
