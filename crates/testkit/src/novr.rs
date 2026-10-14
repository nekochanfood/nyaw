//! novr ランタイムの E2E。本番の novr と同じくエミュレーター（[`Emulator`]）を通す: 全層（エミュレーター → 入力 →
//! オーバーレイ → UI → GPUI）を丸ごと持ち、パネルの描画はテクスチャとしてエミュレーターへ送られ、
//! 入力はエミュレーターのマウス（右手のコントローラ）・ボタン・スティックから来る。ユーザーに見える絵は
//! エミュレーターの画面（[`NoVrKit::screenshot`]）。ウィンドウは出さない（`NYAW_E2E_WATCH` で画面を見られる）。

use gpui::{Bounds, Context, Pixels, Render, TestAppContext, VisualTestContext, Window};
use kernel::{Kernel, PanelId};
use ui::Frame;
use vr::novr::{Button, Emulator};

use crate::panels::Panels;
use crate::watch::Watch;
use crate::{Driver, Hand, Pose, Quat, Vec2, Vec3};

/// 最初のパネルの位置。隣に召喚したパネル（右 1.2m）も画面に収まる距離。
const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.5);
/// エミュレーターの画面の大きさ（本番のウィンドウの初期値と同じ）。
const SCREEN: (u32, u32) = (1280, 720);

pub struct NoVrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    panels: Panels<V>,
    /// 直接覗いてよい（例: パネルの状態の確認）。エミュレーターは `kernel.vr`。
    pub kernel: Kernel<Emulator>,
    pub panel: PanelId,
    /// エミュレーターの画面を出す（`NYAW_E2E_WATCH`）。
    watch: Option<Watch>,
}

impl<'a, V: Render> NoVrKit<'a, V> {
    /// `build` のビューを、正面に置いた最初のパネルとして起動する。パネル解像度 = ウィンドウサイズ。
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        // 手ブレ抑えは指した瞬間に当たらなくなるので切る（VrKit と同じ）。
        let config = kernel::config::Config {
            pointer_smoothing: Some(0.0),
            ..Default::default()
        };
        let mut kernel = Kernel::with_backend(Emulator::headless(SCREEN.0, SCREEN.1), config);
        let mut panels = Panels::new(None);
        let pose = Pose::new(PANEL_POS, Quat::IDENTITY);
        let panel = panels.add(cx, &mut kernel, pose, Some(view));
        let mut kit = Self {
            cx,
            panels,
            kernel,
            panel,
            watch: Watch::from_env(),
        };
        kit.step(); // 最初のフレームを送っておく
        kit
    }

    /// 1フレーム進める（エミュレーターの入力 → `ui::pump` → GPUI → 描画をエミュレーターへ）。
    pub fn step(&mut self) -> &mut Self {
        self.panels.step(self.cx, &mut self.kernel);
        if self.watch.is_some() {
            let shot = self.screenshot();
            let w = self.watch.as_mut().unwrap();
            w.show(0, &shot);
            w.wait();
        }
        self
    }

    /// 以降の操作・読み取りに使うパネルを選ぶ。
    pub fn select_panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.panels.windows.len(), "パネル {panel} がない");
        self.panel = panel;
        self
    }

    fn visual(&self) -> VisualTestContext {
        VisualTestContext::from_window(self.panels.windows[self.panel], &self.cx.cx)
    }

    /// `debug_selector` を付けた要素のパネル上の範囲（論理ピクセル）。
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.visual()
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// 要素の中心が映っている画面上の点。
    pub fn on_screen(&mut self, selector: &'static str) -> Vec2 {
        let c = self.bounds(selector).center();
        let world = self
            .kernel
            .panel(self.panel)
            .pixel_to_world(Vec2::new(c.x.into(), c.y.into()));
        self.kernel
            .vr
            .project(world)
            .unwrap_or_else(|| panic!("要素 {selector:?} が目の後ろにある"))
    }

    /// エミュレーターの画面（ユーザーに見えている絵）。
    pub fn screenshot(&mut self) -> Frame {
        let (width, height, rgba) = self.kernel.vr.screenshot();
        Frame {
            width,
            height,
            rgba,
        }
    }

    /// 画面上の点の色（0xRRGGBB）。
    pub fn color_on_screen(&mut self, at: Vec2) -> u32 {
        let shot = self.screenshot();
        let i = ((at.y as u32 * shot.width + at.x as u32) * 4) as usize;
        u32::from_be_bytes([0, shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]])
    }

    /// マウスを画面上の点へ動かして（`None` で画面の外へ出して）1フレーム進める。
    pub fn move_mouse(&mut self, at: Option<Vec2>) -> &mut Self {
        self.kernel.vr.point(at);
        self.step()
    }

    /// マウスのボタン（左 = トリガー、右 = グリップ）を押して1フレーム進める。
    pub fn press(&mut self, button: Button) -> &mut Self {
        self.kernel.vr.set_button(button, true);
        self.step()
    }

    pub fn release(&mut self, button: Button) -> &mut Self {
        self.kernel.vr.set_button(button, false);
        self.step()
    }

    /// スティック（左手 = WASD、右手 = 矢印キー）を倒して1フレーム進める。戻すときは `Vec2::ZERO`。
    pub fn stick(&mut self, hand: Hand, value: Vec2) -> &mut Self {
        self.kernel.vr.set_stick(hand, value);
        self.step()
    }

    /// フォーカスのある要素にキーを打つ（`"h i backspace"` のように空白区切り）。エミュレーターのキーボードは
    /// スティックなのでパネルには届かない。UI のロジックを見るために GPUI のウィンドウへ直接打つ。
    pub fn type_keys(&mut self, keys: &str) -> &mut Self {
        self.visual().simulate_keystrokes(keys);
        self.step()
    }
}

impl<V: Render> Driver<V> for NoVrKit<'_, V> {
    fn web_spawns(&self) -> &[String] {
        &self.panels.web_spawns
    }

    /// マウスを要素の中心へ動かす。
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let at = self.on_screen(selector);
        self.move_mouse(Some(at))
    }

    /// 要素の中心を左クリック（トリガー）する。
    fn click(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector)
            .press(Button::Trigger)
            .release(Button::Trigger)
    }

    /// 要素の中心で左ボタン（トリガー）を押したままにする。
    fn down(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector).press(Button::Trigger)
    }

    fn up(&mut self) -> &mut Self {
        self.release(Button::Trigger)
    }

    /// 要素の中心を指し、矢印キー（右手のスティック）を1フレームだけ縦に倒す。
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        self.hover(selector)
            .stick(Hand::Right, Vec2::new(0.0, lines))
            .stick(Hand::Right, Vec2::ZERO)
    }

    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.panels.views[self.panel]
            .as_ref()
            .expect("画面型が異なる")
            .read_with(self.cx, |v, _| f(v))
    }

    /// エミュレーターの画面に映っている色。
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let at = self.on_screen(selector);
        self.color_on_screen(at)
    }
}
