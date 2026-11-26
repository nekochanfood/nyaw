use super::*;

/// テストごとの空のディレクトリ。
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nyaw-update-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_marker_names_the_package_format() {
    for (marker, package) in [
        ("tar\n", Package::Tar),
        ("deb", Package::Deb),
        ("rpm\n", Package::Rpm),
        ("pacman\n", Package::Pacman),
    ] {
        assert_eq!(Package::from_marker(marker), Some(package));
    }
    assert_eq!(Package::from_marker("appimage"), None);
    assert_eq!(Package::current(), None, "テストのバイナリは配布物ではない");
}

#[test]
fn an_appimage_is_replaced_in_place_and_stays_executable() {
    let dir = temp("replace");
    let (new, target) = (dir.join("download"), dir.join("nyaw.AppImage"));
    fs::write(&new, b"new").unwrap();
    fs::write(&target, b"old").unwrap();
    replace_file(&new, &target).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        2,
        "作業用のファイルは残さない"
    );
    // 置き換えに失敗した場合は、元のファイルを残す。
    assert!(replace_file(&dir.join("missing"), &target).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    fs::remove_dir_all(dir).unwrap();
}

/// `files`（名前と内容）を最上位の `nyaw/` に収めた tar.gz を作成する。
fn archive(dir: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = dir.join(format!("{name}.src"));
    fs::create_dir_all(root.join("nyaw")).unwrap();
    for (file, contents) in files {
        fs::write(root.join("nyaw").join(file), contents).unwrap();
    }
    let archive = dir.join(name);
    run(Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&root)
        .arg("nyaw"))
    .unwrap();
    archive
}

#[test]
fn a_tarball_replaces_its_files_and_keeps_the_others() {
    let dir = temp("unpack");
    let install = dir.join("install");
    fs::create_dir(&install).unwrap();
    fs::write(install.join("nyaw"), "old").unwrap();
    fs::write(install.join("notes.txt"), "mine").unwrap();
    let update = archive(
        &dir,
        "update.tar.gz",
        &[("nyaw", "new"), ("libphonon.so", "so")],
    );
    unpack_over(&update, &install).unwrap();
    let read = |name: &str| fs::read_to_string(install.join(name)).unwrap();
    assert_eq!(read("nyaw"), "new");
    assert_eq!(read("libphonon.so"), "so");
    assert_eq!(read("notes.txt"), "mine");
    assert!(!install.join(".nyaw-update").exists());

    // nyaw を含まないアーカイブは適用しない。
    let broken = archive(&dir, "broken.tar.gz", &[("libphonon.so", "bad")]);
    assert!(unpack_over(&broken, &install).is_err());
    assert_eq!(read("libphonon.so"), "so");
    assert!(!install.join(".nyaw-update").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_relauncher_waits_for_the_caller_to_exit() {
    let dir = temp("relaunch");
    let marker = dir.join("started");
    let mut caller = Command::new("sleep").arg("0.6").spawn().unwrap();
    relaunch_after(
        caller.id(),
        Path::new("/bin/sh"),
        [
            "-c".to_owned(),
            r#"printf %s "$1" > "$0""#.to_owned(),
            marker.to_string_lossy().into_owned(),
            "--novr".to_owned(),
        ],
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!marker.exists(), "呼び出し元の実行中は起動しない");
    // 回収するまで、終了したプロセスには `kill -0` が成功する。
    caller.wait().unwrap();
    for _ in 0..50 {
        if fs::read_to_string(&marker).is_ok_and(|s| s == "--novr") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(fs::read_to_string(&marker).unwrap(), "--novr");
    fs::remove_dir_all(dir).unwrap();
}
