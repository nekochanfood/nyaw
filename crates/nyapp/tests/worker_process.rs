use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(target_os = "linux", windows))]
use nyapp::Error;
use nyapp::{package, worker::RemoteSandbox};
#[cfg(windows)]
use std::io::Read;
#[cfg(target_os = "linux")]
#[path = "../src/worker/platform/linux.rs"]
#[allow(dead_code)]
mod linux_worker;
#[cfg(target_os = "linux")]
type Pipes = (
    Box<dyn std::io::Write + Send>,
    Box<dyn std::io::Read + Send>,
    linux_worker::Guard,
);
#[cfg(windows)]
mod manifest {
    pub use nyapp::manifest::valid_id;
}
#[cfg(windows)]
mod system {
    pub use nyapp::system::is_system;
}
#[cfg(windows)]
#[path = "../src/worker/platform/windows.rs"]
#[allow(dead_code)]
mod windows_worker;
#[cfg(windows)]
type Pipes = (
    Box<dyn std::io::Write + Send>,
    Box<dyn std::io::Read + Send>,
    windows_worker::Guard,
);

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--confinement-probe") {
        probe_linux_confinement();
        return;
    }
    // ロック中はワーカーを起こせない（tests/lock.rs が確かめる）。
    if !nyapp::NORMAL_NYAPPS_ENABLED {
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--nyapp-worker") {
        #[cfg(windows)]
        if std::env::var("NYAW_WORKER_ID")
            .is_ok_and(|id| id.starts_with("net.bettaworx.auditforeign_"))
        {
            unsafe { std::env::set_var("NYAW_WORKER_ID", "net.bettaworx.workertest") };
            println!("{}", nyapp::worker::main().unwrap_err());
            return;
        }
        if std::env::var_os("NYAW_TEST_PARENT_SECRET").is_some() {
            std::process::exit(2);
        }
        if let Err(error) = nyapp::worker::main() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    external_nyapp_runs_in_a_restricted_worker();
    worker_cannot_run_without_the_restricted_launcher();
    #[cfg(target_os = "linux")]
    linux_confinement_denies_files_and_sockets();
    #[cfg(windows)]
    windows_foreign_sid_and_profile_cleanup();
}

#[cfg(windows)]
fn windows_foreign_sid_and_profile_cleanup() {
    use windows_sys::Win32::Security::Isolation::CreateAppContainerProfile;
    use windows_sys::Win32::Security::{FreeSid, PSID};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let id = format!(
        "net.bettaworx.auditforeign_{}_{}",
        std::process::id(),
        stamp
    );
    let profile: Vec<u16> = windows_worker::profile_name(&id)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let (input, mut output, guard) =
        windows_worker::spawn(&std::env::current_exe().unwrap(), &id).unwrap();
    drop(input);
    let mut message = String::new();
    output.read_to_string(&mut message).unwrap();
    drop(guard);

    let create = || {
        let mut sid: PSID = std::ptr::null_mut();
        let hr = unsafe {
            CreateAppContainerProfile(
                profile.as_ptr(),
                profile.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            )
        };
        if !sid.is_null() {
            unsafe { FreeSid(sid) };
        }
        hr
    };
    let existed = create();
    let removed = nyapp::installed::uninstall(&id);
    let recreated = create();
    windows_worker::delete_profile(&id).unwrap();
    assert!(message.contains("unexpected AppContainer SID"), "{message}");
    assert_eq!(existed as u32, 0x8007_00b7, "profile was not created");
    removed.unwrap();
    assert_eq!(recreated, 0, "profile still exists after uninstall");
}

#[cfg(target_os = "linux")]
fn linux_confinement_denies_files_and_sockets() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--confinement-probe")
        .env_clear()
        .env("NYAW_WORKER_PARENT_PID", std::process::id().to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "confinement probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
fn probe_linux_confinement() {
    linux_worker::confine().unwrap();
    let file = unsafe { libc::open(c"/etc/passwd".as_ptr(), libc::O_RDONLY) };
    assert_eq!(file, -1, "worker opened a host file");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    let socket = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert_eq!(socket, -1, "worker opened a network socket");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    let executable = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_EXEC,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_eq!(
        executable,
        libc::MAP_FAILED,
        "worker mapped executable memory"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    let writable = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(
        writable,
        libc::MAP_FAILED,
        "worker cannot map writable memory"
    );
    assert_eq!(
        unsafe { libc::mprotect(writable, 4096, libc::PROT_READ | libc::PROT_EXEC) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    assert_eq!(unsafe { libc::munmap(writable, 4096) }, 0);
}

fn external_nyapp_runs_in_a_restricted_worker() {
    // この専用テスト実行体は単一スレッドで起動する。
    unsafe { std::env::set_var("NYAW_TEST_PARENT_SECRET", "must-not-cross") };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nyaw-worker-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("net.bettaworx.workertest.nyapp");
    let wasm = wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (data (i32.const 16) "{\"type\":\"text\",\"text\":\"worker\"}")
            (func (export "nyapp_alloc") (param i32) (result i32) (i32.const 1024))
            (func (export "nyapp_init"))
            (func (export "nyapp_view") (result i64) (i64.const 68719476767))
            (func (export "nyapp_event") (param i32 i32)))"#,
    )
    .unwrap();
    package::write(
        &path,
        "id = \"net.bettaworx.workertest\"\nname = \"Worker test\"\nversion = \"1\"\napi = 1\n",
        &wasm,
        &package::Icon::default(),
    )
    .unwrap();

    let exe = std::env::current_exe().unwrap();
    let (worker, initial, results) =
        RemoteSandbox::start_with_executable(&exe, &path, "net.bettaworx.workertest", "en")
            .unwrap();
    assert!(worker.is_alive());
    assert!(format!("{:?}", initial.view).contains("worker"));
    worker
        .event(nyapp_sdk::Event::Tick, "en", None, Vec::new())
        .unwrap();
    let next = results
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(format!("{:?}", next.view).contains("worker"));
    drop(worker);
    unsafe { std::env::remove_var("NYAW_TEST_PARENT_SECRET") };
    std::fs::remove_dir_all(dir).unwrap();
}

fn worker_cannot_run_without_the_restricted_launcher() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--nyapp-worker")
        .status()
        .unwrap();
    assert!(!status.success());
}
