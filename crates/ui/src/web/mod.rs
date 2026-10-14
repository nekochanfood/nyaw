//! UI 層の Web 側: Tauri の WebView でパネルを実体化する（GPUI のウィンドウと並ぶもう 1 つの見せ方）。
//! フレームループ（[`crate::run`]）は GPUI のイベントループの上で回り、毎フレーム [`Host::step`] で Tauri も 1 回回す。
//! カーネルは 1 つで、GPUI のパネルと Web のパネルは同じ PanelId の空間に並ぶ。
//!
//! - レーザー → Web: オーバーレイ層のポインタイベントを Tauri のイベント `nyaw:pointer` でそのパネルの WebView へ送り、
//!   注入したブリッジ（`bridge.js`）が DOM の PointerEvent / MouseEvent / click / wheel として発火する。
//!   ページは普通の Web ページと同じく React 等で書け、VR 上にあることを意識しない。
//!   novr もエミュレーターがコントローラを模すので同じ経路を通る。
//! - Web → カーネル: ページは Tauri のコマンド（`panel_command` / `panel_status` / `spawn_panel`、JS からは `window.nyaw`）で
//!   操作を積み、フレームループがカーネルに反映して状態の変化を Tauri のイベント `nyaw:status` で返す
//!   （GPUI の `PanelControl` / `sync_panel` と同じ形）。
//! - Web → VR: WebView を画面に出さずに描き、[`capture`] で読み戻して `Kernel::submit` で送る。
//! - リサイズ: `{ resize }` コマンドでウィンドウの大きさが変わるとパネルの解像度を合わせる
//!   （`PanelCommand::Resize`。1m あたりのピクセル数を保つので VR 上のパネルも広がる）。
//!
//! どのページかは知らない。ページのファイル（[`Files`]）は上から渡され、`nyaw://localhost/<ページ名>/index.html` で配信する。

mod capture;

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use kernel::vr::{Hand, Quat, Vec3};
use kernel::{
    Device, Kernel, PanelCommand, PanelId, PointerEvent, PointerMode, Pose, Runtime, UiEvent, Vec2,
    VrBackend,
};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalSize, RunEvent, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};

pub use capture::Image;

/// すべてのパネルに注入するブリッジ。
pub const BRIDGE: &str = include_str!("bridge.js");
/// カーネル → ページ: レーザーのポインタイベントの束（`[WebPointer]`。[`Outbox`]）。
pub const POINTER_EVENT: &str = "nyaw:pointer";
/// カーネル → ページ: パネルの状態が変わった（[`WebStatus`]）。
pub const STATUS_EVENT: &str = "nyaw:status";

/// 「HMD に取り付け」の相対姿勢: 1m 前（Testing と同じ）。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」の相対姿勢: 0.3m 前（Testing と同じ）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// 入力・状態の変化から、毎フレーム撮り直し続ける時間。ページの JS がイベントを処理して描き終わるのを待たずに済む。
// ponytail: WebView の描画完了を知る手段が無いので時間で決める。requestAnimationFrame から知らせる手もある。
const HOT: Duration = Duration::from_millis(300);
/// 入力がこれだけ無ければ、予備の WebView を作る（作る間はメインスレッドが止まるので、操作中は避ける）。
const PREWARM_IDLE: Duration = Duration::from_secs(1);
/// 終わるときに WebView が閉じ終わるのを待つ上限。
const SHUTDOWN: Duration = Duration::from_millis(1000);

/// ページのファイル（`<ページ名>/index.html` と、そこから相対パスで辿る共有の `assets/` など）。
/// `webpages::FILES` のように、ビルドしたものを埋め込んで渡す。
pub type Files = &'static [(&'static str, &'static [u8])];

// ---- ページの配信 ----

/// ページの入口の URL。Windows（WebView2）は独自スキームを `http://<scheme>.localhost` で受ける。
pub fn page_url(name: &str) -> tauri::Url {
    let base = if cfg!(windows) {
        "http://nyaw.localhost"
    } else {
        "nyaw://localhost"
    };
    format!("{base}/{name}/index.html")
        .parse()
        .expect("ページ名は URL のパスに使える")
}

fn has_page(files: Files, name: &str) -> bool {
    let entry = format!("{name}/index.html");
    files.iter().any(|(p, _)| *p == entry)
}

/// URL のパス（`/<ページ名>/<ファイル>`）に当たるファイルと Content-Type。
fn serve(files: Files, path: &str) -> Option<(&'static [u8], &'static str)> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() || path.ends_with('/') {
        format!("{path}index.html")
    } else {
        path.to_owned()
    };
    let (_, bytes) = files.iter().find(|(p, _)| *p == path)?;
    let mime = match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    Some((bytes, mime))
}

// ---- ページとのやり取り（JSON） ----

/// 取り付け先。JSON では `"hmd"` / `"left"` / `"right"` / `{ "tracker": "<ID>" }`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebDevice {
    Hmd,
    Left,
    Right,
    Tracker(String),
}

