//! Layer 5（UI 層と同格）: Tauri の Web フロントエンド。各オーバーレイパネルは Tauri の WebView ウィンドウで実体化される。
//!
//! - レーザー → Web: オーバーレイ層のポインタイベントを Tauri のイベント `nyaw:pointer` でそのパネルの WebView へ送り、
//!   注入したブリッジ（`bridge.js`）が DOM の PointerEvent / MouseEvent / click / wheel として発火する。
//!   画面は React のポインタイベントで書け、VR 上にあることを意識しない。
//!   novr（`Surface::Desktop`）では WebView を OS のウィンドウとして出すので、OS のマウスがそのまま届く。
//! - Web → カーネル: ページは Tauri のコマンド（`panel_command` / `panel_status` / `spawn_panel`、JS からは `window.nyaw`）で
//!   操作を積み、フレームループがカーネルに反映して状態の変化を Tauri のイベント `nyaw:status` で返す
//!   （UI 層の `PanelControl` / `sync_panel` と同じ形）。
//! - Web → VR: `Surface::Overlay` では WebView を非表示のウィンドウで描き、[`capture`] で読み戻して `Kernel::submit` で送る。
//! - リサイズ: ウィンドウの大きさが変わると（OS の操作でも `{ resize }` コマンドでも）パネルの解像度を合わせる
//!   （`PanelCommand::Resize`。1m あたりのピクセル数を保つので VR 上のパネルも広がる）。
//!
//! どの画面かは知らない。ページ（[`Page`]）は React のルート名を持ち、`nyaw://` プロトコルで共通アプリを配信する。

mod capture;

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use kernel::vr::{Hand, Quat, Vec3};
use kernel::{
    Device, Kernel, PanelCommand, PanelId, Pointer, PointerEvent, Pose, Runtime, Surface, Vec2,
    VrBackend,
};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalSize, PhysicalSize, RunEvent, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};

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

const PANEL_PX: (u32, u32) = (800, 600);
/// Desktop（novr）の周期。VR ランタイムは `poll` が vsync まで待つ。
// ponytail: Tauri はモニタのリフレッシュレートを出さないので約 90Hz に固定。
const FRAME: Duration = Duration::from_millis(11);
/// 入力も状態の変化も無いパネルを撮り直す間隔。ページが自分で動かす表示（時計など）はこれで反映される。
const IDLE_REDRAW: Duration = Duration::from_millis(250);
/// 入力・状態の変化から、毎フレーム撮り直し続ける時間。ページの JS がイベントを処理して描き終わるのを待たずに済む。
// ponytail: WebView の描画完了を知る手段が無いので時間で決める。requestAnimationFrame から知らせる手もある。
const HOT: Duration = Duration::from_millis(300);

/// 読み戻した 1 フレーム（RGBA8、ストレートアルファ、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

// ---- ページ ----

/// Web の画面（UI 層でいう画面のビュー）。`nyaw://localhost/<name>` が React のルート。
#[derive(Debug, Clone)]
pub struct Page {
    name: &'static str,
}

impl Page {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// ページの入口の URL。Windows（WebView2）は独自スキームを `http://<scheme>.localhost` で受ける。
pub fn page_url(name: &str) -> tauri::Url {
    let base = if cfg!(windows) {
        "http://nyaw.localhost"
    } else {
        "nyaw://localhost"
    };
    format!("{base}/{name}")
        .parse()
        .expect("ページ名は URL のパスに使える")
}

/// 登録した画面のルートだけを共通 HTML にし、アセットは実ファイルのパスを使う。
fn asset_path<'a>(pages: &HashMap<&'static str, Page>, path: &'a str) -> Option<&'a str> {
    let name = path.strip_prefix('/')?;
    if name.starts_with("assets/") {
        Some(path)
    } else if pages.contains_key(name.trim_end_matches('/')) {
        Some("/index.html")
    } else {
        None
    }
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
/// `{ "lock": true }` / `{ "resize": { "width": 640, "height": 480 } }`。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebCommand {
    Attach(WebDevice),
    Detach,
    Lock(bool),
    Close,
    /// ウィンドウ（WebView のビューポート）を論理ピクセルでこの大きさにする。パネルの解像度も合わせる。
    Resize {
        width: u32,
        height: u32,
    },
}

/// パネルの状態（`nyaw.status()` / `nyaw.onStatus(..)`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WebStatus {
    pub panel: PanelId,
    /// `"openvr"` / `"openxr"` / `"novr"`。
    pub runtime: &'static str,
    pub attached: Option<WebDevice>,
    pub locked: bool,
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
            trackers: s.trackers,
            menu: s.menu,
            grabbed: s.grabbed,
            resolution: p.resolution,
            size: (p.size.x, p.size.y),
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
}

