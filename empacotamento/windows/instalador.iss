; Script Inno Setup para Instalador Windows 10/11 (64 bits) — Divisor de Vídeo
#define AppName "Divisor de Vídeo"
#define AppVersion "2.0.0"
#define AppPublisher "Equipe de Engenharia"
#define AppExeName "divisor_video_gui.exe"

[Setup]
AppId={{7D9E4C52-9B12-4F5A-8B8E-3C1E84D20001}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\DivisorVideo
DefaultGroupName={#AppName}
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
OutputDir=..\..\dist\windows-x64
OutputBaseFilename=DivisorVideo-Windows-x64-Setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
MinVersion=10.0.17763

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na Área de Trabalho"; GroupDescription: "Atalhos adicionais:"

[Files]
Source: "..\..\dist\windows-x64\DivisorVideo-Windows-x64-Portable\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\Desinstalar {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Abrir {#AppName}"; Flags: nowait postinstall skipifsilent
