//! Layer 4: GPUI フロントエンド。各オーバーレイパネルは GPUI ウィンドウで実体化され、
//! オーバーレイ層のレーザーイベントは通常の GPUI マウス入力に変換される。
//! そのためビューはデスクトップの GPUI ビューと全く同じように書け、VR 上にあることを意識しない。
//! 逆向きは [`capture`] でウィンドウをオフスクリーン描画して読み戻し、`Overlay::submit` で VR パネルへ送る。

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{
    AnyView, AnyWindowHandle, App, Bounds, Context, Entity, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollWheelEvent, Window, WindowBounds, WindowHandle, WindowOptions, prelude::*, px, size,
};
use kernel::vr::{Hand, Quat, Vec3};
use kernel::{
    Device, Kernel, PanelCommand, PanelId, PanelStatus, Pointer, PointerEvent, Pose, Surface, Vec2,
    VrBackend,
};

/// 1つの手のレーザーイベントを GPUI のプラットフォーム入力に変換する。GPUI のイベントは
/// 位置とボタン状態を持つがオーバーレイのイベントは持たないので、直前の値を保持する。
#[derive(Debug, Default)]
pub struct PointerBridge {
    pos: Point<Pixels>,
    pressed: bool,
}

impl PointerBridge {
    pub fn translate(&mut self, ev: PointerEvent) -> PlatformInput {
        let at = |p: Vec2| Point::new(px(p.x), px(p.y));
        let held = |pressed: bool| pressed.then_some(MouseButton::Left);
        match ev {
            PointerEvent::Move { pos } => {
                self.pos = at(pos);
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: self.pos,
                    pressed_button: held(self.pressed),
                    modifiers: Default::default(),
                })
            }
            PointerEvent::Down { pos } => {
                (self.pos, self.pressed) = (at(pos), true);
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            }
            PointerEvent::Up { pos } => {
                (self.pos, self.pressed) = (at(pos), false);
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                })
            }
            PointerEvent::Scroll { pos, delta } => {
                self.pos = at(pos);
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: self.pos,
                    delta: ScrollDelta::Lines(Point::new(delta.x, delta.y)),
                    modifiers: Default::default(),
                    touch_phase: Default::default(),
                })
            }
            PointerEvent::Leave => {
                let pressed_button = held(std::mem::take(&mut self.pressed));
                PlatformInput::MouseExited(MouseExitEvent {
                    position: self.pos,
                    pressed_button,
                    modifiers: Default::default(),
                })
            }
        }
    }
}

/// 1パネル分の操作要求と状態。
#[derive(Debug, Default)]
pub struct PanelState {
    pub commands: Vec<PanelCommand>,
    pub status: PanelStatus,
}

/// ビューとフレームループの受け渡し口（GPUI のグローバル）。ビューは対象パネルの `commands` に積み、
/// フレームループが [`sync_panel`] でオーバーレイに反映して `status` を書き戻す。
/// ビューの型に依存しないので、本番・testkit のどのキットも同じ経路を通る。
#[derive(Default)]
pub struct PanelControl {
    pub panels: HashMap<PanelId, PanelState>,
    pub spawns: Vec<PanelSpec>,
}

impl gpui::Global for PanelControl {}

/// ビューが積んだ操作要求を `panel` に反映し、状態を書き戻す。変わったらウィンドウを描き直す（`true` を返す）。
/// 入力を配送した後・描画する前に毎フレーム呼ぶ。
pub fn sync_panel<B: VrBackend>(kernel: &mut Kernel<B>, panel: PanelId, cx: &mut App) -> bool {
    let ctl = cx
        .default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default();
    for c in ctl.commands.drain(..) {
        kernel.apply(panel, c);
    }
    let status = kernel.status(panel);
    if ctl.status == status {
        return false;
    }
    ctl.status = status;
    cx.refresh_windows();
    true
}

/// 1フレーム分: 下位パイプラインを回し、パネル宛てのイベントを GPUI 入力に変換して返す。
/// 本番の [`run`] とテスト用の testkit が共通で通る経路。
pub fn pump<B: VrBackend>(
    kernel: &mut Kernel<B>,
    bridges: &mut HashMap<(PanelId, Hand), PointerBridge>,
) -> Vec<(PanelId, PlatformInput)> {
    kernel
        .tick()
        .into_iter()
        .map(|e| {
            let input = bridges
                .entry((e.panel, e.hand))
                .or_default()
                .translate(e.event);
            (e.panel, input)
        })
        .collect()
}

