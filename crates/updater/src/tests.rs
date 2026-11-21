use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[test]
fn the_build_number_hides_in_the_notes_and_installers_are_plain_names() {
    let notes = "## What's Changed\n* fix\n\n<!-- nyaw-build: 655 -->\n";
    assert_eq!(notes_build(notes), Some(655));
    assert_eq!(
        notes_build("<!--nyaw-build:7-->"),
        None,
        "ワークフロー定義どおりの書式のみ有効"
    );
    assert_eq!(notes_build("<!-- nyaw-build: 7 -->"), Some(7));
    // 本文に同一の注釈を記述しても、最後のもの（ワークフローが追加したもの）が有効。
    assert_eq!(
        notes_build("例: `<!-- nyaw-build: 1 -->`\n\n<!-- nyaw-build: 9 -->"),
        Some(9)
    );
    assert_eq!(notes_build("## What's Changed"), None);
    assert_eq!(notes_build("<!-- nyaw-build: x -->"), None);
    // インストーラーのファイル名にはバージョンのみ（ビルド番号は含めない）。バージョン名は任意。
    assert!(is_installer("nyaw-2026.0.1-patch.2-setup.exe"));
    assert!(is_installer("nyaw-2026.0.1-リリース候補.1-setup.exe"));
    assert!(!is_installer("nyaw--setup.exe"));
    assert!(!is_installer("phonon.dll"));
    assert!(!is_installer("nyaw-..\\x-setup.exe"));
}

