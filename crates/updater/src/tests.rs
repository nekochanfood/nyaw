use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[test]
fn installer_names_carry_the_build() {
    assert_eq!(
        installer_build("nyaw-2026.0.1-patch.2-build655-setup.exe"),
        Some(655)
    );
    assert_eq!(
        installer_build("nyaw-2026.1.0-build700-setup.exe"),
        Some(700)
    );
    // 版の名前は何でもよい（`build` を含んでも、最後の `-build` が番号）。
    assert_eq!(
        installer_build("nyaw-2026.0.1-build.3-build701-setup.exe"),
        Some(701)
    );
    assert_eq!(
        installer_build("nyaw-2026.0.1-リリース候補.1-build702-setup.exe"),
        Some(702)
    );
    // ビルド番号の無い古い名前・ほかのファイル・区切りを含むものは読まない。
    assert_eq!(installer_build("nyaw-2026.0.1-setup.exe"), None);
    assert_eq!(installer_build("nyaw-2026.0.1-buildx-setup.exe"), None);
    assert_eq!(installer_build("nyaw--build3-setup.exe"), None);
    assert_eq!(installer_build("phonon.dll"), None);
    assert_eq!(installer_build("nyaw-..\\x-build3-setup.exe"), None);
}

/// GitHub の API の形（使うところだけ）。
fn release(
    tag: &str,
    draft: bool,
    assets: &[(&str, u64, Option<&str>)],
    base: &str,
) -> serde_json::Value {
    serde_json::json!({
        "tag_name": tag,
        "name": tag,
        "body": format!("## What's Changed\n* {tag}"),
        "draft": draft,
        "prerelease": true,
        "published_at": "2026-11-13T00:00:00Z",
        "html_url": format!("https://github.com/nekochanfood/nyaw/releases/tag/{tag}"),
        "assets": assets.iter().map(|(name, size, digest)| serde_json::json!({
            "name": name,
            "size": size,
            "browser_download_url": format!("{base}/{name}"),
            "digest": digest,
        })).collect::<Vec<_>>(),
    })
}

fn list(base: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!([
        // 下書きは数えない（ビルド番号が一番大きくても）。
        release(
            "v2026.0.1-patch.9",
            true,
            &[("nyaw-2026.0.1-patch.9-build999-setup.exe", 1, None)],
            base
        ),
        release(
            "v2026.0.1-patch.2",
            false,
            &[
                ("phonon.dll", 1, None),
                (
                    "nyaw-2026.0.1-patch.2-build655-setup.exe",
                    5,
                    Some("sha256:abc")
                ),
            ],
            base
        ),
        // 版はタグから（アセットの名前の記号は GitHub が置き換えることがある）。
        release(
            "v2026.0.1-ぽてと.1",
            false,
            &[("nyaw-2026.0.1-....1-build650-setup.exe", 5, None)],
            base
        ),
        // ビルド番号の無い、更新の仕組みより前のリリース。
        release(
            "v2026.0.1",
            false,
            &[("nyaw-2026.0.1-setup.exe", 5, None)],
            base
        ),
    ]))
    .unwrap()
}

#[test]
fn the_newest_build_above_ours_wins_whatever_the_version_says() {
    let releases = parse_releases(&list("https://example.com")).unwrap();
    assert_eq!(
        releases
            .iter()
            .map(|r| (r.version.as_str(), r.build))
            .collect::<Vec<_>>(),
        [("2026.0.1-patch.2", 655), ("2026.0.1-ぽてと.1", 650)],
        "下書きと、ビルド番号の読めないものは除く"
    );
    let patch2 = &releases[0];
    assert_eq!(patch2.installer.sha256.as_deref(), Some("abc"));
    assert_eq!(
        patch2.installer.url,
        "https://example.com/nyaw-2026.0.1-patch.2-build655-setup.exe"
    );
    assert_eq!(patch2.notes, "## What's Changed\n* v2026.0.1-patch.2");
    let newer = |build| newest(releases.clone(), build).map(|r| r.version);
    assert_eq!(newer(650).as_deref(), Some("2026.0.1-patch.2"));
    assert_eq!(
        newer(0).as_deref(),
        Some("2026.0.1-patch.2"),
        "git の無いビルドにはどれも新しい"
    );
    assert_eq!(newer(655), None, "同じビルドは新しくない");
    assert_eq!(
        newer(700),
        None,
        "main の先にいるビルドに古いリリースを勧めない"
    );
}

#[test]
fn the_api_url_comes_from_the_repository() {
    assert_eq!(
        releases_url(),
        "https://api.github.com/repos/nekochanfood/nyaw/releases?per_page=30"
    );
    assert!(!VERSION.is_empty());
}

#[test]
fn installer_runs_silently_and_waits_for_us() {
    let args = installer_args(42, ["--novr".into(), "--profile".into()]);
    for flag in [
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/SP-",
        "/WAITPID=42",
        "/RELAUNCH=1",
        "/ARGS=--novr --profile",
    ] {
        assert!(args.iter().any(|a| a == flag), "{flag}");
    }
    // 起動し直す引数は空白でつながるので、空白・引用符を含むものは渡さない。
    let args = installer_args(
        1,
        [
            "--profile=C:\\a b.csv".into(),
            "\"".into(),
            "--openxr".into(),
        ],
    );
    assert_eq!(args.last().map(String::as_str), Some("/ARGS=--openxr"));
    assert_eq!(
        installer_args(1, []).last().map(String::as_str),
        Some("/ARGS=")
    );
}

