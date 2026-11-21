//! アプリの更新。GitHub のリリースから現在より新しいビルドを検出して通知し、ユーザーが選択するとダウンロードしてインストールする
//! （自動インストールは行わない）。新旧はバージョン文字列（CalVer。書式は不定）ではなく、ビルド番号で比較する（`AGENT.md`）。
//! 推奨リリースは設定のチャンネル（`update_channel`。安定版はプレリリースを除外）で決定する。

use std::fmt::Display;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::App;
use kernel::UpdateChannel;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use ui::PanelControl;

// `VERSION`（表示用バージョン。`2026.0.1-beta.1` など）と `BUILD`（ビルド番号 = HEAD までのコミット数）。build.rs が生成する。
include!(concat!(env!("OUT_DIR"), "/version.rs"));

/// リリース一覧のサイズと、インストーラーのサイズ上限。
const MAX_LIST: u64 = 4 << 20;
const MAX_INSTALLER: u64 = 512 << 20;
/// 再確認間隔（ダッシュボードが起動時およびこの間隔で確認する）。GitHub の API は認証なしで 1 時間に 60 回まで。
pub const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
/// 一覧・インストーラーの取得タイムアウト。
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 新しいリリース。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// バージョン（タグから v を除いた文字列。`2026.0.1-beta.1` など）とビルド番号。
    pub version: String,
    pub build: u32,
    /// リリースの名前（無ければタグ）と本文（Markdown）。
    pub title: String,
    pub notes: String,
    /// 公開日時（ISO 8601）とリリースページの URL。
    pub published_at: Option<String>,
    pub page: String,
    /// プレリリース（GitHub のフラグ。alpha・beta など）。ナイトリーチャンネルにのみ推奨する。
    pub prerelease: bool,
    pub installer: Installer,
}

/// リリースに付属するインストーラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub が提供する SHA-256（16 進。アセットの `digest`）。未提供の場合は検証不可のため、推奨も実行もしない。
    pub sha256: String,
}

/// リリースのインストーラーか判定する（`nyaw-<バージョン>-setup.exe`。`installer/nyaw.iss`）。パス区切りを含む名前はダウンロード先外に到達するため除外する。
pub fn is_installer(name: &str) -> bool {
    name.len() > "nyaw--setup.exe".len()
        && name.starts_with("nyaw-")
        && name.ends_with("-setup.exe")
        && !name.contains(['/', '\\'])
}

/// リリースノート末尾の非表示注釈 `<!-- nyaw-build: <番号> -->`（リリースワークフローが追加する）からビルド番号を読み取る。
/// ファイル名には含めない。GitHub も nyaw の Markdown も HTML コメントをレンダリングしない。
pub fn notes_build(notes: &str) -> Option<u32> {
    let (_, rest) = notes.rsplit_once("<!-- nyaw-build:")?;
    rest.split_once("-->")?.0.trim().parse().ok()
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

/// GitHub のリリース一覧（API の JSON）をパースする。下書きと、インストーラー・SHA-256・ビルド番号のいずれかが欠けたものは除外する。
pub fn parse_releases(json: &[u8]) -> Result<Vec<Release>, String> {
    let list: Vec<ApiRelease> =
        serde_json::from_slice(json).map_err(|e| format!("cannot read the release list: {e}"))?;
    Ok(list
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let build = notes_build(r.body.as_deref()?)?;
            let asset = r.assets.iter().find(|a| is_installer(&a.name))?;
            let sha256 = asset.digest.as_deref()?.strip_prefix("sha256:")?.to_owned();
            Some(Release {
                version: r
                    .tag_name
                    .strip_prefix('v')
                    .unwrap_or(&r.tag_name)
                    .to_owned(),
                build,
                title: r
                    .name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| r.tag_name.clone()),
                notes: r.body.unwrap_or_default(),
                published_at: r.published_at,
                page: r.html_url,
                prerelease: r.prerelease,
                installer: Installer {
                    name: asset.name.clone(),
                    url: asset.browser_download_url.clone(),
                    size: asset.size,
                    sha256,
                },
            })
        })
        .collect())
}

/// `releases` のうち `channel` で受け入れるもの（安定版はプレリリースを除外）で、ビルド番号が `build` より大きいもののうち
/// 最新のもの。
pub fn newest(releases: &[Release], build: u32, channel: UpdateChannel) -> Option<Release> {
    releases
        .iter()
        .filter(|r| r.build > build && (channel == UpdateChannel::Nightly || !r.prerelease))
        .max_by_key(|r| r.build)
        .cloned()
}

/// リリース一覧を返す GitHub API の URL（リポジトリは Cargo.toml の `repository`）。
pub fn releases_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let api = repo.replacen("https://github.com/", "https://api.github.com/repos/", 1);
    format!("{api}/releases?per_page=30")
}

/// 一覧 `url` のリリースを取得する（新旧の判定は [`newest`] で行う）。ネットワークアクセスを伴うため別スレッドで呼び出す。
pub fn check(url: &str) -> Result<Vec<Release>, String> {
    let mut response = agent(LIST_TIMEOUT)
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_LIST + 1)
        .read_to_end(&mut body)
        .map_err(err)?;
    if body.len() as u64 > MAX_LIST {
        return Err(format!("the release list is larger than {MAX_LIST} bytes"));
    }
    parse_releases(&body)
}

