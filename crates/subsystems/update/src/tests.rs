use super::*;
use fake::fixture;

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

#[test]
fn incomplete_and_draft_releases_are_dropped() {
    let releases = parse_releases(&fake::varied("https://example.com")).unwrap();
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
}

#[test]
fn the_api_url_comes_from_the_repository() {
    assert_eq!(
        releases_url(),
        "https://api.github.com/repos/nekochanfood/nyaw/releases?per_page=30"
    );
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

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nyaw-updater-{name}-{}", std::process::id()))
}

#[test]
fn downloads_and_verifies_the_installer() {
    let (base, files) = fixture();
    let found = check(&format!("{base}/releases")).unwrap();
    let release = found.into_iter().find(|r| r.build == 655).expect("patch.2 is listed");
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
