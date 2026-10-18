use std::time::{SystemTime, UNIX_EPOCH};

use nyapp::{package, worker::RemoteSandbox};

fn main() {
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
    worker.event(nyapp_sdk::Event::Tick, "en", None).unwrap();
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
