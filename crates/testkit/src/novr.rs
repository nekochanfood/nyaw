//! novr ランタイムの E2E。パネルはウィンドウで、入力は OS のマウスとしてネイティブに届く
//! （本番の novr と同じく、VR・入力・オーバーレイ層を経由しない）。

use gpui::{
    Bounds, Context, Entity, Modifiers, Pixels, Point, Render, TestAppContext, VisualTestContext,
    Window,
};

use crate::{Driver, raster};

pub struct NoVrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    view: Entity<V>,
}

impl<'a, V: Render> NoVrKit<'a, V> {
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        Self { cx, view }
    }

    /// `debug_selector` を付けた要素のウィンドウ上の範囲。
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// マウスをウィンドウ上の点へ動かす。
    pub fn move_to(&mut self, pos: Point<Pixels>) -> &mut Self {
        self.cx.simulate_mouse_move(pos, None, Modifiers::none());
        self
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
        self
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
