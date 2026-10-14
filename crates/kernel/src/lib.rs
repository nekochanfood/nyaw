//! カーネル: 一点集中型の中枢。ブートローダー（`main`）が起こし、UI や画面はこれだけを相手にする。
//!
//! 入れ子の層: 設定（[`config`]）を内に持ち、下位の層（ランタイム `vr`・入力 `input`・オーバーレイ `overlay`）の状態を
//! 1 か所で持つ。フレームの進行（[`Kernel::tick`]）・パネルへの操作（[`Kernel::apply`]）・描画結果の受け取り
//! （[`Kernel::submit`]）をシステムコールのように提供する。上位層が下位層を直接組み立てたり、状態を別々に持ったりしない。
//! 上位層が語彙として使う `vr` / `overlay` の型は再エクスポートする。

pub mod config;

use std::time::{Duration, Instant};

pub use config::{Config, PointerMode};
pub use overlay::{self, Overlay, Panel, PanelId, Pointer, PointerEvent, UiEvent};
pub use vr::{self, Device, Hand, Pose, Runtime, Vec2, VrBackend, VrError};

/// [`Kernel::tick`] 1 回の内訳。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TickTiming {
    /// ランタイムの poll（vsync 待ち・合成を含む）。
    pub poll: Duration,
    /// オーバーレイ層（追従・ヒット判定・ポインタの配置・アニメーション）。
    pub overlay: Duration,
    /// poll のうち、ランタイムがこのスレッドで合成・表示にかけた時間（novr のエミュレーターだけ）。
    pub compositor: Duration,
}

/// 上位層（ビュー）からパネルへの操作要求。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelCommand {
    Attach(Device, Pose),
    Detach,
    Lock(bool),
    /// このパネルのイン・アウトアニメーションを有効化・無効化する。
    Animated(bool),
    Close,
    /// その手のレーザーを描くパネルにする（ヒット判定の対象外になり、置き場所はオーバーレイが決める）。
    Pointer(Hand, Pointer),
    /// 解像度（UI の論理ピクセル）を変える。1m あたりのピクセル数を保って VR 上の大きさも変わる。
    Resize(u32, u32),
    /// レーザーのエミュレート方式を変える。パネルを問わずすべてのパネルに効く。
    PointerMode(PointerMode),
}

/// パネルの状態。ビューの表示用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelStatus {
    pub runtime: Option<Runtime>,
    pub attached: Option<Device>,
    pub locked: bool,
    pub animated: bool,
    /// 見えているトラッカーの ID（取り付け先の候補）。
    pub trackers: Vec<String>,
    /// メニューボタンを押している。
    pub menu: bool,
    /// パネルを掴んでいる。
    pub grabbed: bool,
    /// 閉じられた。
    pub closed: bool,
    /// レーザーのエミュレート方式（全パネル共通）。
    pub pointer_mode: PointerMode,
}

/// 中枢。本番は `Kernel`（= `Kernel<Box<dyn VrBackend>>`）、テストは具象のバックエンド（モック等）を持てる。
pub struct Kernel<B: VrBackend = Box<dyn VrBackend>> {
    pub config: Config,
    /// テストがモックを直接いじるために公開している。本番コードはメソッド経由で使う。
    pub vr: B,
    overlay: Overlay,
    timing: TickTiming,
    pointer_mode: PointerMode,
}

impl Kernel {
    /// ランタイムを選んで起動する。`requested` は `"--novr"` / `"novr"` のような指定（コマンドライン > 設定ファイル。決めるのは呼び出し側）。
    /// 指定なしで VR ランタイムが無ければ novr で起動する（novr は常に使える）。指定したのに繋がらなければエラー。
    pub fn boot(requested: Option<&str>, config: Config) -> Result<Self, VrError> {
        let prefer = match requested {
            Some("--novr" | "novr") => Some(Runtime::NoVr),
            Some("--openxr" | "openxr") => Some(Runtime::OpenXr),
            Some("--openvr" | "openvr") => Some(Runtime::OpenVr),
            _ => None,
        };
        let vr = match vr::connect(prefer) {
            Ok(b) => b,
            Err(e) if prefer.is_none() => {
                logger::warn!("VR ランタイムに接続できないので novr で起動する", error = e);
                vr::connect(Some(Runtime::NoVr)).expect("novr は常に接続できる")
            }
            Err(e) => {
                logger::error!("VR ランタイムに接続できない", runtime = ?prefer, error = e);
                return Err(e);
            }
        };
        logger::info!("ランタイムを選択", runtime = ?vr.runtime());
        Ok(Self::with_backend(vr, config))
    }
}

impl<B: VrBackend> Kernel<B> {
    /// 接続済みのバックエンドで起動する（テスト用。本番は [`Kernel::boot`]）。
    pub fn with_backend(vr: B, config: Config) -> Self {
        // フレームの周期を作る短い待ちが丸められないように。
        os::precise_timers();
        let mut overlay = Overlay::new();
        if let Some(s) = config.pointer_smoothing {
            overlay.set_pointer_smoothing(s);
        }
        Self {
            pointer_mode: config.pointer_mode.unwrap_or_default(),
            config,
            vr,
            overlay,
            timing: TickTiming::default(),
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.vr.runtime()
    }

    /// レーザーのエミュレート方式。UI がポインタイベントをマウス・タッチのどちらとして流すかを決める。
    pub fn pointer_mode(&self) -> PointerMode {
        self.pointer_mode
    }

    pub fn add_panel(
        &mut self,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        self.overlay
            .add_panel(&mut self.vr, key, pose, size, resolution)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        self.overlay.panel(id)
    }

    /// 1 フレーム進める。ランタイムを poll し、パネルの追従・掴み移動とレーザーのヒット判定を行って UI イベントを返す。
    pub fn tick(&mut self) -> Vec<UiEvent> {
        let t = Instant::now();
        let frame = self.vr.poll();
        let polled = Instant::now();
        let events = self.overlay.tick_frame(&mut self.vr, frame);
        self.timing = TickTiming {
            poll: polled - t,
            overlay: polled.elapsed(),
            compositor: self.vr.compositor_time(),
        };
        events
    }

    /// 最後の [`tick`](Self::tick) の内訳（プロファイラ用）。
    pub fn timing(&self) -> TickTiming {
        self.timing
    }

    /// 最後の tick の HMD の姿勢。
    pub fn hmd(&self) -> Pose {
        self.overlay.hmd()
    }

    /// `hand` でドラッグしている間、その見た目を幅 `width_m` で出す（`Overlay::set_drag`）。`None` で隠す。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.overlay.set_drag(hand, width_m)
    }

