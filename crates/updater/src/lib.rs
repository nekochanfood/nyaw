//! アプリの更新。GitHub のリリースから今より新しいビルドを見つけて知らせ、ユーザーが選んだら落として入れ替える
//! （勝手には入れない）。新旧は版の文字（CalVer。書き方が決まっていない）ではなく、ビルド番号で比べる（`AGENT.md`）。
//! どのリリースを勧めるかは設定のチャンネル（`update_channel`。stable はプレリリースを除く）で決める。

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

// `VERSION`（表示する版。`2026.0.1-beta.1` など）と `BUILD`（ビルド番号 = HEAD までのコミット数）。build.rs が作る。
include!(concat!(env!("OUT_DIR"), "/version.rs"));

/// リリースの一覧の大きさと、インストーラーの大きさの上限。
const MAX_LIST: u64 = 4 << 20;
const MAX_INSTALLER: u64 = 512 << 20;
/// 確かめ直す間隔（ダッシュボードが起動時とこの間隔で確かめる）。GitHub の API は認証なしで 1 時間に 60 回まで。
pub const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
/// 一覧・インストーラーの取得にかけてよい時間。
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 新しいリリース。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// 版（タグから v を除いたもの。`2026.0.1-beta.1` など）とビルド番号。
    pub version: String,
    pub build: u32,
    /// リリースの名前（無ければタグ）と本文（Markdown）。
    pub title: String,
    pub notes: String,
    /// 公開した時刻（ISO 8601）とリリースのページ。
    pub published_at: Option<String>,
    pub page: String,
    /// プレリリース（GitHub の印。alpha・beta など）。nightly のチャンネルにだけ勧める。
    pub prerelease: bool,
    pub installer: Installer,
}

/// リリースに付いたインストーラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub が知らせる SHA-256（16 進。アセットの `digest`）。無いものは確かめられないので、勧めも走らせもしない。
    pub sha256: String,
}

/// リリースのインストーラーか（`nyaw-<版>-setup.exe`。`installer/nyaw.iss`）。区切りを含む名前は落とす先の外に出るので除く。
pub fn is_installer(name: &str) -> bool {
    name.len() > "nyaw--setup.exe".len()
        && name.starts_with("nyaw-")
        && name.ends_with("-setup.exe")
        && !name.contains(['/', '\\'])
}

/// リリースノートの末尾の見えない注釈 `<!-- nyaw-build: <番号> -->`（リリースのワークフローが足す）からビルド番号を読む。
/// ファイル名には出さない。GitHub も nyaw の Markdown も HTML のコメントは描かない。
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

/// GitHub のリリースの一覧（API の JSON）を読む。下書きと、インストーラー・その SHA-256・ビルド番号のどれかが無いものは除く。
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

/// `releases` のうち、`channel` で受け取るもの（stable はプレリリースを除く）で、ビルド番号が `build` より大きいものの中で
/// 一番新しいもの。
pub fn newest(releases: &[Release], build: u32, channel: UpdateChannel) -> Option<Release> {
    releases
        .iter()
        .filter(|r| r.build > build && (channel == UpdateChannel::Nightly || !r.prerelease))
        .max_by_key(|r| r.build)
        .cloned()
}

/// リリースの一覧を返す GitHub の API（リポジトリは Cargo.toml の `repository`）。
pub fn releases_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let api = repo.replacen("https://github.com/", "https://api.github.com/repos/", 1);
    format!("{api}/releases?per_page=30")
}

/// 一覧 `url` のリリースを読む（どれが新しいかは [`newest`] で選ぶ）。ネットワークを使うので別スレッドで呼ぶ。
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

/// `release` のインストーラーを `dir` に落とし、大きさと SHA-256 を確かめる。落とした量（バイト）を `progress` に書く。
/// 違っていれば消してエラー。ネットワークとファイルを使うので別スレッドで呼ぶ。
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

