#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef PackageDir
  #error PackageDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif

[Setup]
AppId={{7C1D4752-D1B8-441D-8293-C92E9DEBF5A5}
AppName=ShipForge Workspace
AppVersion={#AppVersion}
AppPublisher=ShipForge
AppPublisherURL=https://github.com/Hubujiu/ShipForge
AppSupportURL=https://github.com/Hubujiu/ShipForge/issues
AppUpdatesURL=https://github.com/Hubujiu/ShipForge/releases
DefaultDirName={code:DefaultDirectory}
DefaultGroupName=ShipForge Workspace
DisableDirPage=no
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir={#OutputDir}
OutputBaseFilename=shipforge-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
UsePreviousAppDir=yes
UninstallDisplayIcon={app}\shipforge-desktop.exe
Uninstallable=not VerificationMode
CreateUninstallRegKey=not VerificationMode
SetupMutex=ShipForge.Workspace.Setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#PackageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[INI]
Filename: "{app}\shipforge-install.ini"; Section: "Installation"; Key: "format"; String: "1"; Flags: uninsdeleteentry
Filename: "{app}\shipforge-install.ini"; Section: "Installation"; Key: "version"; String: "{#AppVersion}"; Flags: uninsdeleteentry
Filename: "{app}\shipforge-install.ini"; Section: "Installation"; Key: "data_dir"; String: "{param:SHIPFORGEDATA|}"; Flags: uninsdeleteentry uninsdeletesectionifempty; Check: HasDataDirectory

[UninstallDelete]
Type: files; Name: "{app}\shipforge-install.ini"

[Icons]
Name: "{userprograms}\ShipForge Workspace"; Filename: "{app}\shipforge-desktop.exe"; Parameters: "{code:DataArguments}"; WorkingDir: "{app}"; Check: not VerificationMode
Name: "{userdesktop}\ShipForge Workspace"; Filename: "{app}\shipforge-desktop.exe"; Parameters: "{code:DataArguments}"; WorkingDir: "{app}"; Tasks: desktopicon; Check: not VerificationMode

[Run]
Filename: "{app}\shipforge-desktop.exe"; Parameters: "{code:DataArguments}"; Description: "Launch ShipForge"; Flags: nowait postinstall skipifsilent runasoriginaluser; Check: NormalLaunch
Filename: "{app}\shipforge-desktop.exe"; Parameters: "{code:RestartArguments}"; Flags: nowait runasoriginaluser; Check: RestartRequested

[Code]
function VerificationMode: Boolean;
begin
  Result := ExpandConstant('{param:VERIFYINSTALL|0}') = '1';
end;

function RestartRequested: Boolean;
begin
  Result := (not VerificationMode) and (ExpandConstant('{param:RESTARTAPP|0}') = '1');
end;

function NormalLaunch: Boolean;
begin
  Result := (not VerificationMode) and (not RestartRequested);
end;

function DefaultDirectory(Param: String): String;
begin
  Result := ExpandConstant('{localappdata}\Programs\ShipForge Workspace');
end;

function HasDataDirectory: Boolean;
begin
  Result := ExpandConstant('{param:SHIPFORGEDATA|}') <> '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var Marker: String; Bom: AnsiString;
begin
  if CurStep = ssInstall then begin
    Marker := ExpandConstant('{app}\shipforge-install.ini');
    if not FileExists(Marker) then begin
      ForceDirectories(ExpandConstant('{app}'));
      { Unicode INI avoids losing a custom data path outside the system code page. }
      SetLength(Bom, 2);
      Bom[1] := Chr($FF);
      Bom[2] := Chr($FE);
      if not SaveStringToFile(Marker, Bom, False) then
        RaiseException('Could not create installation marker.');
    end;
  end;
end;

function DataArguments(Param: String): String;
var DataDir: String;
begin
  Result := '';
  DataDir := ExpandConstant('{param:SHIPFORGEDATA|}');
  if DataDir = '' then
    DataDir := GetIniString('Installation', 'data_dir', '', ExpandConstant('{app}\shipforge-install.ini'));
  if DataDir <> '' then Result := '--data-dir "' + DataDir + '"';
end;

function RestartArguments(Param: String): String;
begin
  Result := DataArguments('');
  if ExpandConstant('{param:DEMO|0}') = '1' then begin
    Result := '--demo';
  end;
end;

function OpenProcess(Access: LongWord; Inherit: Boolean; ProcessId: LongWord): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForSingleObject(Handle: THandle; Milliseconds: LongWord): LongWord;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function PrepareToInstall(var NeedsRestart: Boolean): String;
var Pid: Integer; Handle: THandle;
begin
  Result := '';
  Pid := StrToIntDef(ExpandConstant('{param:WAITPID|0}'), 0);
  if Pid <> 0 then begin
    Handle := OpenProcess($00100000, False, Pid);
    if Handle <> 0 then begin
      if WaitForSingleObject(Handle, 30000) <> 0 then
        Result := 'Please close ShipForge before installing. Your project data has not been changed.';
      CloseHandle(Handle);
    end;
  end;
end;
