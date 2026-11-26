//! アプリの更新。GitHub のリリースから現在より新しいビルドを検出して通知し、ユーザーが選択するとダウンロードしてインストールする
//! （自動インストールは行わない）。新旧はバージョン文字列（CalVer。書式は不定）ではなく、ビルド番号で比較する（`AGENT.md`）。
//! 推奨リリースは設定のチャンネル（`update_channel`。安定版はプレリリースを除外）で決定する。
//! 通信・ダウンロード・インストーラーの起動は `update` サブシステムが行い、このクレートは更新の状態と画面からの操作だけを持つ。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use gpui::App;
use kernel::UpdateChannel;
use kernel::subsystems::update;
use ui::PanelControl;

pub use update::{Installer, Package, Release, start_installer};

// `VERSION`（表示用バージョン。`2026.0.1-beta.1` など）と `BUILD`（ビルド番号 = HEAD までのコミット数）。build.rs が生成する。
include!(concat!(env!("OUT_DIR"), "/version.rs"));

/// 再確認間隔（ダッシュボードが起動時およびこの間隔で確認する）。GitHub の API は認証なしで 1 時間に 60 回まで。
pub const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

/// `releases` のうち `channel` で受け入れるもの（安定版はプレリリースを除外）で、ビルド番号が `build` より大きいもののうち
/// 最新のもの。
pub fn newest(releases: &[Release], build: u32, channel: UpdateChannel) -> Option<Release> {
    releases
        .iter()
        .filter(|r| r.build > build && (channel == UpdateChannel::Nightly || !r.prerelease))
        .max_by_key(|r| r.build)
        .cloned()
}

/// 配布物から起動していないビルドで、確認の代わりに表示するエラー。
const NOT_PACKAGED: &str = "this build was not installed from a release package";

/// 更新の状態（GPUI のグローバル）。ダッシュボードのボタンと設定のアップデート画面が同一インスタンスを参照する。
pub struct Updater {
    /// 比較元（現在実行中のビルド）・一覧の URL・ダウンロード先・インストーラーの実行方法。テストが差し替える。
    /// 一覧の URL が空であれば、更新を確認しない（配布物から起動していないビルド）。
    pub build: u32,
    pub source: String,
    /// 取得するアセットの配布形式（実行中のビルドの形式）。
    pub package: Package,
    pub dir: PathBuf,
    pub launch: fn(&Path, &Installer) -> Result<(), String>,
    /// 最後に確認したリリース一覧（チャンネル不問）。推奨リリースは [`latest`] が現在のチャンネルで選択するため、
    /// チャンネルを切り替えると再確認なしで即座に反映される。
    pub found: Vec<Release>,
    pub step: Step,
    /// 1 回以上確認完了済み。
    pub checked: bool,
    /// 最後のエラー。
    pub error: Option<String>,
    /// ダッシュボードのボタンから設定のアップデート画面を開くよう要求された（設定画面が参照して消費する）。
    pub reveal: bool,
}

impl Default for Updater {
    fn default() -> Self {
        let package = Package::current();
        Self {
            build: BUILD,
            // 配布物から起動していないビルドは一覧を参照しない（[`check_now`] がエラーとして表示する）。
            source: match package {
                Some(_) => update::releases_url(),
                None => String::new(),
            },
            // 一覧を差し替えるテストは、どの OS でも同じアセットを使えるようインストーラーの形式とする。
            package: package.unwrap_or(Package::WindowsSetup),
            dir: update::download_dir(),
            launch: update::run_installer,
            found: Vec::new(),
            step: Step::Idle,
            checked: false,
            error: None,
            reveal: false,
        }
    }
}

impl gpui::Global for Updater {}

/// 現在の処理状態。
#[derive(Debug, Clone, Default)]
pub enum Step {
    #[default]
    Idle,
    Checking,
    /// ダウンロード中（ダウンロード済みバイト数。合計は `Installer::size`）。
    Downloading(Arc<AtomicU64>),
    /// ダウンロードしたファイルの適用中。Linux のパッケージは、認証とインストールの完了を待つ。
    Applying,
    /// 適用済み（アプリは直後に終了し、更新後の版が起動する）。
    Installing,
}

/// 現在の状態（未初期化であれば既定値）。
pub fn state(cx: &mut App) -> &Updater {
    cx.default_global::<Updater>()
}

/// 現在のチャンネル（設定の `update_channel`）で推奨する新しいリリース（最新版であれば `None`）。
pub fn latest(cx: &App) -> Option<Release> {
    let u = cx.try_global::<Updater>()?;
    newest(&u.found, u.build, ui::settings(cx).update_channel)
}