/// CPU に読み戻した UI の1フレーム（RGBA8、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ウィンドウを描き直し、画面に出さずにオフスクリーンへ描画して読み戻す。
/// 大きさはデバイスピクセル（論理サイズ × 拡大率）。
// ponytail: gpui の `render_to_image` は test-support フィーチャー限定かつ Windows のみ実装。
// 上流で一般公開されたらフィーチャーを外す。Linux は gpui_wgpu のヘッドレスレンダラ経由が次の一手。
pub fn capture(window: &mut Window, cx: &mut App) -> gpui::Result<Frame> {
    window.draw(cx).clear(cx);
    let img = window.render_to_image()?;
    Ok(Frame {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// パネル用ウィンドウの設定。ランタイムの [`Surface`] によって見せ方だけが変わる。
/// - `Overlay`: 描画は VR に送るので、デスクトップには出さない（非表示）。
/// - `Desktop`: OS のウィンドウとして画面中央に表示し、OS の入力をそのまま受ける。
pub fn panel_window_options(surface: Surface, resolution: (u32, u32), cx: &App) -> WindowOptions {
    let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
    match surface {
        Surface::Overlay => WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size,
            })),
            show: false,
            focus: false,
            ..Default::default()
        },
        Surface::Desktop => WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
            ..Default::default()
        },
    }
}

/// パネル用ウィンドウを開く。`Overlay` では Windows の非表示ウィンドウのレンダーターゲットが
/// 1x1 のまま作られるので、明示的にリサイズして実寸にする（反映は非同期。最初の数フレームは 1x1）。
pub fn open_panel_window<V: Render>(
    cx: &mut App,
    surface: Surface,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    let options = panel_window_options(surface, resolution, cx);
    let window = cx.open_window(options, build)?;
    if surface == Surface::Overlay {
        let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
        cx.update_window(window.into(), |_, window, _| window.resize(size))?;
    }
    Ok(window)
}

const PANEL_PX: (u32, u32) = (800, 600);
/// Desktop（novr）だけが使う周期。VR ランタイムは `poll` が vsync まで待つ。
const FRAME: Duration = Duration::from_millis(11);
/// 入力も状態の変化も無いパネルを描き直す間隔。時計のようにビューが自分で描き直すものはこれで反映される。
// ponytail: GPUI はウィンドウの dirty を公開していないので推測で描く。公開されたら dirty なときだけにする。
const IDLE_REDRAW: Duration = Duration::from_millis(250);

type PanelBuilder = dyn FnOnce(PanelId, &mut Window, &mut App) -> AnyView;

/// 画面と VR 空間での配置。異なる画面型を同じ起動リストに入れられる。
pub struct PanelSpec {
    pose: Pose,
    from: Option<PanelId>,
    size: Vec2,
    resolution: (u32, u32),
    /// 起動時に取り付ける先と相対姿勢。
    attach: Option<(Device, Pose)>,
    /// 起動時から掴めない。
    locked: bool,
    /// レーザーを描くパネル。
    pointer: Option<(Hand, Pointer)>,
    build: Box<PanelBuilder>,
}

impl PanelSpec {
    pub fn new<V: Render>(
        pose: Pose,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            pose,
            from: None,
            size: Vec2::new(0.8, 0.6),
            resolution: PANEL_PX,
            attach: None,
            locked: false,
            pointer: None,
            build: Box::new(move |panel, window, cx| cx.new(|cx| build(panel, window, cx)).into()),
        }
    }

    pub fn spawn<V: Render>(
        from: PanelId,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            from: Some(from),
            ..Self::new(Pose::IDENTITY, build)
        }
    }

    /// 大きさ（m）と解像度（px）を指定する。既定は 0.8m × 0.6m / 800 × 600。
    pub fn sized(mut self, size: Vec2, resolution: (u32, u32)) -> Self {
        (self.size, self.resolution) = (size, resolution);
        self
    }

    /// 起動時にデバイスへ取り付ける。
    pub fn attached(mut self, device: Device, offset: Pose) -> Self {
        self.attach = Some((device, offset));
        self
    }

    /// 起動時から掴めなくする。
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// `hand` のレーザー（線またはカーソル）を描くパネルにする。置き場所はオーバーレイ層が決める。
    pub fn pointer(mut self, hand: Hand, part: Pointer) -> Self {
        self.pointer = Some((hand, part));
        self
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
            None => self.pose,
        }
    }

    pub fn build(self, panel: PanelId, window: &mut Window, cx: &mut App) -> AnyView {
        (self.build)(panel, window, cx)
    }
}

pub struct PanelRoot(AnyView);

impl PanelRoot {
    pub fn new(view: AnyView) -> Self {
        Self(view)
    }

    pub fn view(&self) -> AnyView {
        self.0.clone()
    }
}

