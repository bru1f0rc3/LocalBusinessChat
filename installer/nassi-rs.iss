; Nassi RS installer (Inno Setup 6.3+)
; One setup.exe for all supported systems: the installer itself is 32-bit, runs on
; Windows 7 SP1 ... 11 (x86 and x64) and installs the matching build of the program:
;   64-bit Windows 10/11        -> x86_64-pc-windows-msvc
;   64-bit Windows 7 SP1 / 8.x  -> x86_64-win7-windows-msvc
;   32-bit Windows 7 SP1 ... 11 -> i686-win7-windows-msvc
;
; Build: build-installer.ps1 (compiles both targets, then runs ISCC on this file)
;
; No administrator rights needed: the program is installed for the current user only
; (%LOCALAPPDATA%\Programs\Nassi RS, Start menu / desktop / autostart of this user).
; Only the optional firewall rule asks for administrator rights (UAC), and only if chosen.

#define AppName "Nassi RS"
#ifndef AppVersion
  #define AppVersion "0.2.6"
#endif
#define AppExe "NassiRS.exe"
#define Root ".."
#define Assets ".."

[Setup]
AppId={{8F3A2C61-5B7E-4D2A-9C1F-3E6B7A9D0C42}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Nassi RS
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} — локальный мессенджер и удалённое администрирование
; always the same folder (%LOCALAPPDATA%\Programs\Nassi RS): every update replaces the
; program in place, there are never several copies in different folders
DefaultDirName={autopf}\{#AppName}
DisableDirPage=yes
UsePreviousAppDir=no
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; x86compatible: runs on 32-bit Windows; x64compatible: on 64-bit installs the x64 build
ArchitecturesAllowed=x86compatible x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; per-user install, no UAC prompt ({autopf} = %LOCALAPPDATA%\Programs, HKA = HKCU)
PrivilegesRequired=lowest
OutputDir={#Root}\dist
OutputBaseFilename=NassiRS-Setup-{#AppVersion}
SetupIconFile={#Root}\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
; Windows 7 SP1 and later (it was 10.0, so Windows 7/8.1 refused to install)
MinVersion=6.1sp1

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; GroupDescription: "Ярлыки:"
Name: "autostart"; Description: "Запускать Nassi RS при входе в Windows"; GroupDescription: "Дополнительно:"
; the only step that needs administrator rights: Windows asks for them (UAC) only if this is ticked
Name: "firewall"; Description: "Разрешить Nassi RS в брандмауэре Windows (попросит права администратора)"; GroupDescription: "Дополнительно:"; Flags: unchecked

[Files]
; 64-bit Windows 10/11 -> 64-bit build
Source: "{#Root}\target\dist\x86_64-pc-windows-msvc\release\nassi-rs.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Check: Is64BitInstallMode and IsWin10OrLater; Flags: ignoreversion
; 64-bit Windows 7 SP1 / 8 / 8.1 -> 64-bit build for Windows 7
Source: "{#Root}\target\dist\x86_64-win7-windows-msvc\release\nassi-rs.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Check: Is64BitInstallMode and not IsWin10OrLater; Flags: ignoreversion
; 32-bit Windows (7 SP1 ... 11) -> 32-bit build (Windows 7 compatible)
Source: "{#Root}\target\dist\i686-win7-windows-msvc\release\nassi-rs.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Check: not Is64BitInstallMode; Flags: ignoreversion
; emoticons and sounds from the project folder (picked up next to the exe)
Source: "{#Assets}\Emoticons\*"; DestDir: "{app}\Emoticons"; Flags: recursesubdirs createallsubdirs ignoreversion
Source: "{#Assets}\Sounds\*"; DestDir: "{app}\Sounds"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
; the window is drawn by the CPU by default; this one tries the video card (OpenGL)
Name: "{group}\{#AppName} (OpenGL)"; Filename: "{app}\{#AppExe}"; Parameters: "--opengl"
Name: "{group}\Удалить {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
; replace an old rule (if any), then allow the program for UDP/TCP on private and domain networks;
; one elevated cmd (Verb: runas) so Windows asks for administrator rights only once
Filename: "{sys}\cmd.exe"; Parameters: "/c netsh advfirewall firewall delete rule name=""{#AppName}"" >nul & netsh advfirewall firewall add rule name=""{#AppName}"" dir=in action=allow program=""{app}\{#AppExe}"" enable=yes profile=private,domain"; Verb: "runas"; Flags: shellexec runhidden waituntilterminated; Tasks: firewall; Check: not IsAutoUpdate
Filename: "{app}\{#AppExe}"; Description: "Запустить {#AppName}"; Flags: nowait postinstall skipifsilent
; auto-update (src/update.rs runs "setup.exe /VERYSILENT ... /relaunch=1|2"): start the new
; version again; 2 = it was in the tray, start it there
Filename: "{app}\{#AppExe}"; Parameters: "{code:RelaunchParams}"; Flags: nowait; Check: IsAutoUpdate

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#AppName}"""; Verb: "runas"; Flags: shellexec runhidden waituntilterminated; Tasks: firewall; RunOnceId: "DelFirewallRule"

[Code]
function IsWin10OrLater: Boolean;
begin
  Result := GetWindowsVersion >= $0A000000;
end;

// started by the program's auto-update: no questions (the firewall rule is already there)
function IsAutoUpdate: Boolean;
begin
  Result := ExpandConstant('{param:relaunch|0}') <> '0';
end;

function RelaunchParams(Param: String): String;
begin
  // --delay: the program is single-instance, let the old copy finish closing first
  if ExpandConstant('{param:relaunch|0}') = '2' then
    Result := '--delay=500 --tray'
  else
    Result := '--delay=500';
end;

// Older versions were installed for all users (Program Files, with administrator rights) and
// start at logon for everybody. This installer works without those rights, so it cannot
// remove that copy itself: it offers to run the old uninstaller (Windows asks for the rights
// once). Settings and history are in %APPDATA%\NassiRS and stay.
function OldUninstaller(var Params: String): String;
var
  Inno, Nsis: String;
begin
  Inno := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F3A2C61-5B7E-4D2A-9C1F-3E6B7A9D0C42}_is1';
  Nsis := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\NassiRS';
  Result := '';
  Params := '/SILENT /SUPPRESSMSGBOXES /NORESTART';
  if RegQueryStringValue(HKLM, Inno, 'UninstallString', Result) then Exit;
  if IsWin64 and RegQueryStringValue(HKLM64, Inno, 'UninstallString', Result) then Exit;
  Params := '/S';
  if RegQueryStringValue(HKLM, Nsis, 'UninstallString', Result) then Exit;
  if IsWin64 and RegQueryStringValue(HKLM64, Nsis, 'UninstallString', Result) then Exit;
  Result := '';
end;

function InitializeSetup: Boolean;
var
  Uninst, Params: String;
  Code: Integer;
begin
  Result := True;
  Uninst := RemoveQuotes(OldUninstaller(Params));
  // an auto-update must not ask anything; the question comes with the next manual install
  if (Uninst = '') or WizardSilent or not FileExists(Uninst) then
    Exit;
  case MsgBox('Найдена старая копия Nassi RS, установленная для всех пользователей:' + #13#10 + ExtractFileDir(Uninst) + #13#10#13#10 +
      'Теперь программа ставится в одну папку пользователя и обновляется там же, без прав администратора. ' +
      'Старую копию лучше удалить, иначе при входе в Windows будут запускаться две программы. ' +
      'Windows один раз попросит права администратора. Настройки и переписка сохранятся.' + #13#10#13#10 +
      'Да: удалить старую копию и продолжить.' + #13#10 + 'Нет: оставить её и продолжить.', mbConfirmation, MB_YESNOCANCEL) of
    IDYES:
      if not ShellExec('runas', Uninst, Params, '', SW_SHOW, ewWaitUntilTerminated, Code) then
        MsgBox('Старую копию удалить не удалось (' + SysErrorMessage(Code) + '). Её можно удалить позже через «Параметры → Приложения».', mbError, MB_OK);
    IDCANCEL:
      Result := False;
  end;
end;

[Messages]
russian.WelcomeLabel2=Будет установлен [name/ver].%n%nЛокальный мессенджер (личный, общий и групповой чат, файлы) и удалённое администрирование для системного администратора.%n%nРекомендуется закрыть все другие программы перед продолжением.