impl WebPointer {
    pub fn new(hand: Hand, ev: PointerEvent) -> Self {
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

// ---- パネルの指定 ----

/// ページと VR 空間での配置（UI 層の `PanelSpec` に当たる）。
pub struct WebPanelSpec {
    page: Page,
    pose: Pose,
    from: Option<PanelId>,
    /// `pose` を開いた瞬間の頭（向きは水平方向だけ）からの相対姿勢とする。
    in_view: bool,
    size: Vec2,
    resolution: (u32, u32),
    attach: Option<(Device, Pose)>,
    locked: bool,
    pointer: Option<(Hand, Pointer)>,
    /// 背景を透かす（ページの背景が透明なら VR でも透ける）。
    transparent: bool,
}

impl WebPanelSpec {
    pub fn new(pose: Pose, page: Page) -> Self {
        Self {
            page,
            pose,
            from: None,
            in_view: false,
            size: Vec2::new(0.8, 0.6),
            resolution: PANEL_PX,
            attach: None,
            locked: false,
            pointer: None,
            transparent: false,
        }
    }

    fn spawn(from: PanelId, page: Page) -> Self {
        Self {
            from: Some(from),
            ..Self::new(Pose::IDENTITY, page)
        }
    }

    /// 開いた瞬間の目の前に出す。`offset` は頭から見た相対姿勢（頭の傾きは無視し、パネルは立てて置く）。
    pub fn in_view(mut self, offset: Pose) -> Self {
        (self.pose, self.from, self.in_view) = (offset, None, true);
        self
    }

    /// 大きさ（m）と解像度（論理 px = CSS px）。既定は 0.8m × 0.6m / 800 × 600。
    pub fn sized(mut self, size: Vec2, resolution: (u32, u32)) -> Self {
        (self.size, self.resolution) = (size, resolution);
        self
    }

    pub fn attached(mut self, device: Device, offset: Pose) -> Self {
        self.attach = Some((device, offset));
        self
    }

    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// `hand` のレーザー（線またはカーソル）を描くパネルにする。置き場所はオーバーレイ層が決める。
    pub fn pointer(mut self, hand: Hand, part: Pointer) -> Self {
        self.pointer = Some((hand, part));
        self
    }

    pub fn transparent(mut self) -> Self {
        self.transparent = true;
        self
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    pub fn pose_for<B: VrBackend>(&self, kernel: &Kernel<B>, next: PanelId) -> Pose {
        match self.from {
            Some(from) => {
                kernel.panel(from).pose
                    * Pose::new(
                        Vec3::new(1.2 * (next - from) as f32, 0.0, 0.0),
                        Quat::IDENTITY,
                    )
            }
            None if self.in_view => kernel.hmd().leveled() * self.pose,
            None => self.pose,
        }
    }
}

// ---- コマンド（ページ → カーネル） ----

/// コマンドとフレームループの受け渡し口（Tauri の管理状態）。UI 層の `PanelControl` に当たる。
struct Shared {
    control: Mutex<Control>,
    pages: HashMap<&'static str, Page>,
}

#[derive(Default)]
struct Control {
    commands: HashMap<PanelId, Vec<WebCommand>>,
    statuses: HashMap<PanelId, WebStatus>,
    /// (開いたパネル, ページ名)。
    spawns: Vec<(PanelId, &'static str)>,
    /// ポインタの束を処理し終えたパネル（[`Outbox::ack`]）。
    acks: Vec<PanelId>,
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
    let panel = panel_of(webview.label())?;
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
    let panel = panel_of(webview.label())?;
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
    let panel = panel_of(webview.label())?;
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
    let panel = panel_of(webview.label())?;
    let (&name, _) = shared
        .pages
        .get_key_value(page.as_str())
        .ok_or_else(|| format!("知らないページ: {page}"))?;
    let mut ctl = shared.control.lock().map_err(|e| e.to_string())?;
    ctl.spawns.push((panel, name));
    Ok(())
}

// ---- 起動とフレームループ ----

fn context() -> tauri::Context<Wry> {
    tauri::generate_context!()
}

/// メインスレッドを占有する。Tauri のイベントループがランタイムのフレームループも回す。
/// どのランタイムでも同じ経路を通り、違いは `kernel.surface()` だけ。戻り値は終了コード。
pub fn run(kernel: Kernel, panels: Vec<WebPanelSpec>) -> i32 {
    run_with(kernel, panels, |_| {})
}

/// [`run`] に、パネルを開いた直後に呼ぶ `ready` を足したもの（テストがイベントを購読し、操作を始めるのに使う）。
pub fn run_with<B: VrBackend + 'static>(
    kernel: Kernel<B>,
    panels: Vec<WebPanelSpec>,
    ready: impl FnOnce(&AppHandle) + 'static,
) -> i32 {
    let shared = Arc::new(Shared {
        control: Mutex::default(),
        pages: panels
            .iter()
            .map(|s| (s.page.name, s.page.clone()))
            .collect(),
    });
    let served = shared.clone();
    let app = tauri::Builder::<Wry>::default()
        .manage(shared.clone())
        .register_uri_scheme_protocol("nyaw", move |ctx, req| {
            let res = tauri::http::Response::builder();
            let resolver = ctx.app_handle().asset_resolver();
            let asset = asset_path(&served.pages, req.uri().path()).and_then(|path| {
                // resolver.get は見つからないパスにも HTML を返すので、実在を先に確認する。
                resolver
                    .iter()
                    .any(|(key, _)| key == path)
                    .then(|| resolver.get(path.into()))
                    .flatten()
            });
            match asset {
                Some(asset) => res
                    .header(tauri::http::header::CONTENT_TYPE, asset.mime_type)
                    .body(Cow::Owned(asset.bytes)),
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
        .build(context());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            logger::error!("Tauri を起動できない", error = e);
            return 1;
        }
    };
    let mut frames = Frames::new(kernel, shared);
    let mut start = Some((panels, ready));
    let pending = Arc::new(AtomicBool::new(false));
    app.run_return(move |app, event| match event {
        RunEvent::Ready => {
            if let Some((panels, ready)) = start.take() {
                for spec in panels {
                    frames.open(app, spec);
                }
                tick(app.clone(), frames.wait(), pending.clone());
                ready(app);
            }
        }
        RunEvent::MainEventsCleared => frames.idle(app),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Resized(size),
            ..
        } => frames.resized(app, &label, size),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } => frames.destroyed(&label),
        _ => {}
    })
}

/// `period` ごとにイベントループを起こす（[`RunEvent::MainEventsCleared`] が来て [`Frames::idle`] が回る）。
/// フレームループはカーネルを持つのでメインスレッドから出せない。起こすだけを別スレッドで行う。
fn tick(app: AppHandle, period: Duration, pending: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);
            // 前の起こしが処理されるまで積まない（VR は poll が vsync まで待つので溜まりやすい）。
            if pending.swap(true, Ordering::AcqRel) {
                continue;
            }
            let p = pending.clone();
            if app
                .run_on_main_thread(move || p.store(false, Ordering::Release))
                .is_err()
            {
                break; // 終了した
            }
        }
    });
}

