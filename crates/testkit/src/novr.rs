//! novr ランタイムの E2E。パネルはウィンドウで、入力は OS のマウスとしてネイティブに届く
//! （本番の novr と同じく、入力は VR・入力・オーバーレイ層を経由しない）。
//! ビューからのパネル操作（取り付け・ロック）は本番と同じく novr バックエンドのオーバーレイに反映する。
//! [`NoVrKit::with_backend`] で実ランタイムを渡すと、マウスで操作した画面の描画をそのランタイムのオーバーレイへ送る
//! （コントローラの無い実ランタイムで、画面を操作した結果が VR に届くかを確かめる用）。

use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Modifiers, MouseButton, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, Window,
};
use kernel::{Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec};
use vr::{Runtime, VrBackend};

use crate::{Driver, Pose, Quat, Vec2, Vec3, raster};

/// パネルを置く位置（実ランタイムで目の前に見えるよう、立った目の高さの正面 1m）。
const PANEL_POSE: Pose = Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY);

pub struct NoVrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    views: Vec<Option<Entity<V>>>,
    windows: Vec<AnyWindowHandle>,
    /// 描画の送り先。novr では何もしない。
    pub kernel: Kernel,
    pub panel: PanelId,
    /// マウスの位置と、左ボタンを押したままか。
    mouse: Vec<(Point<Pixels>, bool)>,
}

impl<'a, V: Render> NoVrKit<'a, V> {
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let vr = vr::connect_exact(Runtime::NoVr).expect("novr は常に接続できる");
        Self::with_backend(cx, vr, build)
    }

    /// 描画を `vr` のオーバーレイ（正面 1m）へ毎フレーム送る。入力はマウスのまま。
    pub fn with_backend(
        cx: &'a mut TestAppContext,
        vr: Box<dyn VrBackend>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let mut kernel = Kernel::with_backend(vr, Default::default());
        let panel = kernel
            .add_panel("nyaw.testkit", PANEL_POSE, Vec2::ONE, res)
            .expect("パネルを作れない");
        let mut kit = Self {
            cx,
            views: vec![Some(view)],
            windows: vec![window],
            kernel,
            panel,
            mouse: vec![(Point::default(), false)],
        };
        kit.step();
        kit
    }

    /// 本番のフレームループ1回分: ビューからのパネル操作を反映し、落ち着くまで回す。
    pub fn step(&mut self) -> &mut Self {
        self.cx.run_until_parked();
        for (panel, window) in self.windows.iter().copied().enumerate() {
            self.cx
                .cx
                .update_window(window, |_, _, cx| {
                    ui::sync_panel(&mut self.kernel, panel, cx)
                })
                .unwrap();
        }
        let spawns = self
            .cx
            .cx
            .update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
        for spec in spawns {
            self.add_spec(spec);
        }
        self.cx.run_until_parked();
        for (panel, window) in self.windows.iter().copied().enumerate() {
            let frame = self
                .cx
                .cx
                .update_window(window, |_, window, cx| raster::capture(window, cx))
                .unwrap();
            self.kernel
                .submit(panel, frame.width, frame.height, &frame.rgba);
        }
        self
    }

    fn add_spec(&mut self, spec: PanelSpec) {
        let id = self.windows.len();
        let pose = spec.pose_for(&self.kernel, id);
        let (root, cx) = self
            .cx
            .cx
            .add_window_view(|window, cx| PanelRoot::new(spec.build(id, window, cx)));
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let view = root.read_with(cx, |root, _| root.view().downcast::<V>().ok());
        let panel = self
            .kernel
            .add_panel(&format!("nyaw.testkit.{id}"), pose, Vec2::ONE, res)
            .unwrap();
        assert_eq!(panel, id);
        self.views.push(view);
        self.windows.push(window);
        self.mouse.push((Point::default(), false));
        cx.update(|_, cx| ui::sync_panel(&mut self.kernel, panel, cx));
    }

    /// 以降の操作・読み取りに使うパネルを選ぶ。
    pub fn select_panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.windows.len(), "パネル {panel} がない");
        self.panel = panel;
        self
    }

    fn visual(&self) -> VisualTestContext {
        VisualTestContext::from_window(self.windows[self.panel], &self.cx.cx)
    }

    /// フォーカスのある要素にキーを打つ（`"h i backspace"` のように空白区切り）。VR にはキーボードが無いので novr だけ。
    pub fn type_keys(&mut self, keys: &str) -> &mut Self {
        self.visual().simulate_keystrokes(keys);
        self.step()
    }

    /// `debug_selector` を付けた要素のウィンドウ上の範囲。
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.visual()
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// マウスをウィンドウ上の点へ動かす。
    pub fn move_to(&mut self, pos: Point<Pixels>) -> &mut Self {
        self.mouse[self.panel].0 = pos;
        let button = self.mouse[self.panel].1.then_some(MouseButton::Left);
        self.visual()
            .simulate_mouse_move(pos, button, Modifiers::none());
        self.step()
    }
}

impl<V: Render> Driver<V> for NoVrKit<'_, V> {
    /// マウスを要素の中心へ動かす。
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c)
    }

    /// 要素の中心を左クリックする。
    fn click(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual().simulate_click(c, Modifiers::none());
        self.step()
    }

    /// 要素の中心で左ボタンを押したままにする。
    fn down(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual()
            .simulate_mouse_down(c, MouseButton::Left, Modifiers::none());
        self.mouse[self.panel].1 = true;
        self.step()
    }

    fn up(&mut self) -> &mut Self {
        self.visual().simulate_mouse_up(
            self.mouse[self.panel].0,
            MouseButton::Left,
            Modifiers::none(),
        );
        self.mouse[self.panel].1 = false;
        self.step()
    }

    /// 要素の中心でホイールを回す。
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual().simulate_event(ScrollWheelEvent {
            position: c,
            delta: ScrollDelta::Lines(Point::new(0.0, lines)),
            ..Default::default()
        });
        self.step()
    }

    /// ビューの状態を読む。
    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.views[self.panel]
            .as_ref()
            .expect("画面型が異なる")
            .read_with(self.cx, |v, _| f(v))
    }

    /// 画面に描かれている要素の中心の色（0xRRGGBB）。
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        let (frame, scale) = self
            .visual()
            .update(|window, cx| (raster::capture(window, cx), window.scale_factor()));
        let (x, y) = (
            (f32::from(c.x) * scale) as u32,
            (f32::from(c.y) * scale) as u32,
        );
        let i = ((y * frame.width + x) * 4) as usize;
        u32::from_be_bytes([0, frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]])
    }
}