impl Render for PanelRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// メインスレッドを占有する。GPUI のイベントループがランタイムのフレームループも回す。
/// どのランタイム（OpenVR / OpenXR / novr）でも同じ経路を通り、違いは `kernel.surface()` だけ。
/// パネルに載せる画面（上の層）は `panels` で受け取る。UI 層はどの画面かを知らない。
pub fn run(mut kernel: Kernel, panels: Vec<PanelSpec>) {
    let surface = kernel.surface();
    gpui_platform::application().run(move |cx: &mut App| {
        let mut windows: Vec<(PanelId, AnyWindowHandle)> = panels
            .into_iter()
            .enumerate()
            .map(|(i, spec)| open_panel(&mut kernel, spec, i, cx))
            .collect();
        if surface == Surface::Desktop {
            cx.activate(true);
        }

        cx.spawn(async move |cx| {
            let mut bridges = HashMap::new();
            let mut warned = false;
            let mut last = vec![None; windows.len()];
            // パネルごとの最後に描いた時刻（None = 次のフレームで必ず描く）。
            let mut drawn: Vec<Option<Instant>> = vec![None; windows.len()];
            // VR では poll がブロックして周期を作る。待ちの前に一度 GPUI へ制御を返す。
            let wait = if surface == Surface::Desktop {
                FRAME
            } else {
                Duration::ZERO
            };
            loop {
                cx.background_executor().timer(wait).await;
                let mut inputs: Vec<Vec<PlatformInput>> =
                    (0..windows.len()).map(|_| Vec::new()).collect();
                for (panel, input) in pump(&mut kernel, &mut bridges) {
                    inputs[panel].push(input);
                }
                if kernel.quit_requested() {
                    cx.update(|cx| cx.quit());
                    return;
                }
                // WindowHandle::update はルートビューを借用したままになり、draw やクリック処理で
                // ビューを更新すると二重借用で落ちる。AnyWindowHandle 経由で借用しない。
                for ((panel, window), inputs) in windows.iter().copied().zip(inputs) {
                    if kernel.panel(panel).closed {
                        continue;
                    }
                    // 描画と読み戻し（GPU の完了待ちを含む）は重いので、入力・状態の変化があったとき、
                    // 前回描いてから IDLE_REDRAW 経ったときだけ行う。
                    let mut redraw = !inputs.is_empty()
                        || drawn[panel].is_none_or(|t| t.elapsed() >= IDLE_REDRAW);
                    let mut changed = false;
                    let Ok(frame) = cx.update_window(window, |_, window, cx| {
                        for input in inputs {
                            window.dispatch_event(input, cx);
                        }
                        changed = sync_panel(&mut kernel, panel, cx);
                        redraw |= changed;
                        if kernel.panel(panel).closed {
                            window.remove_window();
                            return None;
                        }
                        // Desktop では OS が表示するので読み戻さない。
                        (surface == Surface::Overlay && redraw).then(|| capture(window, cx))
                    }) else {
                        // ウィンドウが OS の X などで閉じられた。そのパネルだけ閉じ、全部閉じたら終了する。
                        kernel.apply(panel, PanelCommand::Close);
                        if (0..windows.len()).all(|p| kernel.panel(p).closed) {
                            cx.update(|cx| cx.quit());
                            return;
                        }
                        continue;
                    };
                    if changed {
                        // sync_panel は全ウィンドウを描き直させる（状態はどのパネルにも表示されうる）。
                        drawn.iter_mut().for_each(|t| *t = None);
                    }
                    match frame {
                        // 開いた直後はウィンドウの大きさの反映が間に合わず 1x1 で描かれる。送らずに次のフレームで描き直す。
                        Some(Ok(f)) if f.width <= 1 || f.height <= 1 => {}
                        // 画素が変わったときだけ送る（GPU 書き込みと完了待ちを省く）。
                        Some(Ok(f)) => {
                            drawn[panel] = Some(Instant::now());
                            if last[panel].as_ref() != Some(&f) {
                                kernel.submit(panel, f.width, f.height, &f.rgba);
                                last[panel] = Some(f);
                            }
                        }
                        Some(Err(e)) if !warned => {
                            logger::error!("パネルの描画を取得できない", error = e, panel = panel);
                            warned = true;
                        }
                        _ => {}
                    }
                }
                for spec in
                    cx.update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns))
                {
                    let i = windows.len();
                    windows.push(cx.update(|cx| open_panel(&mut kernel, spec, i, cx)));
                    last.push(None);
                    drawn.push(None);
                }
            }
        })
        .detach();
    });
}

