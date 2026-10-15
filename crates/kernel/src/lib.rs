//! カーネル: 一点集中型の中枢。ブートローダー（`main`）が起こし、UI や画面はこれだけを相手にする。
//!
//! 最下部の [`subsystems`] が OS・ランタイム・入力・描画の差を吸収し、その上で設定とパネル管理を行う。
//! フレームの進行（[`Kernel::tick`]）・パネルへの操作（[`Kernel::apply`]）・描画結果の受け取り
//! （[`Kernel::submit`]）を提供する。UI や画面は下位層を直接組み立てない。

pub mod config;
pub use subsystems::{self, Subsystems};
pub use subsystems::{rendering, vr};

use std::time::{Duration, Instant};
use subsystems::os;

pub use config::{Config, LodQuality, PointerMode};
pub use overlay::{self, Overlay, Panel, PanelId, Pointer, PointerEvent, UiEvent};
pub use vr::{Device, Hand, Pose, Runtime, Vec2, VrBackend, VrError};

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
    /// 今の頭から見た相対姿勢（`ui::PanelSpec::in_view` と同じく頭の傾きは無視）へ置き直す。取り付けは外す。
    Place(Pose),
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
    /// 描画解像度の段階（[`Kernel::panel_lod`]）。
    pub lod: u8,
}

/// 中枢。本番は `Kernel`（= `Kernel<Box<dyn VrBackend>>`）、テストは具象のバックエンド（モック等）を持てる。
pub struct Kernel<B: VrBackend = Box<dyn VrBackend>> {
    pub config: Config,
    /// テストがモックを直接いじるために公開している。本番コードはメソッド経由で使う。
    pub subsystems: Subsystems<B>,
    overlay: Overlay,
    timing: TickTiming,
    pointer_mode: PointerMode,
    /// 進めたフレームの数（[`Self::panel_frame_due`]）。
    frame: u64,
}

impl Kernel {
    /// ランタイムを選んで起動する。`requested` は `"--novr"` / `"novr"` のような指定（コマンドライン > 設定ファイル。決めるのは呼び出し側）。
    /// 指定なしで VR ランタイムが無ければ novr で起動する（`novr` フィーチャーがあれば常に使える）。指定したのに繋がらなければエラー。
    pub fn boot(requested: Option<&str>, config: Config) -> Result<Self, VrError> {
        let prefer = match requested {
            Some("--novr" | "novr") => Some(Runtime::NoVr),
            Some("--openxr" | "openxr") => Some(Runtime::OpenXr),
            Some("--openvr" | "openvr") => Some(Runtime::OpenVr),
            _ => None,
        };
        let vr = match vr::connect(prefer, config.gpu.emulator) {
            Ok(b) => b,
            Err(e) if prefer.is_none() => {
                logger::warn!("failed to connect to VR runtime, starting novr", error = e);
                // `novr` フィーチャーなしのビルドにはエミュレーターが無く、ここで失敗しうる。
                vr::connect(Some(Runtime::NoVr), config.gpu.emulator)?
            }
            Err(e) => {
                logger::error!("failed to connect to VR runtime", runtime = ?prefer, error = e);
                return Err(e);
            }
        };
        logger::info!("runtime selected", runtime = ?vr.runtime());
        Ok(Self::with_backend(vr, config))
    }
}

