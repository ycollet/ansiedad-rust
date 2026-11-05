#ifndef AppVersion
  #error AppVersion must be supplied by the release workflow
#endif
#ifndef FileVersion
  #error FileVersion must be supplied by the release workflow
#endif

[Setup]
AppId={{AAE2748E-6FD7-4885-827A-BEDCEAFB1748}
AppName=ANSIedad
AppVersion={#AppVersion}
VersionInfoVersion={#FileVersion}
AppPublisher=ANSIedad contributors
DefaultDirName={localappdata}\Programs\ANSIedad
DefaultGroupName=ANSIedad
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\ansiedad.ico
OutputDir=..\dist
OutputBaseFilename=ansiedad-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\ansiedad-gui.exe
CloseApplications=yes

[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\ansiedad.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\x86_64-pc-windows-msvc\release\ansiedad-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\examples\*.txt"; DestDir: "{app}\examples"; Flags: ignoreversion

[Icons]
Name: "{group}\ANSIedad"; Filename: "{app}\ansiedad-gui.exe"
Name: "{group}\ANSIedad terminal"; Filename: "{app}\ansiedad.exe"
Name: "{group}\Uninstall ANSIedad"; Filename: "{uninstallexe}"