/// `Surface::Overlay` のウィンドウをデスクトップに出さない。
/// WebKitGTK は未マップ（非表示）のウィンドウを描かず撮れないので、表示したまま画面の外に置く。
// ponytail: Wayland ではアプリがウィンドウの位置を決められず画面に出てしまう。X11（XWayland 含む）のみ。
// GDK_BACKEND=x11 で起動するか、オフスクリーン描画（WPE 等）に移すのが次の一手。
#[cfg(target_os = "linux")]
fn offscreen<'a>(
    b: WebviewWindowBuilder<'a, Wry, AppHandle>,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.visible(true).position(-20000.0, -20000.0)
}

/// `Surface::Overlay` のウィンドウをデスクトップに出さない（非表示のまま描かせて撮る）。
#[cfg(not(target_os = "linux"))]
fn offscreen<'a>(
    b: WebviewWindowBuilder<'a, Wry, AppHandle>,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.visible(false)
}

/// パネルごとの描画（読み戻し）の状態。
#[derive(Default)]
struct Draw {
    capturing: bool,
    captured: Option<Instant>,
    hot_until: Option<Instant>,
    last: Option<Image>,
}

struct Frames<B: VrBackend> {
    kernel: Kernel<B>,
    surface: Surface,
    shared: Arc<Shared>,
    /// 添字は PanelId。閉じたパネルは None。
    windows: Vec<Option<WebviewWindow<Wry>>>,
    draws: Vec<Draw>,
    /// (パネル, 手) ごとに最後に送った Move の位置。
    moved: HashMap<(PanelId, Hand), Vec2>,
    /// 添字は PanelId。
    outboxes: Vec<Outbox>,
    tx: Sender<(PanelId, Result<Image, String>)>,
    rx: Receiver<(PanelId, Result<Image, String>)>,
    next: Instant,
    warned: bool,
}

