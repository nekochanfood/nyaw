use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
use nyapp::Error;
use nyapp::{package, worker::RemoteSandbox};
#[cfg(target_os = "linux")]
#[path = "../src/worker/platform/linux.rs"]
#[allow(dead_code)]
mod linux_worker;

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--confinement-probe") {
        probe_linux_confinement();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--nyapp-worker") {
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
    worker.event(nyapp_sdk::Event::Tick, "en", None, Vec::new()).unwrap();
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