impl WebDevice {
    fn device(self) -> Device {
        match self {
            Self::Hmd => Device::Hmd,
            Self::Left => Device::Controller(Hand::Left),
            Self::Right => Device::Controller(Hand::Right),
            Self::Tracker(id) => Device::Tracker(id),
        }
    }

    fn from_device(d: &Device) -> Self {
        match d {
            Device::Hmd => Self::Hmd,
            Device::Controller(Hand::Left) => Self::Left,
            Device::Controller(Hand::Right) => Self::Right,
            Device::Tracker(id) => Self::Tracker(id.clone()),
        }
    }
}

/// ページからパネルへの操作（`nyaw.command(..)`）。JSON では `"detach"` / `"close"` / `{ "attach": "hmd" }` /
/// `{ "lock": true }` / `{ "resize": { "width": 640, "height": 480 } }` / `{ "pointer_mode": "touch" }`。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebCommand {
    Attach(WebDevice),
    Detach,
    Lock(bool),
    Animated(bool),
    Close,
    /// ウィンドウ（WebView のビューポート）を論理ピクセルでこの大きさにする。パネルの解像度も合わせる。
    Resize {
        width: u32,
        height: u32,
    },
    /// レーザーのエミュレート方式（`"mouse"` / `"touch"`）。全パネルに効く。
    PointerMode(PointerMode),
}

/// パネルの状態（`nyaw.status()` / `nyaw.onStatus(..)`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WebStatus {
    pub panel: PanelId,
    /// `"openvr"` / `"openxr"` / `"novr"`。
    pub runtime: &'static str,
    pub attached: Option<WebDevice>,
    pub locked: bool,
    pub animated: bool,
    /// 見えているトラッカーの ID（取り付け先の候補）。
    pub trackers: Vec<String>,
    /// メニューボタンを押している。
    pub menu: bool,
    /// パネルを掴んでいる。
    pub grabbed: bool,
    /// UI の論理ピクセル（ポインタの座標系）。
    pub resolution: (u32, u32),
    /// VR 上の大きさ（m）。
    pub size: (f32, f32),
    /// レーザーのエミュレート方式（`"mouse"` / `"touch"`）。
    pub pointer_mode: PointerMode,
}

impl WebStatus {
    pub fn of<B: VrBackend>(kernel: &Kernel<B>, panel: PanelId) -> Self {
        let s = kernel.status(panel);
        let p = kernel.panel(panel);
        Self {
            panel,
            runtime: match kernel.runtime() {
                Runtime::OpenVr => "openvr",
                Runtime::OpenXr => "openxr",
                Runtime::NoVr => "novr",
            },
            attached: s.attached.as_ref().map(WebDevice::from_device),
            locked: s.locked,
            animated: s.animated,
            trackers: s.trackers,
            menu: s.menu,
            grabbed: s.grabbed,
            resolution: p.resolution,
            size: (p.size.x, p.size.y),
            pointer_mode: s.pointer_mode,
        }
    }
}

/// レーザーのポインタイベント 1 件（`nyaw:pointer` の中身）。座標はパネルの論理ピクセル（左上原点）= CSS px。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WebPointer {
    /// `"left"` / `"right"`。
    pub hand: &'static str,
    /// `"move"` / `"down"` / `"up"` / `"scroll"` / `"leave"`。
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub x: f32,
    pub y: f32,
    /// スクロール量（行）。上・右に倒すと正。
    pub dx: f32,
    pub dy: f32,
    /// `"mouse"` / `"touch"`。ブリッジは押した瞬間の方式を離すまで使う。
    pub mode: PointerMode,
}

impl WebPointer {
    pub fn new(hand: Hand, ev: PointerEvent, mode: PointerMode) -> Self {
        let hand = match hand {
            Hand::Left => "left",
            Hand::Right => "right",
        };
        let (kind, pos, delta) = match ev {
            PointerEvent::Move { pos } => ("move", pos, Vec2::ZERO),
            PointerEvent::Down { pos } => ("down", pos, Vec2::ZERO),
            PointerEvent::Up { pos } => ("up", pos, Vec2::ZERO),
            PointerEvent::Scroll { pos, delta } => ("scroll", pos, delta),
            PointerEvent::Leave => ("leave", Vec2::ZERO, Vec2::ZERO),
        };
        Self {
            hand,
            kind,
            x: pos.x,
            y: pos.y,
            dx: delta.x,
            dy: delta.y,
            mode,
        }
    }
}

/// ページへ送るのを待っているポインタイベント（パネル 1 枚分）。ページが前の束を処理し終える（`pointer_ack`）まで
/// 次を送らずに溜め、溜まっている間は同じ手の Move は最新だけ、Scroll は合算する。
/// スティックを倒し続けるとスクロールが毎フレーム来るので、こうしないと JS が追いつかず遅れが積み上がる。
#[derive(Debug, Default)]
pub struct Outbox {
    queue: Vec<WebPointer>,
    /// 送って返事を待っている束を送った時刻。
    sent: Option<Instant>,
}

