//! 実ランタイム（Monado / SteamVR）を E2E テストのために用意する。Playwright の
//! `npx playwright install`（[`install`]）と `webServer`（[`launch`] が返す [`Launched`]）に当たる。
//!
//! - インストール先は `NYAW_RUNTIMES_PATH`、無ければ Windows は `%LOCALAPPDATA%\nyaw\runtimes`、
//!   Linux は `$XDG_CACHE_HOME/nyaw/runtimes`（`~/.cache/...`）。testkit が持つのはこの中だけ。
//! - SteamVR は Steam の管理下にあるので、testkit はインストールも削除もしない（場所を探して案内するだけ）。
//! - 実ランタイムはマシンに1つしか動かないので、テストは1つずつ流す（Playwright の `workers: 1`）。
//!
//! ```ignore
//! // harness = false のテストの main から（環境変数を書き換えるのでシングルスレッドで呼ぶ）
//! let rt = unsafe { runtimes::launch(Kind::Monado) }?;
//! let mut vr = rt.connect()?;   // 起動を待って、そのランタイムだけに繋ぐ
//! // ... vr を使う ...
//! drop(vr);
//! drop(rt);                     // 自分が起動したプロセスを止め、環境変数を戻す
//! ```

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vr::{Runtime, VrBackend, VrError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// OpenXR。Windows は GitLab CI の成果物を testkit が入れる。Linux はディストリのパッケージ。
    Monado,
    /// OpenVR。HMD 無しで動かすため、同梱の null ドライバを有効にした設定で起動する。
    SteamVr,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Monado, Kind::SteamVr];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Monado => "monado",
            Kind::SteamVr => "steamvr",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// このランタイムに繋ぐときの `vr` のランタイム。
    pub fn runtime(self) -> Runtime {
        match self {
            Kind::Monado => Runtime::OpenXr,
            Kind::SteamVr => Runtime::OpenVr,
        }
    }

    /// インストール済みなら、その場所（Monado は `monado-service`、SteamVR は本体のフォルダ）。
    pub fn locate(self) -> Option<PathBuf> {
        match self {
            Kind::Monado => monado().map(|m| m.service),
            Kind::SteamVr => vr::openvr_runtime_path().filter(|p| p.exists()),
        }
    }

    /// このランタイムのプロセスが（誰が起動したものでも）動いているか。
    pub fn is_running(self) -> bool {
        match self {
            Kind::Monado => is_running("monado-service"),
            Kind::SteamVr => STEAMVR_PROCESSES.iter().any(|p| is_running(p)),
        }
    }

    /// 見つからないときにユーザーへ出す案内。
    pub fn install_hint(self) -> String {
        match (self, cfg!(windows)) {
            (Kind::Monado, true) => format!("`cargo runtimes install monado` で {} に入る", root().join("monado").display()),
            (Kind::Monado, false) => "ディストリのパッケージを入れる（例: `sudo apt install monado-service libopenxr1-monado`）".into(),
            (Kind::SteamVr, _) => format!("Steam で SteamVR を入れる（`cargo runtimes install steamvr` で {STEAMVR_INSTALL_URL} を開く）"),
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// testkit がランタイムを入れる場所。
pub fn root() -> PathBuf {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env("NYAW_RUNTIMES_PATH") {
        return p;
    }
    let cache = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    cache
        .unwrap_or_else(std::env::temp_dir)
        .join("nyaw")
        .join("runtimes")
}

// --- インストール ---------------------------------------------------------------------------

/// Monado の GitLab CI の `windows` ジョブの成果物。`install/` に一式が入っている。
/// 既定は main の最新の成功分。`NYAW_MONADO_JOB=<job id>` で `jobs/<id>/artifacts` に固定できる。
fn monado_windows_artifact() -> String {
    let base = "https://gitlab.freedesktop.org/api/v4/projects/2685/jobs";
    match std::env::var("NYAW_MONADO_JOB")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(id) => format!("{base}/{id}/artifacts"),
        None => format!("{base}/artifacts/main/download?job=windows"),
    }
}
pub const STEAMVR_INSTALL_URL: &str = "steam://install/250820";

/// インストールする（済みなら入れ直す）。何をするかは `dry_run` で先に表示できる。
pub fn install(kind: Kind, dry_run: bool) -> Result<(), String> {
    match kind {
        Kind::Monado if cfg!(windows) => {
            let dir = root().join("monado");
            println!(
                "monado: {MONADO_WINDOWS_ARTIFACT}\n     -> {}",
                dir.display()
            );
            if !dry_run {
                install_monado_windows(&dir)?;
            }
            Ok(())
        }
        // testkit が持たないもの（パッケージマネージャや Steam の管轄）は案内だけ。
        Kind::Monado => Err(kind.install_hint()),
        Kind::SteamVr => {
            if let Some(p) = kind.locate() {
                println!("steamvr: 入っている ({})。更新は Steam から", p.display());
                return Ok(());
            }
            println!("steamvr: Steam で {STEAMVR_INSTALL_URL} を開く");
            if !dry_run {
                open_url(STEAMVR_INSTALL_URL)?;
                println!(
                    "steamvr: Steam でのインストールが終わったら `cargo runtimes list` で確かめる"
                );
            }
            Ok(())
        }
    }
}

/// testkit が入れたものを消す。SteamVR は Steam から消す。
pub fn uninstall(kind: Kind) -> Result<(), String> {
    match kind {
        Kind::Monado => {
            let dir = root().join("monado");
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            println!("monado: {} を消した", dir.display());
            Ok(())
        }
        Kind::SteamVr => Err("SteamVR は Steam の管理下にあるので Steam から削除する".into()),
    }
}

fn install_monado_windows(dir: &Path) -> Result<(), String> {
    let parent = dir.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    // 途中で失敗しても前のインストールを壊さないよう、別の場所に展開してから差し替える。
    let zip = parent.join("monado.zip");
    let staging = parent.join("monado.partial");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    // Windows 10 以降に同梱の curl.exe と tar.exe（bsdtar。zip も読める）を使う。Rust の Command は
    // PATH より先に System32 を探すので、Git Bash の GNU tar（zip を読めない）は拾わない。
    run(Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(&zip)
        .arg(monado_windows_artifact()))?;
    run(Command::new("tar")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&staging)
        .arg("install/"))?;
    let _ = fs::remove_file(&zip);
    if monado_in(&staging).is_none() {
        return Err(
            "成果物に install/bin/monado-service.exe か install/openxr_monado.json が無い".into(),
        );
    }
    let _ = fs::remove_dir_all(dir);
    fs::rename(&staging, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    println!("monado: 入れた ({})", dir.display());
    Ok(())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{cmd:?}: {status}"))
}

fn open_url(url: &str) -> Result<(), String> {
    if cfg!(windows) {
        // start の最初の引数はウィンドウタイトル
        run(Command::new("cmd").args(["/C", "start", "", url]))
    } else {
        run(Command::new("xdg-open").arg(url))
    }
}

// --- 場所 ------------------------------------------------------------------------------------

struct Monado {
    service: PathBuf,
    manifest: PathBuf,
}

fn monado() -> Option<Monado> {
    if let Some(m) = monado_in(&root().join("monado")) {
        return Some(m);
    }
    // Linux はディストリのパッケージ
    let service = std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("monado-service"))
        .find(|p| p.is_file())?;
    let manifest = ["/usr/share", "/usr/local/share"]
        .iter()
        .map(|d| Path::new(d).join("openxr/1/openxr_monado.json"))
        .find(|p| p.is_file())?;
    Some(Monado { service, manifest })
}

