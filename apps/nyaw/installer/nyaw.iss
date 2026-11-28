; nyaw のインストーラー。`cargo build --release` の後に `iscc installer\nyaw.iss`（CI は release.yml）
; インストール先が %LOCALAPPDATA%\nyaw のため、管理者権限なしでユーザー単位にインストールする
; アプリの更新（crates/updater）は `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /WAITPID=<pid> /RELAUNCH=1 "/ARGS=<引数>"` で実行する

; バージョンはリリースワークフローがタグから渡す（NYAW_VERSION。`2026.0.1-beta.1` や `2026.0.1-patch.1` など）。未設定であれば Cargo.toml の値。
; pkgid は `path+file:///…#2026.0.1`（ディレクトリ名が違うと `#nyaw@2026.0.1`）
#define AppVersion GetEnv("NYAW_VERSION")
#if AppVersion == ""
  #define PkgId ExecAndGetFirstLine("cmd.exe", "/c cargo pkgid -p nyaw", SourcePath + "..")
  #if PkgId == ""
    #error cargo pkgid -p nyaw が失敗
  #endif
  #define AppVersion Copy(PkgId, RPos("#", PkgId) + 1)
  #if RPos("@", AppVersion) > 0
    #define AppVersion Copy(AppVersion, RPos("@", AppVersion) + 1)
  #endif
#endif

[Setup]
; 変更するとアップデートが別アプリとして扱われる
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
SetupIconFile=..\..\..\assets\icons\nyaw-light.ico
OutputDir=..\target\installer
; アプリの更新はリリースに添付されたこの名前のファイルをインストールする（crates/updater の is_installer）
OutputBaseFilename=nyaw-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
Source: "..\target\release\nyaw.exe"; DestDir: "{app}"; Flags: ignoreversion
; Steam Audio。`cargo build` で crates/subsystems/audio/build.rs が配置する。存在しなければ（ダウンロード失敗時）ここでエラーになる
Source: "..\target\release\phonon.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\steamaudio-THIRDPARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\..\LICENSE-GPL"; DestDir: "{app}"
Source: "..\..\..\LICENSE-EXCEPTION"; DestDir: "{app}"
Source: "..\..\..\LICENSE-BRAND"; DestDir: "{app}"
Source: "..\..\..\LICENSE-MIT"; DestDir: "{app}"
Source: "..\..\..\LICENSE-APACHE"; DestDir: "{app}"

; SteamVR に登録した vrmanifest（crates/subsystems/vr/AGENT.md の「SteamVR のアプリ登録」）。削除された nyaw を SteamVR が自動起動しないように
[UninstallDelete]
Type: files; Name: "{localappdata}\nyaw\openvr\nekochanfood.nyaw.vrmanifest"

[Icons]
Name: "{autoprograms}\nyaw"; Filename: "{app}\nyaw.exe"

[Run]
Filename: "{app}\nyaw.exe"; Description: "{cm:LaunchProgram,nyaw}"; Flags: nowait postinstall skipifsilent
; アプリの更新は、インストール完了後にアプリを元の引数（`--novr` など）で再起動する
Filename: "{app}\nyaw.exe"; Parameters: "{param:ARGS|}"; Flags: nowait; Check: Relaunch

[Code]
const
  SYNCHRONIZE = $00100000;

// ponytail: ハンドルを Integer で受けるのは Setup が 32 ビット（Inno Setup 6）だから。64 ビットの Setup にするなら NativeInt に

function OpenProcess(Access: Integer; Inherit: Integer; Pid: Integer): Integer;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForSingleObject(Handle: Integer; Milliseconds: Integer): Integer;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: Integer): Integer;
  external 'CloseHandle@kernel32.dll stdcall';

// アプリの更新（/WAITPID=<pid>）は、起動元アプリの終了を待ってからインストールする（最大 60 秒。超過時はユーザーに終了を要求する）
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