/// インストーラーに渡す引数: ウィザードもメッセージも出さずに入れ（ユーザー単位なので UAC も出ない）、`pid` のプロセス
/// （このアプリ）が終わるのを待ってから入れ替え、終えたら `relaunch`（`--novr` など）で起動し直す（`installer/nyaw.iss`）。
/// インストーラーは引数を空白でつないで渡し直すので、空白・引用符を含むものは落とす。
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

/// 落としたインストーラーを走らせ、終えたら今と同じ引数で起動し直させる（終わりは待たない。待つのはインストーラーの方）。
pub fn run_installer(path: &Path) -> Result<(), String> {
    start_installer(path, std::env::args().skip(1))
}

/// 落としたインストーラーを走らせ、終えたら `relaunch` の引数で起動し直させる。
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
        // GitHub の API は User-Agent の無い要求を断る。
        .user_agent(concat!("nyaw/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn err(e: impl Display) -> String {
    e.to_string()
}

/// 更新の状態（GPUI のグローバル）。ダッシュボードのボタンと設定のアップデートが同じものを見る。
pub struct Updater {
    /// 比べる元（今動いているビルド）・一覧の場所・落とす先・インストーラーの走らせ方。テストが差し替える。
    pub build: u32,
    pub source: String,
    pub dir: PathBuf,
    pub launch: fn(&Path) -> Result<(), String>,
    /// 最後に確かめたリリースの一覧（チャンネルを問わない）。勧めるものは [`latest`] が今のチャンネルで選ぶので、
    /// チャンネルを切り替えると確かめ直さずにすぐ変わる。
    pub found: Vec<Release>,
    pub step: Step,
    /// 一度でも確かめ終えた。
    pub checked: bool,
    /// 最後の失敗。
    pub error: Option<String>,
    /// ダッシュボードのボタンから、設定のアップデートを開くよう頼まれた（設定画面が見て下ろす）。
    pub reveal: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            build: BUILD,
            source: releases_url(),
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

/// 今していること。
#[derive(Debug, Clone, Default)]
pub enum Step {
    #[default]
    Idle,
    Checking,
    /// 落としている（落とした量。全体は `Installer::size`）。
    Downloading(Arc<AtomicU64>),
    /// インストーラーを走らせた（アプリはすぐ終わる）。
    Installing,
}

/// 今の状態（まだ何もしていなければ既定のもの）。
pub fn state(cx: &mut App) -> &Updater {
    cx.default_global::<Updater>()
}

/// 今のチャンネル（設定の `update_channel`）で勧める新しいリリース（最新なら `None`）。
pub fn latest(cx: &App) -> Option<Release> {
    let u = cx.try_global::<Updater>()?;
    newest(&u.found, u.build, ui::settings(cx).update_channel)
}

/// 新しいリリースがあるか（ダッシュボードのボタン）。
pub fn available(cx: &App) -> bool {
    latest(cx).is_some()
}

/// 新しいリリースがあるか確かめる（別スレッド）。何かしている間は何もしない。
pub fn check_now(cx: &mut App) {
    let u = cx.default_global::<Updater>();
    if !matches!(u.step, Step::Idle) {
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

/// 見つけたリリースを落とし、インストーラーを走らせてアプリを終える（インストーラーが入れ替えて起動し直す）。
/// ユーザーが押したときだけ呼ぶ。何かしている間は何もしない。
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

/// 設定のアップデートを開くよう頼む（ダッシュボードのボタン）。
pub fn reveal(cx: &mut App) {
    cx.default_global::<Updater>().reveal = true;
}

/// 頼まれていれば下ろして `true`（設定画面）。
pub fn take_reveal(cx: &mut App) -> bool {
    cx.try_global::<Updater>().is_some_and(|u| u.reveal)
        && std::mem::take(&mut cx.global_mut::<Updater>().reveal)
}

#[cfg(test)]
mod tests;
