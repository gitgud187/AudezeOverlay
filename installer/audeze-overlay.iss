#ifndef AppVersion
  #error AppVersion must be provided by tools/package.ps1
#endif
#ifndef BuildLabel
  #define BuildLabel AppVersion
#endif

[Setup]
AppId={{B0759B40-FCA4-4A47-B8D2-5C07B8FD5D5B}
AppName=Audeze Overlay
AppVersion={#AppVersion}
AppVerName=Audeze Overlay {#BuildLabel}
AppPublisher=Audeze Overlay contributors
DefaultDirName={localappdata}\Programs\Audeze Overlay
DefaultGroupName=Audeze Overlay
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
AppMutex=Local\AudezeOverlay.Singleton
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\Audeze Overlay.exe
SetupIconFile=..\assets\audeze.ico
LicenseFile=..\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=Audeze-Overlay-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
VersionInfoVersion={#AppVersion}

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#StageDir}\Audeze Overlay.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\CONTRIBUTING.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\docs\*.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#StageDir}\docs\screenshots\*.png"; DestDir: "{app}\docs\screenshots"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\DEPENDENCY-LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Audeze Overlay"; Filename: "{app}\Audeze Overlay.exe"; Parameters: "--open"; WorkingDir: "{app}"
Name: "{autodesktop}\Audeze Overlay"; Filename: "{app}\Audeze Overlay.exe"; Parameters: "--open"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\Audeze Overlay.exe"; Parameters: "--open"; Description: "Open Audeze Overlay"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  StartupCommand: String;
begin
  if CurUninstallStep = usUninstall then begin
    // Leave settings and startup entries belonging to other portable copies alone.
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'AudezeOverlay', StartupCommand) then
      if Pos(Lowercase('"' + ExpandConstant('{app}\Audeze Overlay.exe') + '"'), Lowercase(StartupCommand)) = 1 then
        RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'AudezeOverlay');
  end;
end;
