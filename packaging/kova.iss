#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef StageDir
  #error StageDir is required
#endif

[Setup]
AppId={{38810BA3-E7A8-4470-88D1-3192B752D96D}
AppName=Kova
AppVersion={#AppVersion}
AppPublisher=Kova Contributors
AppPublisherURL=https://github.com/cubiix3/Kova-File-Manager
AppSupportURL=https://github.com/cubiix3/Kova-File-Manager/issues
AppUpdatesURL=https://github.com/cubiix3/Kova-File-Manager/releases
DefaultDirName={localappdata}\Kova
DisableDirPage=yes
DefaultGroupName=Kova
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=Kova-Setup-{#AppVersion}-x64
SetupIconFile=..\apps\kova-desktop\assets\kova.ico
UninstallDisplayIcon={app}\Kova.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
LicenseFile=..\LICENSE-MIT

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Kova"; Filename: "{app}\Kova.exe"
Name: "{userdesktop}\Kova"; Filename: "{app}\Kova.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Kova.exe"; Description: "Launch Kova"; Flags: nowait postinstall skipifsilent

[Code]
function InitializeUninstall(): Boolean;
var
  Code: Integer;
  Verb: String;
begin
  Result := True;
  if (CompareText(ExpandConstant('{app}'), ExpandConstant('{localappdata}\Kova')) = 0)
    and not FileExists(ExpandConstant('{localappdata}\Kova\folder-associations.json')) then
  begin
    if (RegQueryStringValue(HKCU, 'Software\Classes\Directory\shell', '', Verb) and (Verb = 'Kova.OpenFolder'))
      or (RegQueryStringValue(HKCU, 'Software\Classes\Drive\shell', '', Verb) and (Verb = 'Kova.OpenFolder')) then
    begin
      MsgBox('The folder integration backup is missing. Restore the Windows folder defaults before removing Kova.', mbError, MB_OK);
      Result := False;
      Exit;
    end;
  end;
  { Only the stable installation owns the optional folder integration. }
  if (CompareText(ExpandConstant('{app}'), ExpandConstant('{localappdata}\Kova')) = 0)
    and FileExists(ExpandConstant('{localappdata}\Kova\folder-associations.json')) then
  begin
    Result := Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' +
      ExpandConstant('{app}\default-file-manager.ps1') + '" -Mode Restore',
      '', SW_HIDE, ewWaitUntilTerminated, Code);
    if Result then Result := Code = 0;
    if not Result then
      MsgBox('Kova could not restore the previous folder defaults. Uninstall has stopped. Restore folder integration from the Kova logo menu, then try again.', mbError, MB_OK);
  end;
end;
