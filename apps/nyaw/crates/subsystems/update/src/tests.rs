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
    let setup = |name| Package::WindowsSetup.matches(name);
    assert!(setup("nyaw-2026.0.1-patch.2-setup.exe"));
    assert!(setup("nyaw-2026.0.1-リリース候補.1-setup.exe"));
    assert!(!setup("nyaw--setup.exe"));
    assert!(!setup("phonon.dll"));
    assert!(!setup("nyaw-..\\x-setup.exe"));
}

/// リリースに添付する各形式のアセット名（`installer/linux.sh`、`installer/nyaw.iss`）は、それぞれ 1 つの形式に対応する。
#[test]
fn each_asset_name_belongs_to_one_package() {
    for (name, package) in [
        ("nyaw-2026.0.4-setup.exe", Package::WindowsSetup),
        ("nyaw-2026.0.4-x86_64.AppImage", Package::AppImage),
        ("nyaw-2026.0.4-beta.1-x86_64.tar.gz", Package::Tar),
        ("nyaw-2026.0.4-beta.1-amd64.deb", Package::Deb),
        ("nyaw-2026.0.4-x86_64.rpm", Package::Rpm),
        ("nyaw-2026.0.4-x86_64.pkg.tar.zst", Package::Pacman),
    ] {
        assert_eq!(Package::of(name), Some(package), "{name}");
        assert_eq!(
            Package::ALL.iter().filter(|p| p.matches(name)).count(),
            1,
            "{name}"
        );
    }
    for other in [
        "nyaw-2026.0.4-x86_64.AppImage.zsync",
        "nyaw--amd64.deb",
        "nyaw-../x-amd64.deb",
        "other-2026.0.4-amd64.deb",
    ] {
        assert_eq!(Package::of(other), None, "{other}");
    }
}

#[test]
fn the_bundled_update_information_names_the_repository_and_the_format() {
    for (pattern, package) in [
        ("nyaw-*-x86_64.tar.gz", Package::Tar),
        ("nyaw-*-amd64.deb", Package::Deb),
        ("nyaw-*-x86_64.rpm", Package::Rpm),
        ("nyaw-*-x86_64.pkg.tar.zst", Package::Pacman),
    ] {
        for channel in ["latest", "latest-all"] {
            assert_eq!(
                UpdateInfo::parse(&format!(
                    "gh-releases|nekochanfood|nyaw|{channel}|{pattern}\n"
                )),
                Some(UpdateInfo {
                    repository: "nekochanfood/nyaw".into(),
                    package
                }),
                "{pattern}"
            );
        }
    }
    for broken in [
        "",
        "deb",
        "zsync|https://example.com/nyaw.zsync",
        "gh-releases|nekochanfood|nyaw|v1|nyaw-*-amd64.deb",
        "gh-releases|nekochanfood|nyaw|latest|other-*.deb",
        "gh-releases|nekochanfood|nyaw|latest|nyaw-*-setup.exe",
        "gh-releases|..|nyaw|latest|nyaw-*-amd64.deb",
        "gh-releases|a/b|nyaw|latest|nyaw-*-amd64.deb",
        "gh-releases||nyaw|latest|nyaw-*-amd64.deb",
    ] {
        assert_eq!(UpdateInfo::parse(broken), None, "{broken}");
    }
}

/// 各形式は、一覧の中から自身のアセットを選ぶ。そのアセットがないリリースは対象にしない。
#[test]
fn a_release_offers_the_asset_of_the_running_package() {
    let digest = Some("sha256:abc");
    let list = serde_json::to_vec(&serde_json::json!([
        fake::release(
            "v2026.0.4",
            false,
            Some(700),
            &[
                ("nyaw-2026.0.4-setup.exe", 1, digest),
                ("nyaw-2026.0.4-x86_64.AppImage.zsync", 1, digest),
                ("nyaw-2026.0.4-x86_64.AppImage", 2, digest),
                ("nyaw-2026.0.4-amd64.deb", 3, digest),
            ],
            "https://example.com"
        ),
        fake::release(
            "v2026.0.3",
            false,
            Some(600),
            &[("nyaw-2026.0.3-setup.exe", 1, digest)],
            "https://example.com"
        ),
    ]))
    .unwrap();
    let offered = |package| {
        parse_releases(&list, package)
            .unwrap()
            .into_iter()
            .map(|r| r.installer.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        offered(Package::AppImage),
        ["nyaw-2026.0.4-x86_64.AppImage"]
    );
    assert_eq!(offered(Package::Deb), ["nyaw-2026.0.4-amd64.deb"]);
    assert_eq!(offered(Package::Rpm), Vec::<String>::new());
    assert_eq!(offered(Package::WindowsSetup).len(), 2);
}

#[test]
fn incomplete_and_draft_releases_are_dropped() {
    let releases =
        parse_releases(&fake::varied("https://example.com"), Package::WindowsSetup).unwrap();
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
fn only_a_loopback_server_can_stand_in_for_github() {
    let accepts = |url: &str| loopback(url.to_owned()).is_some();
    assert!(accepts("http://127.0.0.1:8080/releases"));
    for url in [
        "https://127.0.0.1:8080/releases",
        "http://example.com/releases",
        "http://127.0.0.1.example.com:80/releases",
        "http://127.0.0.1:80@example.com/releases",
        "http://127.0.0.1:/releases",
        "http://127.0.0.1:8080",
    ] {
        assert!(!accepts(url), "{url}");
    }
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
    let found = check(&format!("{base}/releases"), Package::WindowsSetup).unwrap();
    let release = found
        .into_iter()
        .find(|r| r.build == 655)
        .expect("patch.2 is listed");
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
    assert!(
        check(&format!("{base}/releases"), Package::WindowsSetup).is_err(),
        "404"
    );
    let _ = fs::remove_dir_all(dir);
}