fn open_panel(
    kernel: &mut Kernel,
    mut spec: PanelSpec,
    index: usize,
    cx: &mut App,
) -> (PanelId, AnyWindowHandle) {
    let pose = spec.pose_for(kernel, index);
    let panel = kernel
        .add_panel(
            &format!("nyaw.panel.{index}"),
            pose,
            spec.size,
            spec.resolution,
        )
        .expect("create overlay");
    if let Some((device, offset)) = spec.attach.take() {
        kernel.apply(panel, PanelCommand::Attach(device, offset));
    }
    if spec.locked {
        kernel.apply(panel, PanelCommand::Lock(true));
    }
    if let Some((hand, part)) = spec.pointer {
        kernel.apply(panel, PanelCommand::Pointer(hand, part));
    }
    let window = open_panel_window(cx, kernel.surface(), spec.resolution, |w, cx| {
        let view = spec.build(panel, w, cx);
        cx.new(|_| PanelRoot::new(view))
    })
    .expect("open panel window");
    (panel, window.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, div};
    use kernel::{Device, Runtime};

    /// UI 層だけで確かめるための最小の画面。
    #[derive(Default)]
    struct Probe {
        clicks: usize,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("probe")
                    .debug_selector(|| "probe".into())
                    .size_10()
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
        }
    }

    #[gpui::test]
    fn laser_click_reaches_gpui_view(cx: &mut TestAppContext) {
        let (testing, cx) = cx.add_window_view(|_, _| Probe::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("probe").expect("button rendered").center();
        let pos = Vec2::new(c.x.into(), c.y.into());

        let mut bridge = PointerBridge::default();
        for ev in [
            PointerEvent::Move { pos },
            PointerEvent::Down { pos },
            PointerEvent::Up { pos },
            PointerEvent::Leave,
        ] {
            let input = bridge.translate(ev);
            cx.update(|window, cx| {
                window.dispatch_event(input, cx);
            });
        }
        cx.run_until_parked();
        assert_eq!(testing.read_with(cx, |h, _| h.clicks), 1);
    }

    #[gpui::test]
    fn panel_window_shows_only_on_desktop_surface(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(!panel_window_options(Surface::Overlay, PANEL_PX, cx).show);
            assert!(panel_window_options(Surface::Desktop, PANEL_PX, cx).show);
        });
    }

    #[gpui::test]
    fn desktop_window_matches_panel_resolution(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            open_panel_window(cx, Surface::Desktop, PANEL_PX, |_, cx| {
                cx.new(|_| Probe::default())
            })
            .unwrap()
        });
        let vp = cx
            .update(|cx| cx.update_window(window.into(), |_, w, _| w.viewport_size()))
            .unwrap();
        assert_eq!(vp, size(px(PANEL_PX.0 as f32), px(PANEL_PX.1 as f32)));
    }

    #[gpui::test]
    fn spec_attaches_locks_and_sizes_panel(cx: &mut TestAppContext) {
        let mut kernel = Kernel::boot(Some("novr"), Default::default()).unwrap();
        let hand = Device::Controller(Hand::Left);
        let spec = PanelSpec::new(Pose::IDENTITY, |_, _, _| Probe::default())
            .sized(Vec2::new(0.1, 0.05), (10, 5))
            .attached(hand.clone(), Pose::IDENTITY)
            .locked();
        let (panel, _) = cx.update(|cx| open_panel(&mut kernel, spec, 0, cx));
        let p = kernel.panel(panel);
        assert_eq!(p.attached, Some((hand, Pose::IDENTITY)));
        assert!(p.locked);
        assert_eq!((p.size, p.resolution), (Vec2::new(0.1, 0.05), (10, 5)));
    }

    #[gpui::test]
    fn sync_panel_applies_commands_and_reports_status(cx: &mut TestAppContext) {
        let mut kernel = Kernel::boot(Some("novr"), Default::default()).unwrap();
        let panel = kernel
            .add_panel("t", Pose::IDENTITY, Vec2::ONE, PANEL_PX)
            .unwrap();
        cx.update(|cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .extend([
                    PanelCommand::Attach(Device::Hmd, Pose::IDENTITY),
                    PanelCommand::Lock(true),
                ]);
            sync_panel(&mut kernel, panel, cx);
            let ctl = &cx.global::<PanelControl>().panels[&panel];
            assert!(ctl.commands.is_empty());
            assert_eq!(
                ctl.status,
                PanelStatus {
                    runtime: Some(Runtime::NoVr),
                    attached: Some(Device::Hmd),
                    locked: true,
                    ..Default::default()
                }
            );
            cx.default_global::<PanelControl>()
                .panels
                .get_mut(&panel)
                .unwrap()
                .commands
                .push(PanelCommand::Detach);
            sync_panel(&mut kernel, panel, cx);
        });
        assert_eq!(kernel.panel(panel).attached, None);
    }

    #[test]
    fn leave_releases_held_button() {
        let mut b = PointerBridge::default();
        b.translate(PointerEvent::Down {
            pos: Vec2::new(3.0, 4.0),
        });
        let PlatformInput::MouseExited(e) = b.translate(PointerEvent::Leave) else {
            panic!()
        };
        assert_eq!(
            (e.position, e.pressed_button),
            (Point::new(px(3.0), px(4.0)), Some(MouseButton::Left))
        );
        assert!(!b.pressed);
    }
}