    /// ランタイムが終了を求めている（SteamVR の終了など）。呼び出し側はループを抜ける。
    pub fn quit_requested(&self) -> bool {
        self.overlay.quit_requested()
    }

    /// パネルの描画結果（RGBA8）をランタイムへ送る。
    pub fn submit(&mut self, panel: PanelId, width: u32, height: u32, rgba: &[u8]) {
        self.overlay
            .submit(&mut self.vr, panel, width, height, rgba)
    }

    pub fn apply(&mut self, panel: PanelId, cmd: PanelCommand) {
        match cmd {
            PanelCommand::Attach(device, offset) => self.overlay.attach(panel, device, offset),
            PanelCommand::Detach => self.overlay.detach(panel),
            PanelCommand::Lock(locked) => self.overlay.set_locked(panel, locked),
            PanelCommand::Animated(animated) => {
                self.overlay.set_animated(&mut self.vr, panel, animated)
            }
            PanelCommand::Close => self.overlay.close(&mut self.vr, panel),
            PanelCommand::Pointer(hand, part) => {
                self.overlay.set_pointer(&mut self.vr, panel, hand, part)
            }
            PanelCommand::Resize(w, h) => self.overlay.set_resolution(&mut self.vr, panel, (w, h)),
            PanelCommand::PointerMode(m) => self.pointer_mode = m,
        }
    }

    pub fn status(&self, panel: PanelId) -> PanelStatus {
        let p = self.overlay.panel(panel);
        PanelStatus {
            runtime: Some(self.vr.runtime()),
            attached: p.attached.as_ref().map(|(d, _)| d.clone()),
            locked: p.locked,
            animated: p.animated,
            trackers: self.overlay.trackers().to_vec(),
            menu: self.overlay.menu_held(),
            grabbed: self.overlay.grabbed(panel),
            closed: p.closed,
            pointer_mode: self.pointer_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::mock::MockBackend;

    fn kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k.add_panel("t", Pose::IDENTITY, Vec2::ONE, (4, 4)).unwrap();
        (k, p)
    }

    #[test]
    fn boot_novr_by_name() {
        let k = Kernel::boot(Some("--novr"), Config::default()).unwrap();
        assert_eq!(k.runtime(), Runtime::NoVr);
    }

    #[test]
    fn boot_without_request_always_succeeds() {
        // 実ランタイムの有無に依らず、指定なしなら novr にフォールバックして起動できる
        assert!(Kernel::boot(None, Config::default()).is_ok());
    }

    #[test]
    fn apply_lock_shows_in_status() {
        let (mut k, p) = kernel();
        assert!(!k.status(p).locked);
        k.apply(p, PanelCommand::Lock(true));
        assert!(k.status(p).locked);
    }

    #[test]
    fn apply_resize_changes_resolution_and_vr_width() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Resize(8, 2));
        assert_eq!(k.panel(p).resolution, (8, 2));
        assert_eq!(k.panel(p).size, Vec2::new(2.0, 0.5));
        assert_eq!(k.vr.overlays[0].width_m, 2.0);
    }

    #[test]
    fn pointer_mode_starts_from_config_and_switches() {
        let config = Config {
            pointer_mode: Some(PointerMode::Touch),
            ..Default::default()
        };
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        let p = k.add_panel("t", Pose::IDENTITY, Vec2::ONE, (4, 4)).unwrap();
        assert_eq!(k.status(p).pointer_mode, PointerMode::Touch);
        k.apply(p, PanelCommand::PointerMode(PointerMode::Mouse));
        assert_eq!(k.pointer_mode(), PointerMode::Mouse);
        assert_eq!(
            kernel().0.pointer_mode(),
            PointerMode::Mouse,
            "既定はマウス"
        );
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Animated(false));
        k.submit(p, 1, 1, &[1, 2, 3, 4]);
        let tex = k.vr.overlays[0].texture.as_ref().unwrap();
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }

    #[test]
    fn animation_command_opts_out_only_the_selected_panel() {
        let (mut k, p) = kernel();
        let other = k
            .add_panel("other", Pose::IDENTITY, Vec2::ONE, (4, 4))
            .unwrap();
        k.apply(p, PanelCommand::Animated(false));
        assert!(!k.status(p).animated && k.status(other).animated);
        k.submit(p, 1, 1, &[255; 4]);
        k.submit(other, 1, 1, &[255; 4]);
        assert_eq!(k.vr.overlays[p].width_m, 1.0);
        assert_eq!(k.vr.overlays[other].width_m, 0.75);
        k.apply(p, PanelCommand::Close);
        assert!(k.status(p).closed && !k.vr.overlays[p].visible);
        k.apply(other, PanelCommand::Animated(false));
        assert_eq!(
            k.vr.overlays[other].texture.as_ref().unwrap().pixel(0, 0),
            [255; 4]
        );
    }
}
