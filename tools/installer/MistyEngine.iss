; Misty Engine Windows installer (Inno Setup 6).
; Built by tools\make_release.ps1, which passes:
;   /DVersion=0.0.1  /DNumericVersion=0.0.1.0  /DPkgDir=<the unzipped release folder>
;   /DIconFile=<misty.ico>  /O<out dir>
; Per-user install by default (no admin prompt) so the in-app updater can write to the folder.

#ifndef Version
  #error Pass /DVersion=x.y.z
#endif
#ifndef PkgDir
  #error Pass /DPkgDir=<release folder>
#endif
#ifndef IconFile
  #error Pass /DIconFile=<misty.ico>
#endif
; Four numbers for the installer's version info; plain versions (1.2.3) work as they are.
#ifndef NumericVersion
  #define NumericVersion Version
#endif

[Setup]
; Fixed id: lets later installers upgrade the same install. Never change it.
AppId={{6F1C2B7E-4A3D-4E8B-9C55-2D7A1E0B8F31}
AppName=Misty Engine
AppVersion={#Version}
AppVerName=Misty Engine {#Version}
AppPublisher=Misty Engine
AppCopyright=Copyright (C) Misty Engine contributors. GPL-3.0-or-later with runtime exception.
AppPublisherURL=https://github.com/Aalzard/MistyEngine
AppSupportURL=https://github.com/Aalzard/MistyEngine/issues
AppUpdatesURL=https://github.com/Aalzard/MistyEngine/releases
DefaultDirName={autopf}\Misty Engine
DefaultGroupName=Misty Engine
DisableProgramGroupPage=yes
; Current user by default; the dialog lets people pick "all users" (needs admin).
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#PkgDir}\LICENSE.txt
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\MistyEngine.exe
UninstallDisplayName=Misty Engine
OutputBaseFilename=MistyEngine-Setup-v{#Version}-windows-x64
; Version info of the Setup exe, the same values as MistyEngine.exe (crates\misty_editor\build_icon.rs).
; The release signing checks the product name and version (tools\release\signpath\installer.xml).
VersionInfoCompany=Misty Engine
VersionInfoDescription=Misty Engine Setup
VersionInfoProductName=Misty Engine
VersionInfoVersion={#NumericVersion}
VersionInfoTextVersion={#Version}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#Version}
VersionInfoCopyright=Copyright (C) Misty Engine contributors. GPL-3.0-or-later with runtime exception.
VersionInfoOriginalFileName=MistyEngine-Setup-v{#Version}-windows-x64.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Close a running editor before replacing its files.
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#PkgDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Misty Engine"; Filename: "{app}\MistyEngine.exe"
Name: "{group}\Uninstall Misty Engine"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Misty Engine"; Filename: "{app}\MistyEngine.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\MistyEngine.exe"; Description: "{cm:LaunchProgram,Misty Engine}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Files the engine or the updater create next to the install; user projects elsewhere are never touched.
Type: filesandordirs; Name: "{app}\Examples\SampleProject\Library"
Type: filesandordirs; Name: "{app}\Examples\SampleProject\Builds"
Type: filesandordirs; Name: "{app}\.misty-update-backup"
Type: files; Name: "{app}\*.old.exe"
