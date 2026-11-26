// テストが一時ディレクトリを片付けるため、ファイル操作の制限（clippy.toml）を外す。
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use update::fake::{self, fixture};

#[test]
fn the_newest_build_above_ours_wins_whatever_the_version_says() {
    let releases =
        update::parse_releases(&fake::varied("https://example.com"), Package::WindowsSetup)
            .unwrap();
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

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nyaw-updater-{name}-{}", std::process::id()))
}

static LAUNCHED: Mutex<Option<PathBuf>> = Mutex::new(None);
static FAIL: AtomicBool = AtomicBool::new(false);

fn fake_launch(path: &Path, installer: &Installer) -> Result<(), String> {
    assert!(path.ends_with(&installer.name));
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
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_version_is_embedded() {
    assert!(!VERSION.is_empty());
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

/// 配布物から起動していないビルドでは、一覧を参照せずに、確認できない理由をエラーとして示す。
#[gpui::test]
fn without_a_source_checking_reports_that_updates_are_unavailable(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Updater {
            source: String::new(),
            ..Default::default()
        });
        check_now(cx);
        let u = state(cx);
        assert!(matches!(u.step, Step::Idle));
        assert!(!u.checked);
        assert_eq!(u.error.as_deref(), Some(NOT_PACKAGED));
    });
}
