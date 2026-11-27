#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif
#ifndef WinVersion
  #define WinVersion "0.3.0.0"
#endif
#ifndef BinaryPath
  #define BinaryPath "..\..\target\x86_64-pc-windows-msvc\release\ProjPanel.exe"
#endif
#ifndef ReadmePath
  #define ReadmePath "..\..\README.md"
#endif
#ifndef IconPath
  #define IconPath "..\..\target\packaging\app-icon.ico"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "ProjPanel-windows-x86_64-setup"
#endif

[Setup]
AppId={{8E9B3F24-4A71-4D2C-9E6F-3C82B7D51A09}
AppName=ProjPanel
AppVersion={#AppVersion}
AppVerName=ProjPanel {#AppVersion}
AppPublisher=ProjPanel
AppPublisherURL=https://github.com/Ariza-ye/proj-panel
AppSupportURL=https://github.com/Ariza-ye/proj-panel/issues
VersionInfoVersion={#WinVersion}
DefaultDirName={autopf}\ProjPanel
DefaultGroupName=ProjPanel
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
SetupIconFile={#IconPath}
UninstallDisplayIcon={app}\ProjPanel.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinaryPath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReadmePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\ProjPanel"; Filename: "{app}\ProjPanel.exe"
Name: "{autodesktop}\ProjPanel"; Filename: "{app}\ProjPanel.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ProjPanel.exe"; Description: "{cm:LaunchProgram,ProjPanel}"; Flags: nowait postinstall skipifsilent