/// サーバーが返すファイル（パス → 中身）。テストの途中で差し替える。
type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// 手元の HTTP サーバー（ループバック）。`files` のパスを返し、ほかは 404。
fn serve(files: Files) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let body = files.lock().unwrap().get(&path).cloned();
            let mut stream = reader.into_inner();
            let (status, body) = match body {
                Some(b) => ("200 OK", b),
                None => ("404 Not Found", Vec::new()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nyaw-updater-{name}-{}", std::process::id()))
}

/// サーバーに patch.1・patch.2 の一覧と patch.2 のインストーラーを置く。
fn fixture() -> (String, Files) {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let base = serve(files.clone());
    let installer = b"MZ fake installer".to_vec();
    let json = serde_json::to_vec(&serde_json::json!([
        release(
            "v2026.0.1-patch.2",
            false,
            &[(
                "nyaw-2026.0.1-patch.2-build655-setup.exe",
                installer.len() as u64,
                Some(&format!("sha256:{}", sha256(&installer))),
            )],
            &base
        ),
        release(
            "v2026.0.1-patch.1",
            false,
            &[("nyaw-2026.0.1-patch.1-build650-setup.exe", 1, None)],
            &base
        ),
    ]))
    .unwrap();
    let mut f = files.lock().unwrap();
    f.insert("/releases".into(), json);
    f.insert(
        "/nyaw-2026.0.1-patch.2-build655-setup.exe".into(),
        installer,
    );
    drop(f);
    (base, files)
}

#[test]
fn downloads_and_verifies_the_installer() {
    let (base, files) = fixture();
    let release = check(&format!("{base}/releases"), 650)
        .unwrap()
        .expect("patch.2 is newer");
    let dir = temp("download");
    let progress = AtomicU64::new(0);
    let path = download(&release, &dir, &progress).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"MZ fake installer");
    assert_eq!(progress.load(Ordering::Relaxed), release.installer.size);

    // 中身が SHA-256 と違う・大きさが違うものは残さない。
    files.lock().unwrap().insert(
        "/nyaw-2026.0.1-patch.2-build655-setup.exe".into(),
        b"MZ evil installer".to_vec(),
    );
    assert!(
        download(&release, &dir, &progress)
            .unwrap_err()
            .contains("sha256")
    );
    assert!(!path.exists());
    files.lock().unwrap().insert(
        "/nyaw-2026.0.1-patch.2-build655-setup.exe".into(),
        b"MZ".to_vec(),
    );
    assert!(
        download(&release, &dir, &progress)
            .unwrap_err()
            .contains("bytes")
    );
    assert!(!path.exists());
    files.lock().unwrap().clear();
    assert!(check(&format!("{base}/releases"), 650).is_err(), "404");
    let _ = fs::remove_dir_all(dir);
}

static LAUNCHED: Mutex<Option<PathBuf>> = Mutex::new(None);
static FAIL: AtomicBool = AtomicBool::new(false);

fn fake_launch(path: &Path) -> Result<(), String> {
    if FAIL.load(Ordering::Relaxed) {
        return Err("cannot start".into());
    }
    *LAUNCHED.lock().unwrap() = Some(path.to_owned());
    Ok(())
}

/// 確かめると新しいリリースを覚え、ユーザーが選ぶと落としてインストーラーを走らせ、アプリを終える。
#[gpui::test]
fn check_then_install_quits_for_the_installer(cx: &mut gpui::TestAppContext) {
    let (base, _files) = fixture();
    let dir = temp("install");
    cx.update(|cx| {
        cx.set_global(Updater {
            build: 650,
            source: format!("{base}/releases"),
            dir: dir.clone(),
            launch: fake_launch,
            ..Default::default()
        });
        assert!(!available(cx));
        // 確かめる前に入れようとしても何もしない。
        install(cx);
        assert!(matches!(state(cx).step, Step::Idle));
        check_now(cx);
        assert!(matches!(state(cx).step, Step::Checking));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(available(cx));
        let u = state(cx);
        assert!(u.checked && u.error.is_none());
        assert_eq!(u.latest.as_ref().unwrap().version, "2026.0.1-patch.2");
        assert!(
            !cx.default_global::<PanelControl>().quit,
            "見つけただけでは入れない"
        );

        FAIL.store(true, Ordering::Relaxed);
        install(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let u = state(cx);
        assert_eq!(u.error.as_deref(), Some("cannot start"));
        assert!(matches!(u.step, Step::Idle), "失敗したらもう一度押せる");
        assert!(!cx.default_global::<PanelControl>().quit);
        FAIL.store(false, Ordering::Relaxed);
        install(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(state(cx).step, Step::Installing));
        assert!(
            cx.default_global::<PanelControl>().quit,
            "入れ替えのために終える"
        );
    });
    assert_eq!(
        LAUNCHED.lock().unwrap().as_deref(),
        Some(
            dir.join("nyaw-2026.0.1-patch.2-build655-setup.exe")
                .as_path()
        )
    );
    let _ = fs::remove_dir_all(dir);
}

#[gpui::test]
fn reveal_is_taken_once(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        assert!(!take_reveal(cx));
        reveal(cx);
        assert!(take_reveal(cx));
        assert!(!take_reveal(cx));
    });
}
