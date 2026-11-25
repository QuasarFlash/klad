#define Version GetEnv("KLAD_VERSION")
#define Numeric GetEnv("KLAD_NUMERIC")
#define Stage GetEnv("KLAD_STAGE")
#define Out GetEnv("KLAD_OUT")

[Setup]
AppId={{079E2BD6-EB52-4773-9906-FD96CB485E0F}
AppName=klad
AppVersion={#Version}
AppVerName=klad {#Version}
AppPublisher=Indicode
AppPublisherURL=https://klad.studio
AppSupportURL=https://klad.studio
VersionInfoVersion={#Numeric}
DefaultDirName={autopf}\klad
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#Out}
OutputBaseFilename=klad-{#Version}-windows-x86_64-setup
SetupIconFile=..\icon\klad.ico
UninstallDisplayIcon={app}\klad-editor.exe
UninstallDisplayName=klad
LicenseFile={#Stage}\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
CloseApplications=yes

[Tasks]
Name: path; Description: "Put the klad command on the PATH, so agents and terminals find it"
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\klad"; Filename: "{app}\klad-editor.exe"
Name: "{autodesktop}\klad"; Filename: "{app}\klad-editor.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: path; Check: NotOnPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\klad-editor.exe"; Description: "{cm:LaunchProgram,klad}"; Flags: nowait postinstall skipifsilent

[Code]
function OnPath(Path, Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') > 0;
end;

function NotOnPath(Dir: string): Boolean;
var
  Path: string;
begin
  if RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    Result := not OnPath(Path, Dir)
  else
    Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir: string;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  Dir := ExpandConstant('{app}');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    exit;
  if not OnPath(Path, Dir) then
    exit;
  Path := ';' + Path + ';';
  StringChangeEx(Path, ';' + Dir + ';', ';', True);
  while (Length(Path) > 0) and (Path[1] = ';') do
    Delete(Path, 1, 1);
  while (Length(Path) > 0) and (Path[Length(Path)] = ';') do
    Delete(Path, Length(Path), 1);
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path);
end;
