; SliceCraft installer for Windows, built with Inno Setup 6.
; Run through `cargo xtask dist`, which stages the files and passes
; AppVersion, SourceDir, OutputDir and OutputName.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\dist\stage"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\dist"
#endif
#ifndef OutputName
  #define OutputName "slicecraft-setup"
#endif

[Setup]
; the id ties upgrades and the uninstaller together: never change it
AppId={{E09FDD65-9D68-4096-A795-FD861C5B43AE}
AppName=SliceCraft
AppVersion={#AppVersion}
AppVerName=SliceCraft {#AppVersion}
AppPublisher=The SliceCraft contributors
AppPublisherURL=https://github.com/techartdev/slicecraft
AppSupportURL=https://github.com/techartdev/slicecraft/issues
AppUpdatesURL=https://github.com/techartdev/slicecraft/releases
DefaultDirName={autopf}\SliceCraft
DefaultGroupName=SliceCraft
DisableProgramGroupPage=yes
; installs for the current user without asking for administrator rights;
; the first page offers "for all users" too
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile={#SourceDir}\LICENSE-MIT
SetupIconFile={#SourceDir}\slicecraft.ico
UninstallDisplayIcon={app}\slicecraft.ico
UninstallDisplayName=SliceCraft {#AppVersion}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assoc"; Description: "Open SliceCraft projects (.scproj) with SliceCraft, and offer it for STL, OBJ and 3MF models"; GroupDescription: "File types:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\SliceCraft"; Filename: "{app}\slicecraft.exe"; IconFilename: "{app}\slicecraft.ico"
Name: "{autodesktop}\SliceCraft"; Filename: "{app}\slicecraft.exe"; IconFilename: "{app}\slicecraft.ico"; Tasks: desktopicon

[Registry]
; projects open in SliceCraft
Root: HKA; Subkey: "Software\Classes\.scproj"; ValueType: string; ValueName: ""; ValueData: "SliceCraft.Project"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\SliceCraft.Project"; ValueType: string; ValueName: ""; ValueData: "SliceCraft project"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\SliceCraft.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\slicecraft.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\SliceCraft.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\slicecraft.exe"" ""%1"""; Tasks: assoc
; models: SliceCraft is offered under "Open with", nothing is taken over
Root: HKA; Subkey: "Software\Classes\SliceCraft.Model"; ValueType: string; ValueName: ""; ValueData: "3D model"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\SliceCraft.Model\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\slicecraft.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\SliceCraft.Model\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\slicecraft.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.stl\OpenWithProgids"; ValueType: string; ValueName: "SliceCraft.Model"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.obj\OpenWithProgids"; ValueType: string; ValueName: "SliceCraft.Model"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.3mf\OpenWithProgids"; ValueType: string; ValueName: "SliceCraft.Model"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc

[Run]
Filename: "{app}\slicecraft.exe"; Description: "{cm:LaunchProgram,SliceCraft}"; Flags: nowait postinstall skipifsilent