/// `release` のインストーラーを `dir` にダウンロードし、サイズと SHA-256 を検証する。ダウンロード済みバイト数を `progress` に書き込む。
/// 不一致の場合はファイルを削除してエラーを返す。ネットワークとファイルアクセスを伴うため別スレッドで呼び出す。
pub fn download(release: &Release, dir: &Path, progress: &AtomicU64) -> Result<PathBuf, String> {
    let installer = &release.installer;
    if !is_installer(&installer.name) || installer.size > MAX_INSTALLER {
        return Err(format!("refusing to download {}", installer.name));
    }
    let mut response = agent(DOWNLOAD_TIMEOUT)
        .get(&installer.url)
        .call()
        .map_err(|e| format!("cannot download {}: {e}", installer.url))?;
    fs::create_dir_all(dir).map_err(err)?;
    let path = dir.join(&installer.name);
    let result = (|| {
        let mut file = fs::File::create(&path).map_err(err)?;
        let mut reader = response.body_mut().as_reader().take(installer.size + 1);
        let (mut hash, mut buf, mut total) = (Sha256::new(), vec![0; 64 << 10], 0u64);
        loop {
            let n = reader.read(&mut buf).map_err(err)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(err)?;
            total += n as u64;
            progress.store(total, Ordering::Relaxed);
        }
        if total != installer.size {
            return Err(format!(
                "downloaded {total} bytes, expected {}",
                installer.size
            ));
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let want = &installer.sha256;
        if want.is_empty() || !want.eq_ignore_ascii_case(&digest) {
            return Err(format!(
                "the installer does not match sha256 {want:?} (got {digest})"
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = fs::remove_file(&path);
            Err(e)
        }
    }
}

/// インストーラーに渡す引数: ウィザードもメッセージも表示せずにインストールし（ユーザー単位のため UAC も不要）、`pid` のプロセス
/// （このアプリ）の終了を待ってからファイルを入れ替え、完了後 `relaunch`（`--novr` など）で再起動する（`installer/nyaw.iss`）。
/// インストーラーは引数を空白で結合して再渡しするため、空白・引用符を含むものは除外する。
pub fn installer_args(pid: u32, relaunch: impl IntoIterator<Item = String>) -> Vec<String> {
    let flags = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"];
    let mut args: Vec<String> = flags.map(String::from).to_vec();
    let plain = |a: &String| !a.is_empty() && !a.contains(|c: char| c.is_whitespace() || c == '"');
    let relaunch: Vec<String> = relaunch.into_iter().filter(plain).collect();
    args.extend([
        format!("/WAITPID={pid}"),
        "/RELAUNCH=1".into(),
        format!("/ARGS={}", relaunch.join(" ")),
    ]);
    args
}

/// ダウンロード済みインストーラーを実行し、完了後に現在と同じ引数で再起動させる（完了を待たない。待機するのはインストーラー側）。
pub fn run_installer(path: &Path) -> Result<(), String> {
    start_installer(path, std::env::args().skip(1))
}

/// ダウンロード済みインストーラーを実行し、完了後に `relaunch` の引数で再起動させる。
pub fn start_installer(
    path: &Path,
    relaunch: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    std::process::Command::new(path)
        .args(installer_args(std::process::id(), relaunch))
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start {}: {e}", path.display()))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // GitHub の API は User-Agent のないリクエストを拒否する。
        .user_agent(concat!("nyaw/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn err(e: impl Display) -> String {
    e.to_string()
}

/// 更新に用いるインストーラー（`installer/nyaw.iss`）を配布している OS。それ以外の OS では、更新を確認しない。
const HAS_INSTALLER: bool = cfg!(windows);
/// インストーラーを配布していない OS で、確認の代わりに表示するエラー。
const NO_INSTALLER: &str = "automatic updates are not available on this platform yet";

/// 更新の状態（GPUI のグローバル）。ダッシュボードのボタンと設定のアップデート画面が同一インスタンスを参照する。
pub struct Updater {
    /// 比較元（現在実行中のビルド）・一覧の URL・ダウンロード先・インストーラーの実行方法。テストが差し替える。
    /// 一覧の URL が空であれば、更新を確認しない（インストーラーを配布していない OS）。
    pub build: u32,
    pub source: String,
    pub dir: PathBuf,
    pub launch: fn(&Path) -> Result<(), String>,
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
        Self {
            build: BUILD,
            // インストーラーのない OS では一覧を参照しない（[`check_now`] がエラーとして表示する）。
            source: if HAS_INSTALLER {
                releases_url()
            } else {
                String::new()
            },
            dir: std::env::temp_dir().join("nyaw-update"),
            launch: run_installer,
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
    /// インストーラー実行済み（アプリは直後に終了する）。
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
        u.error = Some(NO_INSTALLER.into());
        return;
    }
    u.step = Step::Checking;
    u.error = None;
    let url = u.source.clone();
    let task = cx.background_executor().spawn(async move { check(&url) });
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
    let task = cx
        .background_executor()
        .spawn(async move { download(&release, &dir, &progress).and_then(|path| launch(&path)) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            let u = cx.default_global::<Updater>();
            match result {
                Ok(()) => {
                    logger::info!("installer started, quitting to update");
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