/// testkit が展開した Monado（`<dir>/install/...`）。
fn monado_in(dir: &Path) -> Option<Monado> {
    let install = dir.join("install");
    let service = install.join("bin").join(if cfg!(windows) {
        "monado-service.exe"
    } else {
        "monado-service"
    });
    let manifest = install.join("openxr_monado.json");
    (service.is_file() && manifest.is_file()).then_some(Monado { service, manifest })
}

// --- 起動と後片付け --------------------------------------------------------------------------

/// SteamVR が動かすプロセス。後片付けではこれを全部止める。
const STEAMVR_PROCESSES: [&str; 7] = [
    "vrstartup",
    "vrmonitor",
    "vrserver",
    "vrcompositor",
    "vrdashboard",
    "vrwebhelper",
    // null ドライバだとルームセットアップが立ち上がり、vrserver を起動し直す
    "steamvr_room_setup",
];

/// HMD 無しで SteamVR を動かす設定。普段の設定には触らず、これだけを置いたフォルダを `VR_CONFIG_PATH` で指す。
const STEAMVR_NULL_SETTINGS: &str = r#"{
  "steamvr": { "requireHmd": false, "forcedDriver": "null", "activateMultipleDrivers": true },
  "driver_null": { "enable": true }
}
"#;

#[derive(Debug)]
pub enum LaunchError {
    /// 入っていない。中身は案内。テストではスキップにする（Playwright のブラウザ未インストールに当たる）。
    NotInstalled(String),
    /// すでに動いている。別の設定で動いているかもしれないし、止めると持ち主が困るので使わない。
    AlreadyRunning(Kind),
    Io(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotInstalled(hint) => write!(f, "インストールされていない: {hint}"),
            LaunchError::AlreadyRunning(k) => write!(f, "{k} がすでに動いている。終了してから流す"),
            LaunchError::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LaunchError {}

/// 起動したランタイム。Drop で、自分が起動したプロセスを止めて環境変数を元に戻す。
pub struct Launched {
    kind: Kind,
    child: Option<Child>,
    /// 書き換えた環境変数と元の値
    env: Vec<(&'static str, Option<OsString>)>,
    /// 消す一時フォルダ
    scratch: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

/// ランタイムを起動する。このプロセスの環境変数（`XR_RUNTIME_JSON` / `VR_CONFIG_PATH`）を書き換えて、
/// [`Launched::connect`] がそのランタイムに繋がるようにする。
///
/// # Safety
/// 環境変数を書き換えるので、他のスレッドが環境変数を読み書きしていないときに呼ぶ
/// （`harness = false` のテストの main から呼ぶ）。返した [`Launched`] の Drop も同じ。
pub unsafe fn launch(kind: Kind) -> Result<Launched, LaunchError> {
    if kind.locate().is_none() {
        return Err(LaunchError::NotInstalled(kind.install_hint()));
    }
    if kind.is_running() {
        return Err(LaunchError::AlreadyRunning(kind));
    }
    let mut launched = Launched {
        kind,
        child: None,
        env: vec![],
        scratch: None,
        log: None,
    };
    match kind {
        Kind::Monado => {
            let m = monado().unwrap();
            let log = root().join("logs").join("monado-service.log");
            fs::create_dir_all(log.parent().unwrap())
                .map_err(|e| LaunchError::Io(e.to_string()))?;
            let out = fs::File::create(&log)
                .map_err(|e| LaunchError::Io(format!("{}: {e}", log.display())))?;
            let err = out
                .try_clone()
                .map_err(|e| LaunchError::Io(e.to_string()))?;
            let child = Command::new(&m.service)
                // Windows 版はコンピュート合成でクアッドを出すと落ちる
                .env("XRT_COMPOSITOR_COMPUTE", "false")
                .stdin(Stdio::null())
                .stdout(out)
                .stderr(err)
                .spawn()
                .map_err(|e| LaunchError::Io(format!("{}: {e}", m.service.display())))?;
            launched.child = Some(child);
            launched.log = Some(log);
            unsafe { launched.set_env("XR_RUNTIME_JSON", m.manifest.into()) };
        }
        Kind::SteamVr => {
            // SteamVR は最初に繋いだクライアントが起動する（vrserver はクライアントの環境変数を引き継ぐ）。
            let dir =
                std::env::temp_dir().join(format!("nyaw-steamvr-null-{}", std::process::id()));
            fs::create_dir_all(&dir).map_err(|e| LaunchError::Io(e.to_string()))?;
            fs::write(dir.join("steamvr.vrsettings"), STEAMVR_NULL_SETTINGS)
                .map_err(|e| LaunchError::Io(e.to_string()))?;
            unsafe { launched.set_env("VR_CONFIG_PATH", dir.clone().into()) };
            launched.scratch = Some(dir);
        }
    }
    Ok(launched)
}

impl Launched {
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 起動を待って、このランタイムだけに繋ぐ（Playwright の `webServer` が URL を待つのに当たる）。
    pub fn connect(&mut self) -> Result<Box<dyn VrBackend>, VrError> {
        let start = Instant::now();
        loop {
            if let Some(status) = self
                .child
                .as_mut()
                .and_then(|c| c.try_wait().ok().flatten())
            {
                return Err(VrError::Runtime(format!(
                    "{} が終了した ({status})。ログ: {:?}",
                    self.kind, self.log
                )));
            }
            match vr::connect_exact(self.kind.runtime()) {
                Ok(b) => return Ok(b),
                Err(e) if start.elapsed() > Duration::from_secs(30) => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(500)),
            }
        }
    }

    unsafe fn set_env(&mut self, key: &'static str, value: OsString) {
        self.env.push((key, std::env::var_os(key)));
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // ponytail: launch の時点で動いていなかったので、いま動いている SteamVR は自分が起動したものとみなす。
        // その間にユーザーが起動したものも止めてしまう。困ったら PID を控えて絞る。
        // SteamVR は止めている途中で生き残ったプロセスが仲間を起動し直すので、2 秒続けて静かになるまで止め続ける。
        let start = Instant::now();
        let mut quiet_since = Instant::now();
        while start.elapsed() < Duration::from_secs(30)
            && quiet_since.elapsed() < Duration::from_secs(2)
        {
            if self.kind.is_running() {
                if self.kind == Kind::SteamVr {
                    kill_all(&STEAMVR_PROCESSES);
                }
                quiet_since = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        for (key, old) in self.env.drain(..).rev() {
            // SAFETY: launch と同じ約束（シングルスレッドで drop する）
            unsafe {
                match old {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        if let Some(dir) = self.scratch.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

// --- プロセス --------------------------------------------------------------------------------

/// 実行ファイル名（拡張子なし）のプロセスが動いているか。
pub fn is_running(name: &str) -> bool {
    if cfg!(windows) {
        let exe = format!("{name}.exe");
        Command::new("tasklist")
            .args(["/FO", "CSV", "/NH", "/FI"])
            .arg(format!("IMAGENAME eq {exe}"))
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains(&format!("\"{exe}\""))
            })
    } else {
        // Linux のプロセス名（comm）は 15 文字で切られる
        let comm = &name[..name.len().min(15)];
        Command::new("pgrep")
            .args(["-x", comm])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn kill_all(names: &[&str]) {
    if cfg!(windows) {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/T"]);
        for n in names {
            cmd.arg("/IM").arg(format!("{n}.exe"));
        }
        let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
    } else {
        for n in names {
            let _ = Command::new("pkill").args(["-x", n]).status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_roundtrip() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.name()), Some(k));
        }
        assert_eq!(Kind::parse("oculus"), None);
    }

    #[test]
    fn monado_in_needs_service_and_manifest() {
        let dir = std::env::temp_dir().join(format!("nyaw-testkit-monado-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bin = dir.join("install").join("bin");
        fs::create_dir_all(&bin).unwrap();
        assert!(monado_in(&dir).is_none());

        let exe = if cfg!(windows) {
            "monado-service.exe"
        } else {
            "monado-service"
        };
        fs::write(bin.join(exe), "").unwrap();
        assert!(
            monado_in(&dir).is_none(),
            "マニフェストが無いのに見つかった"
        );

        fs::write(dir.join("install").join("openxr_monado.json"), "{}").unwrap();
        let m = monado_in(&dir).unwrap();
        assert!(m.service.ends_with(exe) && m.manifest.ends_with("openxr_monado.json"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn is_running_sees_a_spawned_process_and_not_a_missing_one() {
        assert!(!is_running("nyaw-no-such-process"));
        // 自分のテストバイナリは動いているはず
        let me = std::env::current_exe().unwrap();
        assert!(is_running(me.file_stem().unwrap().to_str().unwrap()));
    }
}
