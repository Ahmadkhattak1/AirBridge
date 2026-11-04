#ifndef PayloadDir
  #error PayloadDir is required
#endif
[Setup]
AppId={{F8DCD771-3245-48B6-9845-9330D00D33FA}
AppName=AirBridge
AppVersion=0.2.0
AppPublisher=AirBridge contributors
AppPublisherURL=https://github.com/Ahmadkhattak1/AirBridge
DefaultDirName={localappdata}\Programs\AirBridge
DefaultGroupName=AirBridge
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
OutputBaseFilename=AirBridge-0.2.0-Setup-x64
OutputDir={#PayloadDir}\..
UninstallDisplayIcon={app}\AirBridge.exe
CloseApplications=yes
RestartApplications=no
LicenseFile=..\LICENSE
InfoBeforeFile=INSTALL-NOTES.txt
[Files]
Source: "{#PayloadDir}\AirBridge.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\airbridge-runtime.tar.gz"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\runtime-manifest.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\THIRD-PARTY-LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "Setup-Runtime.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "dependencies.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
[Icons]
Name: "{group}\AirBridge"; Filename: "{app}\AirBridge.exe"
Name: "{group}\Repair AirBridge setup"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\Setup-Runtime.ps1"" -InstallRoot ""{app}"""
[Run]
Filename: "{app}\AirBridge.exe"; Description: "Open AirBridge"; Flags: nowait postinstall skipifsilent; Check: RuntimeReady
[Code]
var
  SetupSucceeded: Boolean;
  RestartRequired: Boolean;
function RuntimeReady: Boolean;
begin
  Result := SetupSucceeded;
end;
function NeedsRestart: Boolean;
begin
  Result := RestartRequired;
end;
procedure CurStepChanged(CurStep: TSetupStep);
var
  ExitCode: Integer;
  MessageText: AnsiString;
begin
  if CurStep = ssPostInstall then begin
    WizardForm.StatusLabel.Caption := 'Preparing WSL and USB support. This can take several minutes.';
    if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ExpandConstant('{app}\Setup-Runtime.ps1') + '" -InstallRoot "' + ExpandConstant('{app}') + '"',
      '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
      RaiseException('Could not start runtime setup. Run Setup again.');
    SetupSucceeded := ExitCode = 0;
    RestartRequired := ExitCode = 3010;
    if not SetupSucceeded then begin
      if not LoadStringFromFile(ExpandConstant('{localappdata}\AirBridge\setup-result.txt'), MessageText) then
        MessageText := 'Runtime setup did not finish. Run Setup again.';
      if RestartRequired then
        MsgBox(String(MessageText), mbInformation, MB_OK)
      else
        RaiseException(String(MessageText));
    end;
  end;
end;