impl<B: VrBackend> Frames<B> {
    fn new(kernel: Kernel<B>, shared: Arc<Shared>) -> Self {
        let (tx, rx) = channel();
        Self {
            surface: kernel.surface(),
            kernel,
            shared,
            windows: Vec::new(),
            draws: Vec::new(),
            moved: HashMap::new(),
            outboxes: Vec::new(),
            tx,
            rx,
            next: Instant::now(),
            warned: false,
        }
    }

    /// 起こす間隔。VR は `poll` が vsync まで待つので、すぐ戻ってくる程度でよい（UI 層と同じく 1ms 譲る）。
    fn wait(&self) -> Duration {
        match self.surface {
            Surface::Desktop => FRAME,
            Surface::Overlay => Duration::from_millis(1),
        }
    }

    fn open(&mut self, app: &AppHandle, spec: WebPanelSpec) -> PanelId {
        let index = self.windows.len();
        let pose = spec.pose_for(&self.kernel, index);
        let panel = self
            .kernel
            .add_panel(
                &format!("nyaw.web.{index}"),
                pose,
                spec.size,
                spec.resolution,
            )
            .expect("create overlay");
        debug_assert_eq!(panel, index, "PanelId は開いた順の添字");
        if let Some((device, offset)) = spec.attach {
            self.kernel
                .apply(panel, PanelCommand::Attach(device, offset));
        }
        if spec.locked {
            self.kernel.apply(panel, PanelCommand::Lock(true));
        }
        if let Some((hand, part)) = spec.pointer {
            self.kernel.apply(panel, PanelCommand::Pointer(hand, part));
        }
        let overlay = self.surface == Surface::Overlay;
        let (w, h) = spec.resolution;
        let builder = WebviewWindowBuilder::new(
            app,
            label(panel),
            WebviewUrl::CustomProtocol(page_url(spec.page.name)),
        )
        .title(spec.page.name)
        .inner_size(w as f64, h as f64)
        .initialization_script(BRIDGE)
        .transparent(spec.transparent);
        let window = if overlay {
            // 描画は VR に送るので、デスクトップには出さない。
            offscreen(builder)
                .focused(false)
                .skip_taskbar(true)
                .decorations(false)
                .shadow(false)
                .build()
        } else {
            builder.build()
        };
        let window = match window {
            Ok(w) => Some(w),
            Err(e) => {
                logger::error!("パネルのウィンドウを開けない", panel = panel, error = e);
                self.kernel.apply(panel, PanelCommand::Close);
                None
            }
        };
        logger::info!(
            "Web パネルを開いた",
            panel = panel,
            page = spec.page.name,
            pose = ?pose
        );
        self.windows.push(window);
        self.draws.push(Draw::default());
        self.outboxes.push(Outbox::default());
        panel
    }

    /// イベントループが空いたとき。読み戻しの結果を受け取り、周期が来ていれば 1 フレーム進める。
    fn idle(&mut self, app: &AppHandle) {
        self.receive();
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = now + self.wait();
        self.step(app, now);
    }