/// 返事が来なくても次を送るまでの時間（ページの読み込み直し等で返事が失われたとき）。
const ACK_TIMEOUT: Duration = Duration::from_millis(500);

impl Outbox {
    pub fn push(&mut self, p: WebPointer) {
        let last = self.queue.iter_mut().rev().find(|q| q.hand == p.hand);
        match last {
            Some(last) if last.kind == "move" && p.kind == "move" => *last = p,
            Some(last) if last.kind == "scroll" && p.kind == "scroll" => {
                (last.x, last.y) = (p.x, p.y);
                (last.dx, last.dy) = (last.dx + p.dx, last.dy + p.dy);
            }
            _ => self.queue.push(p),
        }
    }

    /// 送ってよければ、溜まっている束を取り出す。
    pub fn take(&mut self, now: Instant) -> Option<Vec<WebPointer>> {
        let waiting = self.sent.is_some_and(|t| now - t < ACK_TIMEOUT);
        if self.queue.is_empty() || waiting {
            return None;
        }
        self.sent = Some(now);
        Some(std::mem::take(&mut self.queue))
    }

    /// ページが前の束を処理し終えた。
    pub fn ack(&mut self) {
        self.sent = None;
    }
}

// ---- コマンド（ページ → カーネル） ----

/// コマンドとフレームループの受け渡し口（Tauri の管理状態）。UI 層の `PanelControl` に当たる。
struct Shared {
    control: Mutex<Control>,
    files: Files,
}

impl Shared {
    /// ウィンドウのラベルのパネル。予備から割り当てたウィンドウ（`spare-N`）は対応表で引く。
    fn panel_of(&self, label: &str) -> Result<PanelId, String> {
        let mapped = self
            .control
            .lock()
            .map_err(|e| e.to_string())?
            .labels
            .get(label)
            .copied();
        mapped.map_or_else(|| panel_of(label), Ok)
    }
}

#[derive(Default)]
struct Control {
    commands: HashMap<PanelId, Vec<WebCommand>>,
    statuses: HashMap<PanelId, WebStatus>,
    /// (開いたパネル, ページ名)。
    spawns: Vec<(PanelId, String)>,
    /// ポインタの束を処理し終えたパネル（[`Outbox::ack`]）。
    acks: Vec<PanelId>,
    /// 予備から割り当てたウィンドウのラベル → パネル（`panel-N` でないもの）。
    labels: HashMap<String, PanelId>,
}

/// パネルの WebView のウィンドウのラベル（予備から割り当てたものは `spare-N`）。ページへイベントを送る宛先（テスト用）。
pub fn label_of(app: &AppHandle, panel: PanelId) -> String {
    let mapped = app.try_state::<Arc<Shared>>().and_then(|s| {
        let ctl = s.control.lock().ok()?;
        ctl.labels
            .iter()
            .find(|(_, p)| **p == panel)
            .map(|(l, _)| l.clone())
    });
    mapped.unwrap_or_else(|| label(panel))
}

fn label(panel: PanelId) -> String {
    format!("panel-{panel}")
}