impl<B: VrBackend> Kernel<B> {
    /// 接続済みのバックエンドで起動する（テスト用。本番は [`Kernel::boot`]）。
    pub fn with_backend(vr: B, config: Config) -> Self {
        // フレームの周期を作る短い待ちが丸められないように。
        os::precise_timers();
        // ゲームが GPU を使い切っていても、パネルの描画と読み戻しが後回しにされないように。
        if !os::high_gpu_priority() {
            logger::warn!("cannot raise GPU scheduling priority");
        }
        let subsystems = Subsystems::with_backend(vr);
        let mut overlay = Overlay::new();
        if let Some(s) = config.pointer_smoothing {
            overlay.set_pointer_smoothing(s);
        }
        let pointer_mode = config.pointer_mode.unwrap_or_default();
        // タッチではスライドがパネルからはみ出しても続くよう、押している間はポインタを掴んでおく。
        overlay.set_capture(pointer_mode == PointerMode::Touch);
        Self {
            pointer_mode,
            config,
            subsystems,
            overlay,
            timing: TickTiming::default(),
            frame: 0,
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.subsystems.backend.runtime()
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
            .add_panel(&mut self.subsystems.backend, key, pose, size, resolution)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        self.overlay.panel(id)
    }

    /// 距離による描画解像度の段階（LOD0〜3）。Mid は 2 / 4 / 8 m で切り替える。
    pub fn panel_lod(&self, id: PanelId) -> u8 {
        let near = match self.config.lod_quality {
            LodQuality::Off => return 0,
            LodQuality::Low => 1.0,
            LodQuality::Mid => 2.0,
            LodQuality::High => 4.0,
        };
        let distance = self.panel(id).pose.position.distance(self.hmd().position);
        [near, near * 2.0, near * 4.0]
            .into_iter()
            .filter(|&threshold| distance >= threshold)
            .count() as u8
    }

    /// このフレームでパネルを描き直してよいか。LOD0 は毎フレーム、LOD n は 2^n フレームに 1 回。
    /// パネルごとにずらし、間引いたパネルの描き直しが同じフレームに重ならないようにする。
    pub fn panel_frame_due(&self, id: PanelId) -> bool {
        (self.frame + id as u64).is_multiple_of(1 << self.panel_lod(id))
    }

    /// 1 フレーム進める。ランタイムを poll し、パネルの追従・掴み移動とレーザーのヒット判定を行って UI イベントを返す。
    pub fn tick(&mut self) -> Vec<UiEvent> {
        let t = Instant::now();
        self.frame += 1;
        let frame = self.subsystems.poll();
        let polled = Instant::now();
        let events = self.overlay.tick_frame(&mut self.subsystems.backend, frame);
        self.timing = TickTiming {
            poll: polled - t,
            overlay: polled.elapsed(),
            compositor: self.subsystems.backend.compositor_time(),
        };
        events
    }

    /// 最後の [`tick`](Self::tick) の内訳（プロファイラ用）。
    pub fn timing(&self) -> TickTiming {
        self.timing
    }

    /// フレームの周期が決まっていれば（novr）その長さ。プロファイラがドロップを数える基準に使う。
    pub fn frame_interval(&self) -> Option<Duration> {
        self.subsystems.backend.frame_interval()
    }

    /// `hand` が押している間のポインタのキャプチャを外す（タッチの長押しでドラッグになったとき。ほかのパネルへ運べるように）。
    pub fn release_capture(&mut self, hand: Hand) {
        self.overlay.release_capture(hand);
    }

    /// パネルが見えているか（ほかのパネルに隠れていない・視野の中）。ランタイムが直前の合成から教えてくれればそれ（novr）、
    /// 無ければ頭の向きと視野で見積もる。見えないパネルの描き直しを省く（カリング）のに使う。
    pub fn panel_seen(&self, panel: PanelId) -> bool {
        let p = self.overlay.panel(panel);
        !p.closed
            && self
                .subsystems
                .backend
                .overlay_seen(p.overlay_id())
                .unwrap_or_else(|| self.overlay.in_view(panel))
    }

    /// ランタイムが同時に出せるオーバーレイの数の上限（[`VrBackend::max_overlays`]）。
    pub fn max_overlays(&self) -> Option<usize> {
        self.subsystems.backend.max_overlays()
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
            .submit(&mut self.subsystems.backend, panel, width, height, rgba)
    }

    pub fn apply(&mut self, panel: PanelId, cmd: PanelCommand) {
        match cmd {
            PanelCommand::Attach(device, offset) => self.overlay.attach(panel, device, offset),
            PanelCommand::Detach => self.overlay.detach(panel),
            PanelCommand::Lock(locked) => self.overlay.set_locked(panel, locked),
            PanelCommand::Animated(animated) => {
                self.overlay
                    .set_animated(&mut self.subsystems.backend, panel, animated)
            }
            PanelCommand::Close => self.overlay.close(&mut self.subsystems.backend, panel),
            PanelCommand::Pointer(hand, part) => {
                self.overlay
                    .set_pointer(&mut self.subsystems.backend, panel, hand, part)
            }
            PanelCommand::Resize(w, h) => {
                self.overlay
                    .set_resolution(&mut self.subsystems.backend, panel, (w, h))
            }
            PanelCommand::PointerMode(m) => {
                self.pointer_mode = m;
                self.overlay.set_capture(m == PointerMode::Touch);
            }
            PanelCommand::Place(offset) => {
                self.overlay.detach(panel);
                self.overlay.set_pose(panel, self.hmd().leveled() * offset);
            }
        }
    }

    pub fn status(&self, panel: PanelId) -> PanelStatus {
        let p = self.overlay.panel(panel);
        PanelStatus {
            runtime: Some(self.subsystems.backend.runtime()),
            attached: p.attached.as_ref().map(|(d, _)| d.clone()),
            locked: p.locked,
            animated: p.animated,
            trackers: self.overlay.trackers().to_vec(),
            menu: self.overlay.menu_held(),
            grabbed: self.overlay.grabbed(panel),
            closed: p.closed,
            pointer_mode: self.pointer_mode,
            lod: self.panel_lod(panel),
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

    #[cfg(feature = "novr")]
    #[test]
    fn boot_novr_by_name() {
        for name in ["novr", "--novr"] {
            for gpu in [false, true] {
                let mut config = Config::default();
                config.gpu.emulator = gpu;
                let k = Kernel::boot(Some(name), config).unwrap();
                assert_eq!(k.runtime(), Runtime::NoVr);
            }
        }
    }

    #[cfg(feature = "novr")]
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
    fn place_detaches_and_moves_relative_to_head() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Attach(Device::Hmd, Pose::IDENTITY));
        let to = Pose::new(vr::Vec3::new(1.0, 0.0, -2.0), vr::Quat::IDENTITY);
        k.apply(p, PanelCommand::Place(to));
        assert_eq!(k.status(p).attached, None);
        assert_eq!(k.panel(p).pose, k.hmd().leveled() * to);
    }

    #[test]
    fn distance_lod_tracks_camera_and_preserves_panel_geometry() {
        let (mut k, p) = kernel();
        let pose = k.panel(p).pose;
        let resolution = k.panel(p).resolution;
        let size = k.panel(p).size;
        k.config.lod_quality = LodQuality::Off;
        k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
        for (quality, near) in [
            (LodQuality::Low, 1.0),
            (LodQuality::Mid, 2.0),
            (LodQuality::High, 4.0),
        ] {
            k.config.lod_quality = quality;
            for (distance, lod) in [
                (0.0, 0),
                (near - 0.01, 0),
                (near, 1),
                (near * 2.0, 2),
                (near * 4.0, 3),
                (100.0, 3),
                (0.5, 0),
            ] {
                k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * distance;
                k.tick();
                assert_eq!(k.panel_lod(p), lod, "{quality:?}: {distance}");
                let due = (0..8)
                    .filter(|_| {
                        k.tick();
                        k.panel_frame_due(p)
                    })
                    .count();
                assert_eq!(due, 8 >> lod, "{quality:?}: {distance}");
                assert_eq!(k.panel(p).resolution, resolution);
                assert_eq!(k.panel(p).size, size);
            }
        }
    }

    #[test]
    fn apply_resize_changes_resolution_and_vr_width() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Resize(8, 2));
        assert_eq!(k.panel(p).resolution, (8, 2));
        assert_eq!(k.panel(p).size, Vec2::new(2.0, 0.5));
        assert_eq!(k.subsystems.backend.overlays[0].width_m, 2.0);
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
            PointerMode::Touch,
            "defaults to touch"
        );
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Animated(false));
        k.submit(p, 1, 1, &[1, 2, 3, 4]);
        let tex = k.subsystems.backend.overlays[0].texture.as_ref().unwrap();
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
        assert_eq!(k.subsystems.backend.overlays[p].width_m, 1.0);
        assert_eq!(k.subsystems.backend.overlays[other].width_m, 0.75);
        k.apply(p, PanelCommand::Close);
        assert!(k.status(p).closed && !k.subsystems.backend.overlays[p].visible);
        k.apply(other, PanelCommand::Animated(false));
        assert_eq!(
            k.subsystems.backend.overlays[other]
                .texture
                .as_ref()
                .unwrap()
                .pixel(0, 0),
            [255; 4]
        );
    }
}
