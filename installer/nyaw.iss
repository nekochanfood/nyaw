; nyaw のインストーラ。`cargo build --release` の後に `iscc installer\nyaw.iss`（CI は release.yml）
; 保存先が %LOCALAPPDATA%\nyaw なので、管理者権限なしでユーザー単位に入れる
; アプリの更新（crates/updater）は `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /WAITPID=<pid> /RELAUNCH=1` で走らせる

; 版はリリースのワークフローがタグから渡す（NYAW_VERSION。`2026.0.1-beta.1` や `2026.0.1-patch.1` など）。無ければ Cargo.toml のもの。
; pkgid は `path+file:///…#2026.0.1`（ディレクトリ名が違うと `#nyaw@2026.0.1`）
#define AppVersion GetEnv("NYAW_VERSION")
#if AppVersion == ""
  #define PkgId ExecAndGetFirstLine("cmd.exe", "/c cargo pkgid -p nyaw", SourcePath + "..")
  #if PkgId == ""
    #error cargo pkgid -p nyaw が失敗した
  #endif
  #define AppVersion Copy(PkgId, RPos("#", PkgId) + 1)
  #if RPos("@", AppVersion) > 0
    #define AppVersion Copy(AppVersion, RPos("@", AppVersion) + 1)
  #endif
#endif
; ビルド番号 = HEAD までのコミット数（crates/updater/build.rs と同じ数え方）。アプリの更新が新旧をこれで比べるので名前に入れる
#define Build ExecAndGetFirstLine("cmd.exe", "/c git rev-list --count HEAD", SourcePath + "..")
#if Build == ""
  #error git rev-list --count HEAD が失敗した
#endif

[Setup]
; 変えるとアップデートが別アプリ扱いになる
AppId={{53D987D4-058F-43C8-89EF-DCAC399771A0}
AppName=nyaw
AppVersion={#AppVersion}
AppPublisher=nekochanfood
AppPublisherURL=https://github.com/nekochanfood/nyaw
DefaultDirName={localappdata}\Programs\nyaw
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\nyaw.exe
SetupIconFile=..\assets\icons\nyaw-light.ico
OutputDir=..\target\installer
; アプリの更新はこの名前から版とビルド番号を読む（crates/updater の parse_installer_name）
OutputBaseFilename=nyaw-{#AppVersion}-build{#Build}-setup
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
Source: "..\target\release\nyaw.exe"; DestDir: "{app}"; Flags: ignoreversion
; Steam Audio。`cargo build` で crates/subsystems/audio/build.rs が置く。無ければ（落とせなかった）ここで止まる
Source: "..\target\release\phonon.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\steamaudio-THIRDPARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"
Source: "..\LICENSE-APACHE"; DestDir: "{app}"

; SteamVR に登録した vrmanifest（crates/subsystems/vr/AGENT.md の「SteamVR のアプリ登録」）。消えた nyaw を SteamVR が自動起動しないように
[UninstallDelete]
Type: files; Name: "{localappdata}\nyaw\openvr\nekochanfood.nyaw.vrmanifest"

[Icons]
Name: "{autoprograms}\nyaw"; Filename: "{app}\nyaw.exe"

[Run]
Filename: "{app}\nyaw.exe"; Description: "{cm:LaunchProgram,nyaw}"; Flags: nowait postinstall skipifsilent
; アプリの更新は、入れ終えたらアプリを起動し直す
Filename: "{app}\nyaw.exe"; Flags: nowait; Check: Relaunch

[Code]
const
  SYNCHRONIZE = $00100000;

function OpenProcess(Access: Integer; Inherit: Integer; Pid: Integer): Integer;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForSingleObject(Handle: Integer; Milliseconds: Integer): Integer;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: Integer): Integer;
  external 'CloseHandle@kernel32.dll stdcall';

// アプリの更新（/WAITPID=<pid>）は、起こしたアプリが終わるのを待ってから入れ替える（60 秒まで。過ぎたら閉じてもらう）
function InitializeSetup: Boolean;
var
  Process: Integer;
begin
  Process := OpenProcess(SYNCHRONIZE, 0, StrToIntDef(ExpandConstant('{param:WAITPID|0}'), 0));
  if Process <> 0 then
  begin
    WaitForSingleObject(Process, 60000);
    CloseHandle(Process);
  end;
  Result := True;
end;

function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;