fn panel_of(label: &str) -> Result<PanelId, String> {
    label
        .strip_prefix("panel-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("パネルのウィンドウではない: {label}"))
}

#[tauri::command]
fn panel_command(
    webview: tauri::Webview,
    shared: tauri::State<'_, Arc<Shared>>,
    command: WebCommand,
) -> Result<(), String> {
    let panel = shared.panel_of(webview.label())?;
    logger::debug!("ページからの操作", panel = panel, command = ?command);
    let mut ctl = shared.control.lock().map_err(|e| e.to_string())?;
    ctl.commands.entry(panel).or_default().push(command);
    Ok(())
}

#[tauri::command]
fn panel_status(
    webview: tauri::Webview,
    shared: tauri::State<'_, Arc<Shared>>,
) -> Result<WebStatus, String> {
    let panel = shared.panel_of(webview.label())?;
    let ctl = shared.control.lock().map_err(|e| e.to_string())?;
    ctl.statuses
        .get(&panel)
        .cloned()
        .ok_or_else(|| format!("パネル {panel} の状態がまだない"))
}

/// ブリッジが `nyaw:pointer` の束を DOM に流し終えた。
#[tauri::command]
fn pointer_ack(
    webview: tauri::Webview,
    shared: tauri::State<'_, Arc<Shared>>,
) -> Result<(), String> {
    let panel = shared.panel_of(webview.label())?;
    let mut ctl = shared.control.lock().map_err(|e| e.to_string())?;
    ctl.acks.push(panel);
    Ok(())
}

#[tauri::command]
fn spawn_panel(
    webview: tauri::Webview,
    shared: tauri::State<'_, Arc<Shared>>,
    page: String,
) -> Result<(), String> {
    let panel = shared.panel_of(webview.label())?;
    if !has_page(shared.files, &page) {
        return Err(format!("知らないページ: {page}"));
    }
    let mut ctl = shared.control.lock().map_err(|e| e.to_string())?;
    ctl.spawns.push((panel, page));
    Ok(())
}

// ---- Tauri の口 ----

fn context() -> tauri::Context<Wry> {
    tauri::generate_context!()
}

/// `run_iteration` の中で受けたウィンドウのイベント。
enum WinEvent {
    Resized(String, PhysicalSize<u32>),
    Destroyed(String),
}

/// パネルごとの描画（読み戻し）の状態。
#[derive(Default)]
struct Draw {
    capturing: bool,
    captured: Option<Instant>,
    hot_until: Option<Instant>,
    last: Option<Image>,
}

/// Web のパネルを持つ Tauri アプリ。メインスレッド専用で、イベントループは持たず [`Self::step`] で回してもらう。
/// GPUI と Tauri はどちらもメインスレッドのイベントループを使うので、GPUI のループの上で Tauri を 1 回ずつ回す。
pub struct Host {
    app: tauri::App<Wry>,
    shared: Arc<Shared>,
    windows: HashMap<PanelId, WebviewWindow<Wry>>,
    draws: HashMap<PanelId, Draw>,
    outboxes: HashMap<PanelId, Outbox>,
    /// (パネル, 手) ごとに最後に送った Move の位置。
    moved: HashMap<(PanelId, Hand), Vec2>,
    events: Rc<RefCell<Vec<WinEvent>>>,
    tx: Sender<(PanelId, Result<Image, String>)>,
    rx: Receiver<(PanelId, Result<Image, String>)>,
    warned: bool,
    /// 予備の WebView。作るのにメインスレッドが 100ms 近く止まるので、入力の無いときに作っておき、開くときはページを読ませるだけにする。
    spare: Option<WebviewWindow<Wry>>,
    spares: u64,
    /// 最後にポインタが届いた・パネルを開いた時刻（予備は落ち着いているときに作る）。
    active: Instant,
}

impl Host {
    /// Tauri を起こす。1 プロセスで 1 回だけ（イベントループは作り直せない）。
    pub fn new(files: Files) -> tauri::Result<Self> {
        let shared = Arc::new(Shared {
            control: Mutex::default(),
            files,
        });
        let app = tauri::Builder::<Wry>::default()
            .manage(shared.clone())
            .register_uri_scheme_protocol("nyaw", move |_, req| {
                let res = tauri::http::Response::builder();
                match serve(files, req.uri().path()) {
                    Some((body, mime)) => res
                        .header(tauri::http::header::CONTENT_TYPE, mime)
                        .body(Cow::Borrowed(body)),
                    None => res.status(404).body(Cow::Borrowed(&b"not found"[..])),
                }
                .expect("ヘッダーは固定の値")
            })
            .invoke_handler(tauri::generate_handler![
                panel_command,
                panel_status,
                pointer_ack,
                spawn_panel
            ])
            .build(context())?;
        let (tx, rx) = channel();
        logger::info!(
            "Web（Tauri）を起動した",
            pages = files
                .iter()
                .filter(|(p, _)| p.ends_with("/index.html"))
                .count()
        );
        Ok(Self {
            app,
            shared,
            windows: HashMap::new(),
            draws: HashMap::new(),
            outboxes: HashMap::new(),
            moved: HashMap::new(),
            events: Rc::default(),
            tx,
            rx,
            warned: false,
            spare: None,
            spares: 0,
            active: Instant::now(),
        })
    }

    pub fn handle(&self) -> &AppHandle {
        self.app.handle()
    }

    /// 終わる前に WebView のウィンドウを閉じ、閉じ終わるまで（最長 [`SHUTDOWN`]）Tauri を回す。
    /// 開いたままプロセスが終わると WebView2 のブラウザが後始末を終えられず（"Failed to unregister class
    /// Chrome_WidgetWin_0" が出る）、続けて起動したときに同じデータフォルダの WebView を作れないことがある。
    pub fn shutdown(&mut self) {
        for w in self
            .windows
            .drain()
            .map(|(_, w)| w)
            .chain(self.spare.take())
        {
            if let Err(e) = w.destroy() {
                logger::warn!("Web のパネルを閉じられない", error = e);
            }
        }
        let deadline = Instant::now() + SHUTDOWN;
        while !self.app.webview_windows().is_empty() && Instant::now() < deadline {
            self.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        self.app.cleanup_before_exit();
        logger::info!("Web（Tauri）を閉じた");
    }

    /// `panel` が開いている Web のパネルか。
    pub fn contains(&self, panel: PanelId) -> bool {
        self.windows.contains_key(&panel)
    }

    /// カーネルに作ったパネル `panel` に、ページ `page` の WebView のウィンドウを開く。開けなければパネルを閉じる。
    /// GPUI のパネルと同じく背景を透かす（塗るかどうかはページが決める）。
    pub fn open<B: VrBackend>(&mut self, kernel: &mut Kernel<B>, panel: PanelId, page: &str) {
        if !has_page(self.shared.files, page) {
            logger::error!("知らない Web のページ", page = page, panel = panel);
            kernel.apply(panel, PanelCommand::Close);
            return;
        }
        let (w, h) = kernel.panel(panel).resolution;
        self.active = Instant::now();
        let window = match self.spare.take() {
            // 予備があればページを読ませるだけ（作り直さないのでメインスレッドが止まらない）。
            Some(spare) => {
                self.lock().labels.insert(spare.label().to_owned(), panel);
                let r = spare
                    .set_size(LogicalSize::new(w, h))
                    .and_then(|_| spare.set_title(page))
                    .and_then(|_| spare.navigate(page_url(page)));
                r.map(|_| spare)
            }
            None => self.build(
                label(panel),
                WebviewUrl::CustomProtocol(page_url(page)),
                (w, h),
            ),
        };
        match window {
            Ok(window) => {
                logger::info!("Web のパネルを開いた", panel = panel, page = page);
                self.windows.insert(panel, window);
                self.draws.insert(panel, Draw::default());
                self.outboxes.insert(panel, Outbox::default());
            }
            Err(e) => {
                logger::error!("Web のパネルを開けない", panel = panel, error = e);
                kernel.apply(panel, PanelCommand::Close);
            }
        }
    }

    /// パネル用の WebView のウィンドウを作る（メインスレッドが 100ms 近く止まる）。
    fn build(
        &self,
        label: String,
        url: WebviewUrl,
        (w, h): (u32, u32),
    ) -> tauri::Result<WebviewWindow<Wry>> {
        let builder = WebviewWindowBuilder::new(&self.app, label, url)
            .inner_size(w as f64, h as f64)
            .initialization_script(BRIDGE);
        // 描画は VR に送るので画面外へ。枠を消すと Windows の WebView の描画領域が縮むため残す。
        offscreen(builder)
            .transparent(true)
            .focused(false)
            .skip_taskbar(true)
            .build()
    }

    /// 入力が [`PREWARM_IDLE`] 無ければ、次に開くパネルのための予備の WebView を作っておく。
    fn prewarm(&mut self, now: Instant) {
        if self.spare.is_some() || now - self.active < PREWARM_IDLE {
            return;
        }
        let t = Instant::now();
        self.spares += 1;
        let url = WebviewUrl::External("about:blank".parse().expect("固定の URL"));
        match self.build(format!("spare-{}", self.spares), url, crate::PANEL_PX) {
            Ok(w) => {
                logger::debug!("予備の WebView を作った", ms = t.elapsed().as_millis());
                self.spare = Some(w);
            }
            Err(e) => {
                logger::warn!("予備の WebView を作れない", error = e);
                // 作れないときに毎フレーム試さない。
                self.active = now;
            }
        }
    }

    /// オーバーレイ層のポインタイベントを、このパネルへ送る束に積む。
    pub fn deliver(&mut self, e: UiEvent, mode: PointerMode, now: Instant) {
        if !self.windows.contains_key(&e.panel) {
            return;
        }
        self.active = now;
        // オーバーレイは指している間 毎 tick Move を出す。同じ位置なら送らない（JS とイベントループを詰まらせない）。
        let at = (e.panel, e.hand);
        match e.event {
            PointerEvent::Move { pos } if self.moved.get(&at) == Some(&pos) => return,
            PointerEvent::Move { pos } => {
                self.moved.insert(at, pos);
            }
            PointerEvent::Leave => {
                self.moved.remove(&at);
            }
            _ => {}
        }
        if let Some(o) = self.outboxes.get_mut(&e.panel) {
            o.push(WebPointer::new(e.hand, e.event, mode));
        }
        if let Some(d) = self.draws.get_mut(&e.panel) {
            d.hot_until = Some(now + HOT);
        }
    }

    /// 1 フレーム: Tauri を 1 回回し、ウィンドウのイベントと読み戻しを処理し、ポインタを送り、ページの操作を反映し、
    /// 撮り直す。ページが開くよう頼んだ (元のパネル, ページ名) を返す。
    pub fn step<B: VrBackend>(
        &mut self,
        kernel: &mut Kernel<B>,
        now: Instant,
    ) -> Vec<(PanelId, String)> {
        self.pump();
        let events = std::mem::take(&mut *self.events.borrow_mut());
        for e in events {
            match e {
                WinEvent::Resized(label, size) => self.resized(kernel, &label, size, now),
                WinEvent::Destroyed(label) => self.destroyed(kernel, &label),
            }
        }
        self.receive(kernel);
        let acks = std::mem::take(&mut self.lock().acks);
        for panel in acks {
            if let Some(o) = self.outboxes.get_mut(&panel) {
                o.ack();
            }
        }
        let app = self.app.handle().clone();
        for (&panel, window) in &self.windows {
            if let Some(batch) = self.outboxes.get_mut(&panel).and_then(|o| o.take(now))
                && let Err(err) = app.emit_to(window.label(), POINTER_EVENT, batch)
            {
                logger::warn!("ポインタを送れない", panel = panel, error = err);
            }
        }
        let panels: Vec<PanelId> = self.windows.keys().copied().collect();
        for panel in panels {
            self.sync(kernel, panel, now);
        }
        self.capture(now);
        self.prewarm(now);
        std::mem::take(&mut self.lock().spawns)
    }

    /// Tauri のイベントループを 1 回回す（待たない）。
    #[allow(deprecated)] // run_iteration は「ループで呼ぶと busy-loop する」ため非推奨。周期は GPUI のループが作る。
    fn pump(&mut self) {
        let events = self.events.clone();
        self.app.run_iteration(move |_, event| match event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::Resized(size),
                ..
            } => events.borrow_mut().push(WinEvent::Resized(label, size)),
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::Destroyed,
                ..
            } => events.borrow_mut().push(WinEvent::Destroyed(label)),
            RunEvent::ExitRequested { api, .. } => {
                // 最後の Web のウィンドウが閉じても終わらない（終わるかどうかは UI 層が決める）。
                api.prevent_exit();
            }
            _ => {}
        });
        // WebKitGTK の描画・読み戻しは GLib のイベントで進む。溜まっている分を処理する。
        #[cfg(target_os = "linux")]
        for _ in 0..256 {
            if !gtk::events_pending() {
                break;
            }
            gtk::main_iteration_do(false);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// ページが積んだ操作を `panel` に反映し、状態が変わったらページへ知らせる。GPUI の `sync_panel` に当たる。
    fn sync<B: VrBackend>(&mut self, kernel: &mut Kernel<B>, panel: PanelId, now: Instant) {
        if kernel.panel(panel).closed {
            return;
        }
        let commands = self.lock().commands.remove(&panel).unwrap_or_default();
        for c in commands {
            self.command(kernel, panel, c);
        }
        if kernel.panel(panel).closed {
            return;
        }
        let status = WebStatus::of(kernel, panel);
        let changed = self.lock().statuses.insert(panel, status.clone()) != Some(status.clone());
        if changed {
            let to = self.windows.get(&panel).map(|w| w.label().to_owned());
            if let Some(to) = to
                && let Err(e) = self.app.emit_to(to.as_str(), STATUS_EVENT, status)
            {
                logger::warn!("状態を送れない", panel = panel, error = e);
            }
            if let Some(d) = self.draws.get_mut(&panel) {
                d.hot_until = Some(now + HOT);
            }
        }
    }

    fn command<B: VrBackend>(&mut self, kernel: &mut Kernel<B>, panel: PanelId, c: WebCommand) {
        match c {
            WebCommand::Attach(d) => {
                let offset = if d == WebDevice::Hmd {
                    HMD_OFFSET
                } else {
                    HAND_OFFSET
                };
                kernel.apply(panel, PanelCommand::Attach(d.device(), offset));
            }
            WebCommand::Detach => kernel.apply(panel, PanelCommand::Detach),
            WebCommand::Lock(l) => kernel.apply(panel, PanelCommand::Lock(l)),
            WebCommand::Animated(a) => kernel.apply(panel, PanelCommand::Animated(a)),
            WebCommand::Close => {
                kernel.apply(panel, PanelCommand::Close);
                self.forget(panel);
            }
            WebCommand::PointerMode(m) => kernel.apply(panel, PanelCommand::PointerMode(m)),
            WebCommand::Resize { width, height } => {
                let (width, height) = (width.max(1), height.max(1));
                if let Some(w) = self.windows.get(&panel)
                    && let Err(e) = w.set_size(LogicalSize::new(width, height))
                {
                    logger::warn!("ウィンドウの大きさを変えられない", panel = panel, error = e);
                }
                // 画面に出ていないウィンドウは Resized が来ないことがあるので、ここでも合わせる。
                kernel.apply(panel, PanelCommand::Resize(width, height));
            }
        }
    }

    /// パネルのウィンドウを閉じて忘れる。
    fn forget(&mut self, panel: PanelId) {
        if let Some(w) = self.windows.remove(&panel)
            && let Err(e) = w.destroy()
        {
            logger::warn!("ウィンドウを閉じられない", panel = panel, error = e);
        }
        self.draws.remove(&panel);
        self.outboxes.remove(&panel);
        self.moved.retain(|(p, _), _| *p != panel);
        self.lock().labels.retain(|_, p| *p != panel);
    }

    /// ウィンドウの大きさが変わった（OS の操作・`resize` コマンド）。パネルの解像度を論理ピクセルで合わせる。
    fn resized<B: VrBackend>(
        &mut self,
        kernel: &mut Kernel<B>,
        label: &str,
        size: PhysicalSize<u32>,
        now: Instant,
    ) {
        let Ok(panel) = self.shared.panel_of(label) else {
            return;
        };
        let Some(window) = self.windows.get(&panel) else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return; // 最小化
        }
        let scale = window.scale_factor().unwrap_or(1.0);
        let logical = size.to_logical::<f64>(scale);
        let res = (logical.width.round() as u32, logical.height.round() as u32);
        if kernel.panel(panel).resolution != res {
            logger::debug!("パネルの大きさが変わった", panel = panel, resolution = ?res);
            kernel.apply(panel, PanelCommand::Resize(res.0, res.1));
            self.sync(kernel, panel, now);
        }
    }

    /// ウィンドウが閉じられた（OS の X など）。そのパネルも閉じる。
    fn destroyed<B: VrBackend>(&mut self, kernel: &mut Kernel<B>, label: &str) {
        let Ok(panel) = self.shared.panel_of(label) else {
            return;
        };
        if self.windows.contains_key(&panel) {
            self.windows.remove(&panel);
            self.forget(panel);
            if !kernel.panel(panel).closed {
                kernel.apply(panel, PanelCommand::Close);
            }
        }
    }

    /// 撮り直すパネルの WebView を撮る（非同期。結果は [`Self::receive`] で受ける）。
    fn capture(&mut self, now: Instant) {
        for (&panel, window) in &self.windows {
            let Some(d) = self.draws.get_mut(&panel) else {
                continue;
            };
            let due = d.hot_until.is_some_and(|t| now < t)
                || d.captured.is_none_or(|t| now - t >= crate::IDLE_REDRAW);
            if d.capturing || !due {
                continue;
            }
            d.capturing = true;
            d.captured = Some(now);
            let tx = self.tx.clone();
            capture::capture(
                window,
                Box::new(move |r| {
                    let _ = tx.send((panel, r));
                }),
            );
        }
    }

    /// 読み戻した描画を VR へ送る。画素が変わったときだけ（GPU への書き込みと完了待ちを省く）。
    fn receive<B: VrBackend>(&mut self, kernel: &mut Kernel<B>) {
        while let Ok((panel, r)) = self.rx.try_recv() {
            let Some(d) = self.draws.get_mut(&panel) else {
                continue; // 閉じた
            };
            d.capturing = false;
            if let Some(t) = d.captured {
                logger::trace!("読み戻した", panel = panel, ms = t.elapsed().as_millis());
            }
            match r {
                // 開いた直後は大きさの反映が間に合わないことがある。送らずに次で撮り直す。
                Ok(img) if img.width <= 1 || img.height <= 1 => {}
                Ok(img) => {
                    if d.last.as_ref() != Some(&img) {
                        kernel.submit(panel, img.width, img.height, &img.rgba);
                        d.last = Some(img);
                    }
                }
                Err(e) if !self.warned => {
                    logger::error!("Web のパネルの描画を取得できない", panel = panel, error = e);
                    self.warned = true;
                }
                Err(_) => {}
            }
        }
    }
}

