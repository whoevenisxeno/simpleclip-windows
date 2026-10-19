; Inno Setup script for the SimpleClip Windows installer.
; Built by packaging\windows\build.ps1:  ISCC /DAppVersion=4.0.0 /DSourceDir=<dist\SimpleClip> simpleclip.iss
; Per-user install (no admin prompt), shows up in Settings > Apps, and uninstalls cleanly.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\SimpleClip"
#endif

[Setup]
AppId={{6C1F2D7E-4A6B-4C55-9E2B-5D3B8A1F0C11}
AppName=SimpleClip
AppVersion={#AppVersion}
AppVerName=SimpleClip {#AppVersion}
AppPublisher=SimpleClip contributors
AppPublisherURL=https://github.com/whoevenisxeno/SimpleClip
DefaultDirName={autopf}\SimpleClip
DefaultGroupName=SimpleClip
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
SetupIconFile=..\..\assets\simpleclip.ico
UninstallDisplayIcon={app}\sc-gui.exe
UninstallDisplayName=SimpleClip
OutputDir=..\..\dist
OutputBaseFilename=SimpleClip-{#AppVersion}-Setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE
ChangesEnvironment=yes
CloseApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start SimpleClip when I sign in (recommended: the replay buffer only works while it runs)"; GroupDescription: "Startup:"
Name: "addpath"; Description: "Add the sc command to my PATH"; GroupDescription: "Command line:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Excludes: "*.ps1"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{group}\SimpleClip"; Filename: "{app}\sc-gui.exe"; WorkingDir: "{app}"
Name: "{group}\Uninstall SimpleClip"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SimpleClip"; Filename: "{app}\sc-gui.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
; Start the daemon at sign-in (the tray menu can toggle this later).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "SimpleClip"; ValueData: """{app}\scd.exe"""; Flags: uninsdeletevalue; Tasks: autostart
; Identity for toast notifications, so they say "SimpleClip" instead of PowerShell.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\SimpleClip"; ValueType: string; ValueName: "DisplayName"; ValueData: "SimpleClip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\SimpleClip"; ValueType: dword; ValueName: "ShowInSettings"; ValueData: "0"

[Run]
Filename: "{app}\scd.exe"; Description: "Start the SimpleClip daemon"; Flags: nowait runhidden skipifsilent postinstall
Filename: "{app}\sc-gui.exe"; Description: "Open SimpleClip"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\sc.exe"; Parameters: "quit"; Flags: runhidden; RunOnceId: "StopDaemon"

[Code]
const
  EnvKey = 'Environment';

// Stop a running daemon (old version) before files are replaced or removed.
procedure StopDaemon(const AppDir: String);
var
  ResultCode: Integer;
begin
  if FileExists(AppDir + '\sc.exe') then
  begin
    Exec(AppDir + '\sc.exe', 'quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(800);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopDaemon(ExpandConstant('{app}'));
  Result := '';
end;

function PathHasDir(const Dir: String): Boolean;
var
  Paths: String;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    Paths := '';
  Result := Pos(';' + Lowercase(Dir) + ';', ';' + Lowercase(Paths) + ';') > 0;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Paths: String;
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addpath') then
  begin
    if not PathHasDir(ExpandConstant('{app}')) then
    begin
      if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
        Paths := '';
      if (Paths <> '') and (Copy(Paths, Length(Paths), 1) <> ';') then
        Paths := Paths + ';';
      RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths + ExpandConstant('{app}'));
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Paths, Dir: String;
  P: Integer;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Dir := ExpandConstant('{app}');
    if RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    begin
      P := Pos(';' + Lowercase(Dir), ';' + Lowercase(Paths));
      if P > 0 then
      begin
        Delete(Paths, P, Length(Dir) + 1);
        RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths);
      end;
    end;
  end;
end;