/// GitHub API レスポンスの形式（使用部分のみ）。`build` があればワークフロー定義どおりリリースノート末尾に注釈を追加する。
/// 名前付きバージョン（`-<名前>.<数>`）は、ワークフロー定義どおりプレリリースとする。
fn release(
    tag: &str,
    draft: bool,
    build: Option<u32>,
    assets: &[(&str, u64, Option<&str>)],
    base: &str,
) -> serde_json::Value {
    let marker = build.map_or(String::new(), |b| format!("\n\n<!-- nyaw-build: {b} -->"));
    serde_json::json!({
        "tag_name": tag,
        "name": tag,
        "body": format!("## What's Changed\n* {tag}{marker}"),
        "draft": draft,
        "prerelease": tag.contains('-'),
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
        // 下書きは除外する（ビルド番号が最大でも）。
        release(
            "v2026.0.1-patch.9",
            true,
            Some(999),
            &[("nyaw-2026.0.1-patch.9-setup.exe", 1, None)],
            base
        ),
        release(
            "v2026.0.1-patch.2",
            false,
            Some(655),
            &[
                ("phonon.dll", 1, None),
                ("nyaw-2026.0.1-patch.2-setup.exe", 5, Some("sha256:abc")),
            ],
            base
        ),
        // GitHub が SHA-256 を提供しないインストーラーは検証不可のため推奨しない（番号が最大でも）。
        release(
            "v2026.0.1-patch.4",
            false,
            Some(670),
            &[("nyaw-2026.0.1-patch.4-setup.exe", 5, None)],
            base
        ),
        // ビルド番号はあるがインストーラーが未添付（ビルド中）。
        release("v2026.0.1-patch.3", false, Some(660), &[], base),
        // バージョンはタグから取得（アセット名の記号は GitHub が変換する場合がある）。
        release(
            "v2026.0.1-ぽてと.1",
            false,
            Some(650),
            &[("nyaw-2026.0.1-....1-setup.exe", 5, Some("sha256:def"))],
            base
        ),
        // ビルド番号のない、更新機構導入前のリリース。
        release(
            "v2026.0.1",
            false,
            None,
            &[("nyaw-2026.0.1-setup.exe", 5, None)],
            base
        ),
        // 正式リリース（安定版チャンネルにも推奨する）。
        release(
            "v2026.0.2",
            false,
            Some(640),
            &[("nyaw-2026.0.2-setup.exe", 5, Some("sha256:123"))],
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
        [
            ("2026.0.1-patch.2", 655),
            ("2026.0.1-ぽてと.1", 650),
            ("2026.0.2", 640)
        ],
        "下書きおよびインストーラー・SHA-256・ビルド番号のいずれかが欠けたものは除外する"
    );
    assert_eq!(
        releases.iter().map(|r| r.prerelease).collect::<Vec<_>>(),
        [true, true, false]
    );
    let patch2 = &releases[0];
    assert_eq!(patch2.installer.sha256, "abc");
    assert_eq!(
        patch2.installer.url,
        "https://example.com/nyaw-2026.0.1-patch.2-setup.exe"
    );
    assert!(
        patch2
            .notes
            .starts_with("## What's Changed\n* v2026.0.1-patch.2")
    );
    let newer = |build| newest(&releases, build, UpdateChannel::Nightly).map(|r| r.version);
    assert_eq!(newer(650).as_deref(), Some("2026.0.1-patch.2"));
    assert_eq!(
        newer(0).as_deref(),
        Some("2026.0.1-patch.2"),
        "git のないビルドにはすべて新しい"
    );
    assert_eq!(newer(655), None, "同一ビルドは新しくない");
    assert_eq!(
        newer(700),
        None,
        "main より先のビルドに旧リリースを推奨しない"
    );
    // 安定版はプレリリースを除外して選択する（プレリリースの方が新しくても）。
    let stable = |build| newest(&releases, build, UpdateChannel::Stable).map(|r| r.version);
    assert_eq!(stable(0).as_deref(), Some("2026.0.2"));
    assert_eq!(stable(640), None);
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
    // 再起動引数は空白で結合されるため、空白・引用符を含むものは渡さない。
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

/// サーバーが返すファイル（パス → 内容）。テスト中に差し替え可能。
type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// ローカル HTTP サーバー（ループバック）。`files` のパスを返し、他は 404。
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

/// サーバーに patch.1・patch.2 の一覧と patch.2 のインストーラーを配置する。
fn fixture() -> (String, Files) {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let base = serve(files.clone());
    let installer = b"MZ fake installer".to_vec();
    let json = serde_json::to_vec(&serde_json::json!([
        release(
            "v2026.0.1-patch.2",
            false,
            Some(655),
            &[(
                "nyaw-2026.0.1-patch.2-setup.exe",
                installer.len() as u64,
                Some(&format!("sha256:{}", sha256(&installer))),
            )],
            &base
        ),
        release(
            "v2026.0.1-patch.1",
            false,
            Some(650),
            &[("nyaw-2026.0.1-patch.1-setup.exe", 1, None)],
            &base
        ),
    ]))
    .unwrap();
    let mut f = files.lock().unwrap();
    f.insert("/releases".into(), json);
    f.insert("/nyaw-2026.0.1-patch.2-setup.exe".into(), installer);
    drop(f);
    (base, files)
}

#[test]
fn downloads_and_verifies_the_installer() {
    let (base, files) = fixture();
    let found = check(&format!("{base}/releases")).unwrap();
    let release = newest(&found, 650, UpdateChannel::Nightly).expect("patch.2 is newer");
    let dir = temp("download");
    let progress = AtomicU64::new(0);
    let path = download(&release, &dir, &progress).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"MZ fake installer");
    assert_eq!(progress.load(Ordering::Relaxed), release.installer.size);
    // 検証用 SHA-256 がなければ、内容が正しくても実行対象として保持しない。
    let mut blank = release.clone();
    blank.installer.sha256.clear();
    assert!(
        download(&blank, &dir, &progress)
            .unwrap_err()
            .contains("sha256")
    );
    assert!(!path.exists());

    // 内容が SHA-256 と不一致、またはサイズが異なるものは保持しない。
    files.lock().unwrap().insert(
        "/nyaw-2026.0.1-patch.2-setup.exe".into(),
        b"MZ evil installer".to_vec(),
    );
    assert!(
        download(&release, &dir, &progress)
            .unwrap_err()
            .contains("sha256")
    );
    assert!(!path.exists());
    files
        .lock()
        .unwrap()
        .insert("/nyaw-2026.0.1-patch.2-setup.exe".into(), b"MZ".to_vec());
    assert!(
        download(&release, &dir, &progress)
            .unwrap_err()
            .contains("bytes")
    );
    assert!(!path.exists());
    files.lock().unwrap().clear();
    assert!(check(&format!("{base}/releases")).is_err(), "404");
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

/// 確認すると新しいリリースを記録し、ユーザーが選択するとダウンロードしてインストーラーを実行し、アプリを終了する。
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
        // fixture のリリースはどちらもプレリリースのため、ナイトリーチャンネルで受け取る。
        cx.default_global::<PanelControl>().settings.update_channel = UpdateChannel::Nightly;
        assert!(!available(cx));
        // 確認前にインストールを試みても何もしない。
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
        assert_eq!(latest(cx).unwrap().version, "2026.0.1-patch.2");
        assert!(
            !cx.default_global::<PanelControl>().quit,
            "検出のみではインストールしない"
        );
        // 安定版に切り替えると、再確認なしで推奨されなくなる（インストールを試みても何もしない）。
        cx.default_global::<PanelControl>().settings.update_channel = UpdateChannel::Stable;
        assert!(!available(cx));
        install(cx);
        assert!(matches!(state(cx).step, Step::Idle));
        cx.default_global::<PanelControl>().settings.update_channel = UpdateChannel::Nightly;

        FAIL.store(true, Ordering::Relaxed);
        install(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let u = state(cx);
        assert_eq!(u.error.as_deref(), Some("cannot start"));
        assert!(matches!(u.step, Step::Idle), "失敗後は再試行可能");
        assert!(!cx.default_global::<PanelControl>().quit);
        FAIL.store(false, Ordering::Relaxed);
        install(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(state(cx).step, Step::Installing));
        assert!(
            cx.default_global::<PanelControl>().quit,
            "更新のために終了する"
        );
    });
    assert_eq!(
        LAUNCHED.lock().unwrap().as_deref(),
        Some(dir.join("nyaw-2026.0.1-patch.2-setup.exe").as_path())
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