/// パネルのウィンドウをデスクトップに出さない。
/// WebKitGTK は未マップ（非表示）のウィンドウを描かず撮れないので、表示したまま画面の外に置く。
// ponytail: Wayland ではアプリがウィンドウの位置を決められず画面に出てしまう。X11（XWayland 含む）のみ。
// GDK_BACKEND=x11 で起動するか、オフスクリーン描画（WPE 等）に移すのが次の一手。
#[cfg(target_os = "linux")]
fn offscreen<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App<Wry>>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App<Wry>> {
    b.visible(true).position(-20000.0, -20000.0)
}

/// パネルのウィンドウをデスクトップに出さない（非表示のまま描かせて撮る）。
#[cfg(not(target_os = "linux"))]
fn offscreen<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App<Wry>>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App<Wry>> {
    b.visible(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::vr::mock::MockBackend;

    #[test]
    fn pointer_events_become_json_for_the_bridge() {
        let p = WebPointer::new(
            Hand::Left,
            PointerEvent::Scroll {
                pos: Vec2::new(10.0, 20.0),
                delta: Vec2::new(0.0, -2.0),
            },
            PointerMode::Touch,
        );
        assert_eq!(
            serde_json::to_value(p).unwrap(),
            serde_json::json!({ "hand": "left", "type": "scroll", "x": 10.0, "y": 20.0, "dx": 0.0, "dy": -2.0, "mode": "touch" })
        );
        let leave = WebPointer::new(Hand::Right, PointerEvent::Leave, PointerMode::Mouse);
        assert_eq!((leave.hand, leave.kind), ("right", "leave"));
    }

    fn ptr(hand: Hand, ev: PointerEvent) -> WebPointer {
        WebPointer::new(hand, ev, PointerMode::Mouse)
    }

    #[test]
    fn outbox_waits_for_ack_and_coalesces_meanwhile() {
        let t0 = Instant::now();
        let at = |x: f32| Vec2::new(x, 0.0);
        let mut o = Outbox::default();
        assert_eq!(o.take(t0), None);
        o.push(ptr(Hand::Right, PointerEvent::Move { pos: at(1.0) }));
        assert_eq!(o.take(t0).map(|b| b.len()), Some(1));
        // 返事待ちの間: 同じ手の Move は最新だけ、Scroll は合算、押す・離すは順に残す。
        for x in [2.0, 3.0] {
            o.push(ptr(Hand::Right, PointerEvent::Move { pos: at(x) }));
        }
        o.push(ptr(Hand::Left, PointerEvent::Move { pos: at(9.0) }));
        o.push(ptr(Hand::Right, PointerEvent::Down { pos: at(3.0) }));
        for _ in 0..3 {
            let delta = Vec2::new(0.0, -1.0);
            o.push(ptr(
                Hand::Right,
                PointerEvent::Scroll {
                    pos: at(4.0),
                    delta,
                },
            ));
        }
        assert_eq!(o.take(t0), None, "返事待ち");
        o.ack();
        let batch = o.take(t0).unwrap();
        let kinds: Vec<_> = batch.iter().map(|p| (p.hand, p.kind, p.x, p.dy)).collect();
        assert_eq!(
            kinds,
            [
                ("right", "move", 3.0, 0.0),
                ("left", "move", 9.0, 0.0),
                ("right", "down", 3.0, 0.0),
                ("right", "scroll", 4.0, -3.0),
            ]
        );
        // 返事が失われても、しばらくしたら次を送る。
        o.push(ptr(Hand::Right, PointerEvent::Leave));
        assert_eq!(o.take(t0), None);
        assert!(o.take(t0 + ACK_TIMEOUT).is_some());
    }

    #[test]
    fn commands_parse_from_page_json() {
        let parse = |s: &str| serde_json::from_str::<WebCommand>(s).unwrap();
        assert_eq!(parse(r#""detach""#), WebCommand::Detach);
        assert_eq!(parse(r#"{"lock":true}"#), WebCommand::Lock(true));
        assert_eq!(parse(r#"{"animated":false}"#), WebCommand::Animated(false));
        assert_eq!(
            parse(r#"{"attach":"left"}"#),
            WebCommand::Attach(WebDevice::Left)
        );
        assert_eq!(
            parse(r#"{"attach":{"tracker":"LHR-1"}}"#),
            WebCommand::Attach(WebDevice::Tracker("LHR-1".into()))
        );
        assert_eq!(
            parse(r#"{"resize":{"width":640,"height":480}}"#),
            WebCommand::Resize {
                width: 640,
                height: 480
            }
        );
        assert_eq!(
            parse(r#"{"pointer_mode":"touch"}"#),
            WebCommand::PointerMode(PointerMode::Touch)
        );
    }

    #[test]
    fn status_reports_attach_lock_and_resolution() {
        let mut k = Kernel::with_backend(MockBackend::default(), Default::default());
        let p = k
            .add_panel("t", Pose::IDENTITY, Vec2::new(0.8, 0.6), (800, 600))
            .unwrap();
        k.apply(
            p,
            PanelCommand::Attach(Device::Controller(Hand::Left), HAND_OFFSET),
        );
        k.apply(p, PanelCommand::Lock(true));
        k.apply(p, PanelCommand::Animated(false));
        k.apply(p, PanelCommand::Resize(400, 300));
        let s = WebStatus::of(&k, p);
        assert!(!s.animated);
        assert_eq!(
            (
                s.runtime,
                s.attached.clone(),
                s.locked,
                s.resolution,
                s.size
            ),
            (
                "openxr",
                Some(WebDevice::Left),
                true,
                (400, 300),
                (0.4, 0.3)
            )
        );
        assert_eq!(
            serde_json::to_value(&s).unwrap()["attached"],
            serde_json::json!("left")
        );
    }

    #[test]
    fn pages_are_served_by_path() {
        static FILES: &[(&str, &[u8])] =
            &[("demo/index.html", b"<p>hi</p>"), ("assets/app.js", b"1")];
        assert_eq!(
            serve(FILES, "/demo/index.html"),
            Some((&b"<p>hi</p>"[..], "text/html; charset=utf-8"))
        );
        assert_eq!(serve(FILES, "/demo/"), serve(FILES, "/demo/index.html"));
        // ページから ../assets/ で辿る共有のファイル。
        assert_eq!(
            serve(FILES, "/assets/app.js").map(|(_, m)| m),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(serve(FILES, "/demo/nope.js"), None);
        assert!(has_page(FILES, "demo"));
        assert!(!has_page(FILES, "assets"));
        assert_eq!(page_url("demo").path(), "/demo/index.html");
    }

    #[test]
    fn panel_labels_round_trip() {
        assert_eq!(panel_of(&label(7)), Ok(7));
        assert!(panel_of("main").is_err());
    }
}