    /// 1 フレーム: カーネルを進めてポインタを配り、ページの操作を反映し、パネルを開き、描画を読み戻す。
    fn step(&mut self, app: &AppHandle, now: Instant) {
        let events = self.kernel.tick();
        if self.kernel.quit_requested() {
            app.exit(0);
            return;
        }
        for e in events {
            if !matches!(self.windows.get(e.panel), Some(Some(_))) {
                continue;
            }
            // オーバーレイは指している間 毎 tick Move を出す。同じ位置なら送らない（JS とイベントループを詰まらせない）。
            let at = (e.panel, e.hand);
            match e.event {
                PointerEvent::Move { pos } if self.moved.get(&at) == Some(&pos) => continue,
                PointerEvent::Move { pos } => {
                    self.moved.insert(at, pos);
                }
                PointerEvent::Leave => {
                    self.moved.remove(&at);
                }
                _ => {}
            }
            self.outboxes[e.panel].push(WebPointer::new(e.hand, e.event));
            self.draws[e.panel].hot_until = Some(now + HOT);
        }
        let acks = std::mem::take(&mut self.lock().acks);
        for panel in acks {
            self.outboxes[panel].ack();
        }
        for (panel, window) in self.windows.iter().enumerate() {
            let Some(window) = window else { continue };
            if let Some(batch) = self.outboxes[panel].take(now)
                && let Err(err) = app.emit_to(window.label(), POINTER_EVENT, batch)
            {
                logger::warn!("ポインタを送れない", panel = panel, error = err);
            }
        }
        for panel in 0..self.windows.len() {
            self.sync(app, panel, now);
        }
        let spawns = std::mem::take(&mut self.lock().spawns);
        for (from, name) in spawns {
            let page = self.shared.pages[name].clone();
            self.open(app, WebPanelSpec::spawn(from, page));
        }
        if self.surface == Surface::Overlay {
            self.capture(now);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// ページが積んだ操作を `panel` に反映し、状態が変わったらページへ知らせる。UI 層の `sync_panel` に当たる。
    fn sync(&mut self, app: &AppHandle, panel: PanelId, now: Instant) {
        if self.kernel.panel(panel).closed {
            return;
        }
        let commands = self.lock().commands.remove(&panel).unwrap_or_default();
        for c in commands {
            self.command(panel, c);
        }
        if self.kernel.panel(panel).closed {
            return;
        }
        let status = WebStatus::of(&self.kernel, panel);
        let changed = self.lock().statuses.insert(panel, status.clone()) != Some(status.clone());
        if changed {
            if let Err(e) = app.emit_to(label(panel), STATUS_EVENT, status) {
                logger::warn!("状態を送れない", panel = panel, error = e);
            }
            self.draws[panel].hot_until = Some(now + HOT);
        }
    }

    fn command(&mut self, panel: PanelId, c: WebCommand) {
        match c {
            WebCommand::Attach(d) => {
                let offset = if d == WebDevice::Hmd {
                    HMD_OFFSET
                } else {
                    HAND_OFFSET
                };
                self.kernel
                    .apply(panel, PanelCommand::Attach(d.device(), offset));
            }
            WebCommand::Detach => self.kernel.apply(panel, PanelCommand::Detach),
            WebCommand::Lock(l) => self.kernel.apply(panel, PanelCommand::Lock(l)),
            WebCommand::Close => {
                self.kernel.apply(panel, PanelCommand::Close);
                if let Some(w) = self.windows[panel].take()
                    && let Err(e) = w.destroy()
                {
                    logger::warn!("ウィンドウを閉じられない", panel = panel, error = e);
                }
            }
            WebCommand::Resize { width, height } => {
                let (width, height) = (width.max(1), height.max(1));
                if let Some(w) = &self.windows[panel]
                    && let Err(e) = w.set_size(LogicalSize::new(width, height))
                {
                    logger::warn!("ウィンドウの大きさを変えられない", panel = panel, error = e);
                }
                // 非表示のウィンドウは Resized が来ないことがあるので、ここでも合わせる。
                self.kernel
                    .apply(panel, PanelCommand::Resize(width, height));
            }
        }
    }

    /// ウィンドウの大きさが変わった（OS の操作・`resize` コマンド）。パネルの解像度を論理ピクセルで合わせる。
    fn resized(&mut self, app: &AppHandle, label: &str, size: PhysicalSize<u32>) {
        let Ok(panel) = panel_of(label) else { return };
        let Some(Some(window)) = self.windows.get(panel) else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return; // 最小化
        }
        let scale = window.scale_factor().unwrap_or(1.0);
        let logical = size.to_logical::<f64>(scale);
        let res = (logical.width.round() as u32, logical.height.round() as u32);
        if self.kernel.panel(panel).resolution != res {
            logger::debug!("パネルの大きさが変わった", panel = panel, resolution = ?res);
            self.kernel.apply(panel, PanelCommand::Resize(res.0, res.1));
            self.sync(app, panel, Instant::now());
        }
    }

    /// ウィンドウが閉じられた（OS の X など）。そのパネルも閉じる。
    fn destroyed(&mut self, label: &str) {
        let Ok(panel) = panel_of(label) else { return };
        if panel < self.windows.len() && !self.kernel.panel(panel).closed {
            self.kernel.apply(panel, PanelCommand::Close);
            self.windows[panel] = None;
        }
    }

    /// 撮り直すパネルの WebView を撮る（非同期。結果は [`Self::receive`] で受ける）。
    fn capture(&mut self, now: Instant) {
        for (panel, window) in self.windows.iter().enumerate() {
            let Some(window) = window else { continue };
            let d = &mut self.draws[panel];
            let due = d.hot_until.is_some_and(|t| now < t)
                || d.captured.is_none_or(|t| now - t >= IDLE_REDRAW);
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
    fn receive(&mut self) {
        while let Ok((panel, r)) = self.rx.try_recv() {
            self.draws[panel].capturing = false;
            if self.kernel.panel(panel).closed {
                continue;
            }
            if let Some(t) = self.draws[panel].captured {
                logger::trace!("読み戻した", panel = panel, ms = t.elapsed().as_millis());
            }
            match r {
                // 開いた直後は大きさの反映が間に合わないことがある。送らずに次で撮り直す。
                Ok(img) if img.width <= 1 || img.height <= 1 => {}
                Ok(img) => {
                    if self.draws[panel].last.as_ref() != Some(&img) {
                        self.kernel.submit(panel, img.width, img.height, &img.rgba);
                        self.draws[panel].last = Some(img);
                    }
                }
                Err(e) if !self.warned => {
                    logger::error!("Web パネルの描画を取得できない", panel = panel, error = e);
                    self.warned = true;
                }
                Err(_) => {}
            }
        }
    }
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
        );
        assert_eq!(
            serde_json::to_value(p).unwrap(),
            serde_json::json!({ "hand": "left", "type": "scroll", "x": 10.0, "y": 20.0, "dx": 0.0, "dy": -2.0 })
        );
        let leave = WebPointer::new(Hand::Right, PointerEvent::Leave);
        assert_eq!((leave.hand, leave.kind), ("right", "leave"));
    }

    fn ptr(hand: Hand, ev: PointerEvent) -> WebPointer {
        WebPointer::new(hand, ev)
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
    }

    #[test]
    fn status_reports_attach_lock_and_resolution() {
        let mut k = Kernel::with_backend(MockBackend::default(), Default::default());
        let p = k
            .add_panel("t", Pose::IDENTITY, Vec2::new(0.8, 0.6), PANEL_PX)
            .unwrap();
        k.apply(
            p,
            PanelCommand::Attach(Device::Controller(Hand::Left), HAND_OFFSET),
        );
        k.apply(p, PanelCommand::Lock(true));
        k.apply(p, PanelCommand::Resize(400, 300));
        let s = WebStatus::of(&k, p);
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
    fn registered_routes_use_the_shared_app() {
        let pages: HashMap<_, _> = [("demo", Page::new("demo"))].into_iter().collect();
        assert_eq!(asset_path(&pages, "/demo"), Some("/index.html"));
        assert_eq!(asset_path(&pages, "/demo/"), Some("/index.html"));
        assert_eq!(asset_path(&pages, "/assets/app.js"), Some("/assets/app.js"));
        assert_eq!(asset_path(&pages, "/demo/nope.js"), None);
        assert_eq!(asset_path(&pages, "/other"), None);
        assert_eq!(asset_path(&pages, "/"), None);
        assert_eq!(page_url("demo").path(), "/demo");
    }

    #[test]
    fn react_app_and_its_assets_are_embedded() {
        let context = context();
        let assets = context.assets();
        let html = assets
            .get(&"/index.html".into())
            .expect("npm run build で作った React の入口");
        assert!(String::from_utf8_lossy(&html).contains("id=\"root\""));
        assert!(
            assets
                .iter()
                .any(|(path, _)| path.starts_with("/assets/") && path.ends_with(".js"))
        );
        assert!(
            assets
                .iter()
                .any(|(path, _)| path.starts_with("/assets/") && path.ends_with(".css"))
        );
    }

    #[test]
    fn panel_labels_round_trip() {
        assert_eq!(panel_of(&label(7)), Ok(7));
        assert!(panel_of("main").is_err());
    }

    #[test]
    fn spawned_panel_is_placed_next_to_its_source() {
        let mut k = Kernel::with_backend(MockBackend::default(), Default::default());
        let pose = Pose::new(Vec3::new(0.0, 1.0, -1.0), Quat::IDENTITY);
        k.add_panel("t", pose, Vec2::ONE, PANEL_PX).unwrap();
        let spec = WebPanelSpec::spawn(0, Page::new("x"));
        assert_eq!(spec.pose_for(&k, 1).position, Vec3::new(1.2, 1.0, -1.0));
    }
}
