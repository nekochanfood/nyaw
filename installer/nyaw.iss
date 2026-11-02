; nyaw のインストーラ。`cargo build --release` の後に `iscc installer\nyaw.iss`（CI は release.yml）
; 保存先が %LOCALAPPDATA%\nyaw なので、管理者権限なしでユーザー単位に入れる

; バージョンは Cargo.toml だけが持つ。pkgid は `path+file:///…#2026.0.1`（ディレクトリ名が違うと `#nyaw@2026.0.1`）
#define PkgId ExecAndGetFirstLine("cmd.exe", "/c cargo pkgid -p nyaw", SourcePath + "..")
#if PkgId == ""
  #error cargo pkgid -p nyaw が失敗した
#endif
#define AppVersion Copy(PkgId, RPos("#", PkgId) + 1)
#if RPos("@", AppVersion) > 0
  #define AppVersion Copy(AppVersion, RPos("@", AppVersion) + 1)
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
OutputDir=..\target\installer
OutputBaseFilename=nyaw-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
Source: "..\target\release\nyaw.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"
Source: "..\LICENSE-APACHE"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\nyaw"; Filename: "{app}\nyaw.exe"

[Run]
Filename: "{app}\nyaw.exe"; Description: "{cm:LaunchProgram,nyaw}"; Flags: nowait postinstall skipifsilent