/// 新しいリリースが利用可能か（ダッシュボードのボタン用）。
pub fn available(cx: &App) -> bool {
    latest(cx).is_some()
}

/// 新しいリリースの有無を確認する（別スレッド）。処理中であれば何もしない。
pub fn check_now(cx: &mut App) {
    let u = cx.default_global::<Updater>();
    if !matches!(u.step, Step::Idle) {
        return;
    }
    if u.source.is_empty() {
        u.error = Some(NOT_PACKAGED.into());
        return;
    }
    u.step = Step::Checking;
    u.error = None;
    let (url, package) = (u.source.clone(), u.package);
    let task = cx
        .background_executor()
        .spawn(async move { update::check(&url, package) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            let u = cx.default_global::<Updater>();
            u.step = Step::Idle;
            match result {
                Ok(found) => {
                    u.found = found;
                    u.checked = true;
                    let build = u.build;
                    match latest(cx) {
                        Some(r) => logger::info!(
                            "update available",
                            version = r.version,
                            build = r.build,
                            current = build
                        ),
                        None => logger::info!("up to date", build = build),
                    }
                }
                Err(e) => {
                    logger::warn!("cannot check for updates", error = e);
                    u.error = Some(e);
                }
            }
        })
    })
    .detach();
}

/// 検出済みリリースをダウンロードし、インストーラーを実行してアプリを終了する（インストーラーがファイルを入れ替えて再起動する）。
/// ユーザーの操作時のみ呼び出す。処理中であれば何もしない。
pub fn install(cx: &mut App) {
    let Some(release) = latest(cx) else {
        return;
    };
    let u = cx.default_global::<Updater>();
    if !matches!(u.step, Step::Idle) {
        return;
    }
    let progress = Arc::new(AtomicU64::new(0));
    u.step = Step::Downloading(progress.clone());
    u.error = None;
    let (dir, launch) = (u.dir.clone(), u.launch);
    logger::info!(
        "downloading update",
        version = release.version,
        url = release.installer.url
    );
    let installer = release.installer.clone();
    let download = cx
        .background_executor()
        .spawn(async move { update::download(&release, &dir, &progress) });
    cx.spawn(async move |cx| {
        let result = match download.await {
            Ok(path) => {
                cx.update(|cx| cx.default_global::<Updater>().step = Step::Applying);
                cx.background_executor()
                    .spawn(async move { launch(&path, &installer) })
                    .await
            }
            Err(e) => Err(e),
        };
        cx.update(|cx| {
            let u = cx.default_global::<Updater>();
            match result {
                Ok(()) => {
                    logger::info!("update applied, quitting to restart");
                    u.step = Step::Installing;
                    cx.default_global::<PanelControl>().quit = true;
                }
                Err(e) => {
                    logger::warn!("cannot update", error = e);
                    u.step = Step::Idle;
                    u.error = Some(e);
                }
            }
        })
    })
    .detach();
}

/// 画面を起動せずに更新する（起動オプション `--update`）。推奨リリースがあればダウンロードして適用し、`relaunch` の引数で
/// 再起動させて、そのリリースを返す。この場合、呼び出し元は直後に終了する。完了まで呼び出し元のスレッドを停止する。
pub fn update_headless(
    channel: UpdateChannel,
    relaunch: Vec<String>,
) -> Result<Option<Release>, String> {
    let package = Package::current().ok_or(NOT_PACKAGED)?;
    let found = update::check(&update::releases_url(), package)?;
    let Some(release) = newest(&found, BUILD, channel) else {
        return Ok(None);
    };
    logger::info!(
        "downloading update",
        version = release.version,
        build = release.build,
        url = release.installer.url
    );
    let path = update::download(&release, &update::download_dir(), &AtomicU64::new(0))?;
    update::start_installer(&path, &release.installer, relaunch)?;
    Ok(Some(release))
}

/// 設定のアップデート画面を開くよう要求する（ダッシュボードのボタン）。
pub fn reveal(cx: &mut App) {
    cx.default_global::<Updater>().reveal = true;
}

/// 要求があれば消費して `true` を返す（設定画面用）。
pub fn take_reveal(cx: &mut App) -> bool {
    cx.try_global::<Updater>().is_some_and(|u| u.reveal)
        && std::mem::take(&mut cx.global_mut::<Updater>().reveal)
}

#[cfg(test)]
mod tests;
