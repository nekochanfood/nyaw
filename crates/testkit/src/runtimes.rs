//! Prepares real runtimes (Monado / SteamVR) for E2E tests. The equivalent of Playwright's
//! `npx playwright install` ([`install`]) and `webServer` ([`Launched`] returned by [`launch`]).
//!
//! ランタイムの準備は現在 Windows 向け。保存先は `NYAW_RUNTIMES_PATH`、未指定なら
//! `%LOCALAPPDATA%\nyaw\runtimes`。testkit はこの中だけを管理する。
//! - SteamVR is managed by Steam, so testkit neither installs nor removes it (it only locates it and gives guidance).
//! - Only one real runtime can run on a machine, so tests run one at a time (like Playwright's `workers: 1`).
//!
//! ```ignore
//! // From the main of a harness = false test (call it single-threaded, since it rewrites environment variables)
//! let rt = unsafe { runtimes::launch(Kind::Monado) }?;
//! let mut vr = rt.connect()?;   // wait for startup and connect to that runtime only
//! // ... use vr ...
//! drop(vr);
//! drop(rt);                     // stop the processes we started and restore environment variables
//! ```

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use kernel::vr::{Runtime, VrBackend, VrError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// OpenXR。Windows 向けの GitLab CI 成果物を入れる。
    Monado,
    /// OpenVR. To run without an HMD, it is launched with a config that enables the bundled null driver.
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

    /// The `vr` runtime to use when connecting to this runtime.
    pub fn runtime(self) -> Runtime {
        match self {
            Kind::Monado => Runtime::OpenXr,
            Kind::SteamVr => Runtime::OpenVr,
        }
    }

    /// If installed, its location (`monado-service` for Monado, the main folder for SteamVR).
    pub fn locate(self) -> Option<PathBuf> {
        match self {
            Kind::Monado => monado().map(|m| m.service),
            Kind::SteamVr => kernel::vr::openvr_runtime_path().filter(|p| p.exists()),
        }
    }

    /// Whether a process of this runtime is running (no matter who started it).
    pub fn is_running(self) -> bool {
        !running(self.processes()).is_empty()
    }

    fn processes(self) -> &'static [&'static str] {
        match self {
            Kind::Monado => &["monado-service"],
            Kind::SteamVr => &STEAMVR_PROCESSES,
        }
    }

    /// Guidance shown to the user when it is not found.
    pub fn install_hint(self) -> String {
        match self {
            Kind::Monado => format!(
                "`cargo runtimes install monado` installs it to {}",
                root().join("monado").display()
            ),
            Kind::SteamVr => format!(
                "Install SteamVR via Steam (`cargo runtimes install steamvr` opens {STEAMVR_INSTALL_URL})"
            ),
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Where testkit installs runtimes.
pub fn root() -> PathBuf {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env("NYAW_RUNTIMES_PATH") {
        return p;
    }
    kernel::subsystems::os::data_dir()
        .unwrap_or_else(|| std::env::temp_dir().join("nyaw"))
        .join("runtimes")
}

// --- Install --------------------------------------------------------------------------------

/// Artifacts of Monado's GitLab CI `windows` job. The whole set is in `install/`.
/// Defaults to the latest successful one on main. `NYAW_MONADO_JOB=<job id>` pins it to `jobs/<id>/artifacts`.
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

/// Installs (reinstalls if already installed). `dry_run` shows beforehand what would be done.
pub fn install(kind: Kind, dry_run: bool) -> Result<(), String> {
    match kind {
        Kind::Monado if cfg!(windows) => {
            let dir = root().join("monado");
            println!(
                "monado: {}\n     -> {}",
                monado_windows_artifact(),
                dir.display()
            );
            if !dry_run {
                install_monado_windows(&dir)?;
            }
            Ok(())
        }
        Kind::Monado => Err("no runtime installer is implemented for this target".into()),
        Kind::SteamVr => {
            if let Some(p) = kind.locate() {
                println!("steamvr: installed ({}). Update it via Steam", p.display());
                return Ok(());
            }
            println!("steamvr: open {STEAMVR_INSTALL_URL} in Steam");
            if !dry_run {
                open_url(STEAMVR_INSTALL_URL)?;
                println!(
                    "steamvr: once the install in Steam finishes, check with `cargo runtimes list`"
                );
            }
            Ok(())
        }
    }
}

/// Removes what testkit installed. SteamVR is removed via Steam.
pub fn uninstall(kind: Kind) -> Result<(), String> {
    match kind {
        Kind::Monado => {
            let dir = root().join("monado");
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            println!("monado: removed {}", dir.display());
            Ok(())
        }
        Kind::SteamVr => Err("SteamVR is managed by Steam; remove it via Steam".into()),
    }
}

fn install_monado_windows(dir: &Path) -> Result<(), String> {
    let parent = dir.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    // Extract to a separate location and then swap, so a failure midway does not break the previous install.
    let zip = parent.join("monado.zip");
    let staging = parent.join("monado.partial");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    // Use curl.exe and tar.exe (bsdtar; it can read zip too) bundled with Windows 10 and later. Rust's Command
    // looks in System32 before PATH, so Git Bash's GNU tar (which cannot read zip) is not picked up.
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
            "the artifacts lack install/bin/monado-service.exe or install/openxr_monado.json"
                .into(),
        );
    }
    let _ = fs::remove_dir_all(dir);
    fs::rename(&staging, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    println!("monado: installed ({})", dir.display());
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
    #[cfg(windows)]
    {
        // start の最初の引数はウィンドウタイトル。
        run(Command::new("cmd").args(["/C", "start", "", url]))
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("no URL opener is implemented for this target".into())
    }
}

// --- Locations ------------------------------------------------------------------------------

struct Monado {
    service: PathBuf,
    manifest: PathBuf,
}

fn monado() -> Option<Monado> {
    monado_in(&root().join("monado"))
}

/// Monado extracted by testkit (`<dir>/install/...`).
fn monado_in(dir: &Path) -> Option<Monado> {
    let install = dir.join("install");
    #[cfg(windows)]
    let service = install.join("bin").join("monado-service.exe");
    let manifest = install.join("openxr_monado.json");
    (service.is_file() && manifest.is_file()).then_some(Monado { service, manifest })
}

// --- Launch and cleanup ---------------------------------------------------------------------

/// Processes that SteamVR runs. Cleanup stops all of them.
const STEAMVR_PROCESSES: [&str; 7] = [
    "vrstartup",
    "vrmonitor",
    "vrserver",
    "vrcompositor",
    "vrdashboard",
    "vrwebhelper",
    // With the null driver, room setup starts up and restarts vrserver
    "steamvr_room_setup",
];

/// Config for running SteamVR without an HMD. Leaves the usual config untouched and points `VR_CONFIG_PATH` at a folder containing only this.
const STEAMVR_NULL_SETTINGS: &str = r#"{
  "steamvr": { "requireHmd": false, "forcedDriver": "null", "activateMultipleDrivers": true },
  "driver_null": { "enable": true }
}
"#;

