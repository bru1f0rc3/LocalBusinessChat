; Nassi RS installer (Inno Setup 6.3+)
; One setup.exe for all supported systems: the installer itself is 32-bit, runs on
; Windows 7 SP1 ... 11 (x86 and x64) and installs the matching build of the program:
;   64-bit Windows 10/11        -> x86_64-pc-windows-msvc
;   64-bit Windows 7 SP1 / 8.x  -> x86_64-win7-windows-msvc
;   32-bit Windows 7 SP1 ... 11 -> i686-win7-windows-msvc
;
; Build: build-installer.ps1 (compiles both targets, then runs ISCC on this file)

#define AppName "Nassi RS"
#ifndef AppVersion
  #define AppVersion "0.2.0"
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
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; x86compatible: runs on 32-bit Windows; x64compatible: on 64-bit installs the x64 build into Program Files
ArchitecturesAllowed=x86compatible x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
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
Name: "autostart"; Description: "Запускать Nassi RS при входе в Windows (для всех пользователей)"; GroupDescription: "Дополнительно:"
Name: "firewall"; Description: "Разрешить Nassi RS в брандмауэре Windows (нужно для работы в сети)"; GroupDescription: "Дополнительно:"

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
; escape hatch for PCs where OpenGL draws artefacts: CPU rendering, no video driver needed
Name: "{group}\{#AppName} (программная отрисовка)"; Filename: "{app}\{#AppExe}"; Parameters: "--software"
Name: "{group}\Удалить {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
; replace an old rule (if any), then allow the program for UDP/TCP on private and domain networks
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#AppName}"""; Flags: runhidden; Tasks: firewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""{#AppName}"" dir=in action=allow program=""{app}\{#AppExe}"" enable=yes profile=private,domain"; Flags: runhidden; Tasks: firewall
Filename: "{app}\{#AppExe}"; Description: "Запустить {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#AppName}"""; Flags: runhidden; RunOnceId: "DelFirewallRule"

[Code]
function IsWin10OrLater: Boolean;
begin
  Result := GetWindowsVersion >= $0A000000;
end;

[Messages]
russian.WelcomeLabel2=Будет установлен [name/ver].%n%nЛокальный мессенджер (личный, общий и групповой чат, файлы) и удалённое администрирование для системного администратора.%n%nРекомендуется закрыть все другие программы перед продолжением.
