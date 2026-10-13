//! novr ランタイムの E2E。パネルはウィンドウで、入力は OS のマウスとしてネイティブに届く
//! （本番の novr と同じく、入力は VR・入力・オーバーレイ層を経由しない）。
//! ビューからのパネル操作（取り付け・ロック）は本番と同じく novr バックエンドのオーバーレイに反映する。
//! [`NoVrKit::with_backend`] で実ランタイムを渡すと、マウスで操作した画面の描画をそのランタイムのオーバーレイへ送る
//! （コントローラの無い実ランタイムで、画面を操作した結果が VR に届くかを確かめる用）。

use gpui::{
    Bounds, Context, Entity, Modifiers, MouseButton, Pixels, Point, Render, ScrollDelta,
    ScrollWheelEvent, TestAppContext, VisualTestContext, Window,
};
use kernel::{Kernel, PanelId};
use vr::{Runtime, VrBackend};

use crate::{Driver, Pose, Quat, Vec2, Vec3, raster};

/// パネルを置く位置（実ランタイムで目の前に見えるよう、立った目の高さの正面 1m）。
const PANEL_POSE: Pose = Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY);

pub struct NoVrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    view: Entity<V>,
    /// 描画の送り先。novr では何もしない。
    pub kernel: Kernel,
    pub panel: PanelId,
    /// マウスの位置と、左ボタンを押したままか。
    pos: Point<Pixels>,
    held: bool,
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
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let mut kernel = Kernel::with_backend(vr, Default::default());
        let panel = kernel
            .add_panel("nyaw.testkit", PANEL_POSE, Vec2::ONE, res)
            .expect("パネルを作れない");
        let mut kit = Self {
            cx,
            view,
            kernel,
            panel,
            pos: Point::default(),
            held: false,
        };
        kit.step();
        kit
    }

    /// 本番のフレームループ1回分: ビューからのパネル操作を反映し、落ち着くまで回す。
    pub fn step(&mut self) -> &mut Self {
        self.cx.run_until_parked();
        let (kernel, panel) = (&mut self.kernel, self.panel);
        self.cx.update(|_, cx| ui::sync_panel(kernel, panel, cx));
        self.cx.run_until_parked();
        let frame = self.cx.update(raster::capture);
        self.kernel
            .submit(self.panel, frame.width, frame.height, &frame.rgba);
        self
    }

    /// フォーカスのある要素にキーを打つ（`"h i backspace"` のように空白区切り）。VR にはキーボードが無いので novr だけ。
    pub fn type_keys(&mut self, keys: &str) -> &mut Self {
        self.cx.simulate_keystrokes(keys);
        self.step()
    }

    /// `debug_selector` を付けた要素のウィンドウ上の範囲。
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// マウスをウィンドウ上の点へ動かす。
    pub fn move_to(&mut self, pos: Point<Pixels>) -> &mut Self {
        self.pos = pos;
        let button = self.held.then_some(MouseButton::Left);
        self.cx.simulate_mouse_move(pos, button, Modifiers::none());
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
        self.cx.simulate_click(c, Modifiers::none());
        self.step()
    }

    /// 要素の中心で左ボタンを押したままにする。
    fn down(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.cx
            .simulate_mouse_down(c, MouseButton::Left, Modifiers::none());
        self.held = true;
        self.step()
    }

    fn up(&mut self) -> &mut Self {
        self.cx
            .simulate_mouse_up(self.pos, MouseButton::Left, Modifiers::none());
        self.held = false;
        self.step()
    }

    /// 要素の中心でホイールを回す。
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.cx.simulate_event(ScrollWheelEvent {
            position: c,
            delta: ScrollDelta::Lines(Point::new(0.0, lines)),
            ..Default::default()
        });
        self.step()
    }

    /// ビューの状態を読む。
    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.view.read_with(self.cx, |v, _| f(v))
    }

    /// 画面に描かれている要素の中心の色（0xRRGGBB）。
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        let (frame, scale) = self
            .cx
            .update(|window, cx| (raster::capture(window, cx), window.scale_factor()));
        let (x, y) = (
            (f32::from(c.x) * scale) as u32,
            (f32::from(c.y) * scale) as u32,
        );
        let i = ((y * frame.width + x) * 4) as usize;
        u32::from_be_bytes([0, frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]])
    }
}