#[derive(Debug)]
pub enum LaunchError {
    /// Not installed. The payload is guidance. Tests skip (like Playwright with browsers not installed).
    NotInstalled(String),
    /// Already running. It may be running with a different config, and stopping it would trouble its owner, so it is not used.
    AlreadyRunning(Kind),
    Io(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotInstalled(hint) => write!(f, "not installed: {hint}"),
            LaunchError::AlreadyRunning(k) => {
                write!(f, "{k} is already running. Quit it and then run")
            }
            LaunchError::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LaunchError {}

/// A launched runtime. On Drop, stops the processes it started and restores environment variables.
pub struct Launched {
    kind: Kind,
    child: Option<Child>,
    /// Environment variables that were rewritten, with their original values
    env: Vec<(&'static str, Option<OsString>)>,
    /// Temporary folders to delete
    scratch: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

/// Launches a runtime. Rewrites this process's environment variables (`XR_RUNTIME_JSON` / `VR_CONFIG_PATH`) so that
/// [`Launched::connect`] connects to that runtime.
///
/// # Safety
/// This rewrites environment variables, so call it when no other thread is reading or writing them
/// (call it from the main of a `harness = false` test). The same applies to the Drop of the returned [`Launched`].
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
                // The Windows build crashes when showing a quad with compute composition
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
            // SteamVR is started by the first client that connects (vrserver inherits the client's environment variables).
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

    /// Waits for startup and connects to this runtime only (like Playwright's `webServer` waiting for the URL).
    pub fn connect(&mut self) -> Result<Box<dyn VrBackend>, VrError> {
        let start = Instant::now();
        loop {
            if let Some(status) = self
                .child
                .as_mut()
                .and_then(|c| c.try_wait().ok().flatten())
            {
                return Err(VrError::Runtime(format!(
                    "{} exited ({status}). log: {:?}",
                    self.kind, self.log
                )));
            }
            match kernel::vr::connect_exact(self.kind.runtime()) {
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
        // ponytail: it was not running at launch time, so the SteamVR running now is assumed to be ours.
        // This also stops one the user started in the meantime. If that is a problem, record the PID and narrow it down.
        // While SteamVR is being stopped, surviving processes restart their peers, so keep stopping until it stays quiet for 2 seconds.
        let start = Instant::now();
        let mut quiet_since = Instant::now();
        // Only SteamVR's processes restart each other. Monado is a single process that we killed.
        let quiet = if self.kind == Kind::SteamVr {
            Duration::from_secs(2)
        } else {
            Duration::ZERO
        };
        while start.elapsed() < Duration::from_secs(30) && quiet_since.elapsed() < quiet {
            // One process snapshot per pass (spawning tasklist per name cost ~100ms each)
            let alive = running(self.kind.processes());
            if !alive.is_empty() {
                kill_all(alive);
                quiet_since = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        for (key, old) in self.env.drain(..).rev() {
            // SAFETY: same contract as launch (drop single-threaded)
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

// --- Processes ------------------------------------------------------------------------------

/// Processes (from one snapshot) whose executable name (without extension) is in `names`.
fn running(names: &[&str]) -> Vec<sysinfo::Pid> {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All);
    sys.processes()
        .iter()
        .filter(|(_, p)| {
            let n = p.name().to_string_lossy().to_lowercase();
            let n = n.strip_suffix(".exe").unwrap_or(&n);
            names.contains(&n)
        })
        .map(|(pid, _)| *pid)
        .collect()
}

/// Whether a process with the executable name (without extension) is running.
pub fn is_running(name: &str) -> bool {
    !running(&[name]).is_empty()
}

fn kill_all(pids: Vec<sysinfo::Pid>) {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All);
    for pid in pids {
        if let Some(p) = sys.process(pid) {
            p.kill();
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

        let exe = "monado-service.exe";
        fs::write(bin.join(exe), "").unwrap();
        assert!(
            monado_in(&dir).is_none(),
            "found even though there is no manifest"
        );

        fs::write(dir.join("install").join("openxr_monado.json"), "{}").unwrap();
        let m = monado_in(&dir).unwrap();
        assert!(m.service.ends_with(exe) && m.manifest.ends_with("openxr_monado.json"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn is_running_sees_a_spawned_process_and_not_a_missing_one() {
        assert!(!is_running("nyaw-no-such-process"));
        // our own test binary should be running
        let me = std::env::current_exe().unwrap();
        assert!(is_running(me.file_stem().unwrap().to_str().unwrap()));
    }
}
