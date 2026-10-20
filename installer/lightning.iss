; Lightning installer (Inno Setup 6). Built by scripts\release.ps1, which
; passes the version, exe and icon:
;   ISCC /DAppVersion=0.1.0 /DSourceExe=... /DIconFile=... installer\lightning.iss
;
; Per-user install: no admin prompt, lives in %LOCALAPPDATA%\Programs\Lightning.

#ifndef AppVersion
  #error AppVersion must be defined (run scripts\release.ps1)
#endif

[Setup]
; Never change AppId: it is how upgrades find the existing install.
AppId={{064FAF4F-A23A-431B-A377-EE73A59D020D}
AppName=Lightning
AppVersion={#AppVersion}
AppVerName=Lightning {#AppVersion}
AppPublisher=Lightning
DefaultDirName={localappdata}\Programs\Lightning
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=Lightning-Setup-{#AppVersion}
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\lightning.exe
UninstallDisplayName=Lightning
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; The app is closed by [Code] below; don't also ask via Restart Manager.
CloseApplications=no

[Tasks]
Name: "startup"; Description: "Start Lightning when I sign in"; GroupDescription: "Options:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Options:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "lightning.exe"; Flags: ignoreversion
Source: "..\crates\lightning\assets\OFL.txt"; DestDir: "{app}"; DestName: "WorkSans-OFL.txt"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Lightning"; Filename: "{app}\lightning.exe"
Name: "{userdesktop}\Lightning"; Filename: "{app}\lightning.exe"; Tasks: desktopicon

[Registry]
; Same value the app's "Launch at startup" toggle writes, so the two agree.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Lightning"; ValueData: """{app}\lightning.exe"""; Tasks: startup; Flags: uninsdeletevalue

[Run]
Filename: "{app}\lightning.exe"; Description: "Launch Lightning (press Alt+Space)"; Flags: postinstall nowait skipifsilent

[Code]
// Lightning lives in the tray, so make sure no copy is running while its
// exe is replaced or removed.
procedure StopLightning();
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM lightning.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(300);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopLightning();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopLightning();
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  // The app's own startup toggle may have created the entry too.
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Lightning');
end;
