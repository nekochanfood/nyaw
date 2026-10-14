//! VR モードの E2E。全層（モック VR → 入力 → オーバーレイ → UI → GPUI）を丸ごと持ち、
//! レーザー・コントローラ機種・手・パネル操作・VR に映る描画など、VR 固有の機能も検証できる。
//! 毎フレーム UI の描画結果も VR モックに送られる（描画は [`crate::raster`] の CPU ラスタライザ）。

use std::collections::HashMap;

use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Pixels, PlatformInput, Render,
    TestAppContext, VisualTestContext, Window,
};
use kernel::{Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec, PointerBridge};
use vr::{
    RawController,
    mock::{MockBackend, MockTexture},
};

use crate::watch::Watch;
use crate::{Action, Driver, Hand, Pose, Quat, Vec2, Vec3, raster};

/// 指定がないときの仮想コントローラの機種。
pub const DEFAULT_PROFILE: &str = "valve/index_controller";
const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
const PANEL_WIDTH_M: f32 = 1.0;
/// 掴み移動がなめらかに追いつくまでの tick 数（余裕を持たせて）。
const DRAG_SETTLE: usize = 60;

/// 仮想コントローラ。押しているアクションを、機種に応じた生入力に変換して VR モックに流す。
struct VirtualController {
    hand: Hand,
    profile: String,
    aim: Pose,
    held: Vec<Action>,
    /// スティック（トラックパッド）の倒し具合。
    stick: Vec2,
}

/// 手を下ろした位置（頭の少し下・左右）。レーザーはここから対象へ向ける。
fn rest_position(hand: Hand) -> Vec3 {
    match hand {
        Hand::Left => Vec3::new(-0.2, -0.3, 0.0),
        Hand::Right => Vec3::new(0.2, -0.3, 0.0),
    }
}

/// VR ランタイム（モック）越しに操作する。
pub struct VrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    views: Vec<Option<Entity<V>>>,
    windows: Vec<AnyWindowHandle>,
    /// 直接覗いてよい（例: VR 側に送られたパネル姿勢の確認）。
    pub kernel: Kernel<MockBackend>,
    panel: PanelId,
    watch: Option<Watch>,
    /// 画面が開くよう頼んだ Web のページ（[`ui::PanelSpec::web`]）。テストプラットフォームでは開かない。
    pub web_spawns: Vec<String>,
    bridges: HashMap<(PanelId, Hand), PointerBridge>,
    controllers: Vec<VirtualController>,
    active: Hand,
}

impl<'a, V: Render> VrKit<'a, V> {
    /// `build` のビューを、正面 1m に置いた最初のパネルとして起動する。パネル解像度 = ウィンドウサイズ。
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let size = Vec2::new(PANEL_WIDTH_M, PANEL_WIDTH_M * res.1 as f32 / res.0 as f32);

        // 手ブレ抑えは指した瞬間に当たらなくなるので切る（抑え自体は overlay の単体テストで確かめる）。
        let config = kernel::config::Config {
            pointer_smoothing: Some(0.0),
            ..Default::default()
        };
        let mut kernel = Kernel::with_backend(MockBackend::default(), config);
        let pose = Pose::new(PANEL_POS, Quat::IDENTITY);
        let panel = kernel
            .add_panel("testkit", pose, size, res)
            .expect("モックはパネル作成に失敗しない");
        let mut kit = Self {
            cx,
            views: vec![Some(view)],
            windows: vec![window],
            kernel,
            panel,
            watch: Watch::from_env(),
            web_spawns: Vec::new(),
            bridges: HashMap::new(),
            controllers: Vec::new(),
            active: Hand::Right,
        };
        kit.step(); // 最初のフレームを VR に送っておく
        kit
    }

    /// 同じ画面型のパネルを追加する。戻り値を [`Self::panel`] に渡して操作先を選ぶ。
    pub fn add_panel(
        &mut self,
        pose: Pose,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> PanelId {
        let (view, cx) = self.cx.cx.add_window_view(build);
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let size = Vec2::new(PANEL_WIDTH_M, PANEL_WIDTH_M * res.1 as f32 / res.0 as f32);
        let id = self
            .kernel
            .add_panel(&format!("testkit.{}", self.windows.len()), pose, size, res)
            .expect("モックはパネル作成に失敗しない");
        self.views.push(Some(view));
        self.windows.push(window);
        self.step();
        id
    }

    /// 以降の操作・読み取りに使うパネルを選ぶ。
    pub fn panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.windows.len(), "パネル {panel} がない");
        self.panel = panel;
        self
    }

    /// 以降の操作に使う手を切り替える（既定は右手）。
    pub fn hand(&mut self, hand: Hand) -> &mut Self {
        self.active = hand;
        self
    }

    /// 現在の手のコントローラ機種を切り替える。
    pub fn profile(&mut self, id: &str) -> &mut Self {
        self.ctrl().profile = id.to_owned();
        self
    }

    /// `debug_selector` を付けた要素のウィンドウ上の範囲。
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        VisualTestContext::from_window(self.windows[self.panel], &self.cx.cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// レーザーをパネル上のピクセルに向けて1フレーム進める。
    pub fn point_at(&mut self, px: Vec2) -> &mut Self {
        let target = self.kernel.panel(self.panel).pixel_to_world(px);
        let c = self.ctrl();
        let dir = (target - c.aim.position).normalize();
        c.aim.orientation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
        self.step()
    }

    /// 現在の手のコントローラを `aim` に動かして1フレーム進める。
    pub fn move_hand(&mut self, aim: Pose) -> &mut Self {
        self.ctrl().aim = aim;
        self.step()
    }

    pub fn press(&mut self, action: Action) -> &mut Self {
        let c = self.ctrl();
        if !c.held.contains(&action) {
            c.held.push(action);
        }
        self.step()
    }

    pub fn release(&mut self, action: Action) -> &mut Self {
        self.ctrl().held.retain(|&a| a != action);
        self.step()
    }

    /// パネル中央を掴み、パネル中心が `to` に来るまで手を平行移動して離す。
    pub fn drag_panel(&mut self, to: Vec3) -> &mut Self {
        let p = self.kernel.panel(self.panel);
        let center = Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32) / 2.0;
        let delta = to - p.pose.position;
        self.point_at(center).press(Action::Grab);
        self.ctrl().aim.position += delta;
        // 掴み移動はなめらかに追いつくので、追いつくまで持ち続ける。
        for _ in 0..DRAG_SETTLE {
            self.step();
        }
        self.release(Action::Grab)
    }

    pub fn panel_pose(&self) -> Pose {
        self.kernel.panel(self.panel).pose
    }

    /// 仮想コントローラの状態を VR モックに流し、本番と同じ `ui::pump` で1フレーム進めて GPUI に配送する。
    pub fn step(&mut self) -> &mut Self {
        self.kernel.vr.frame.controllers = self.controllers.iter().map(raw).collect();
        let mut inputs: Vec<Vec<PlatformInput>> =
            (0..self.windows.len()).map(|_| Vec::new()).collect();
        for (panel, input) in ui::pump(&mut self.kernel, &mut self.bridges) {
            inputs[panel].push(input);
        }
        for (panel, (window, inputs)) in self.windows.iter().copied().zip(inputs).enumerate() {
            self.cx
                .cx
                .update_window(window, |_, window, cx| {
                    for input in inputs {
                        window.dispatch_event(input, cx);
                    }
                    ui::sync_panel(&mut self.kernel, panel, cx);
                })
                .unwrap();
        }
        self.cx.run_until_parked();
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
            if let Some(w) = &mut self.watch {
                w.show(panel, &frame);
            }
            self.kernel
                .submit(panel, frame.width, frame.height, &frame.rgba);
        }
        if let Some(w) = &self.watch {
            w.wait();
        }
        self
    }

    fn add_spec(&mut self, spec: PanelSpec) {
        // Web のページ（Tauri の WebView）はテストプラットフォームでは開けないので、頼まれたことだけ記録する。
        // 実際に開いて操作するのは crates/ui/tests/web_*.rs。
        if let Some(page) = spec.web_page() {
            self.web_spawns.push(page.to_owned());
            return;
        }
        let id = self.windows.len();
        let pose = spec.pose_for(&self.kernel, id);
        let (root, cx) = self
            .cx
            .cx
            .add_window_view(|window, cx| PanelRoot::new(spec.build(id, window, cx)));
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let size = Vec2::new(PANEL_WIDTH_M, PANEL_WIDTH_M * res.1 as f32 / res.0 as f32);
        let view = root.read_with(cx, |root, _| root.view().downcast::<V>().ok());
        let panel = self
            .kernel
            .add_panel(&format!("testkit.{id}"), pose, size, res)
            .unwrap();
        assert_eq!(panel, id);
        self.views.push(view);
        self.windows.push(window);
        cx.update(|_, cx| ui::sync_panel(&mut self.kernel, panel, cx));
    }

    /// VR パネルに最後に送られたテクスチャ。
    pub fn texture(&self) -> &MockTexture {
        self.kernel.vr.overlays[self.kernel.panel(self.panel).overlay_id().0 as usize]
            .texture
            .as_ref()
            .expect("VrKit::new で最初のフレームを送っている")
    }

    /// VR パネルに映っている、パネル上の論理ピクセル `px` の色（0xRRGGBB）。
    pub fn vr_color_at_px(&self, px: Vec2) -> u32 {
        let tex = self.texture();
        let (rw, rh) = self.kernel.panel(self.panel).resolution;
        let x = ((px.x / rw as f32 * tex.width as f32) as u32).min(tex.width - 1);
        let y = ((px.y / rh as f32 * tex.height as f32) as u32).min(tex.height - 1);
        let [r, g, b, _] = tex.pixel(x, y);
        u32::from_be_bytes([0, r, g, b])
    }

    fn ctrl(&mut self) -> &mut VirtualController {
        let hand = self.active;
        let i = match self.controllers.iter().position(|c| c.hand == hand) {
            Some(i) => i,
            None => {
                self.controllers.push(VirtualController {
                    hand,
                    profile: DEFAULT_PROFILE.to_owned(),
                    aim: Pose::new(rest_position(hand), Quat::IDENTITY),
                    held: Vec::new(),
                    stick: Vec2::ZERO,
                });
                self.controllers.len() - 1
            }
        };
        &mut self.controllers[i]
    }
}

fn raw(c: &VirtualController) -> RawController {
    let mut inputs = c
        .held
        .iter()
        .map(|&a| {
            let name = input::source(&c.profile, a)
                .unwrap_or_else(|| panic!("{} には {a:?} に当たる入力がない", c.profile));
            (name.to_owned(), 1.0)
        })
        .collect::<Vec<_>>();
    if c.stick != Vec2::ZERO {
        let (x, y) = input::scroll_source(&c.profile)
            .unwrap_or_else(|| panic!("{} にはスクロールに当たる入力がない", c.profile));
        inputs.extend([(x.to_owned(), c.stick.x), (y.to_owned(), c.stick.y)]);
    }
    RawController {
        hand: c.hand,
        profile: c.profile.clone(),
        aim: c.aim,
        inputs,
    }
}

impl<V: Render> Driver<V> for VrKit<'_, V> {
    fn web_spawns(&self) -> &[String] {
        &self.web_spawns
    }

    /// 要素の中心にレーザーを当てる。
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.point_at(Vec2::new(c.x.into(), c.y.into()))
    }

    /// 要素の中心を狙って選択ボタンを押して離す。
    fn click(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector)
            .press(Action::Select)
            .release(Action::Select)
    }

    /// 要素の中心を狙って選択ボタンを押したままにする。
    fn down(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector).press(Action::Select)
    }

    fn up(&mut self) -> &mut Self {
        self.release(Action::Select)
    }

    /// 要素の中心を狙い、スティックを1フレームだけ縦に倒す。
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        self.hover(selector);
        self.ctrl().stick = Vec2::new(0.0, lines);
        self.step();
        self.ctrl().stick = Vec2::ZERO;
        self.step()
    }

    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.views[self.panel]
            .as_ref()
            .expect("画面型が異なる")
            .read_with(self.cx, |v, _| f(v))
    }

    /// VR パネルに映っている色。
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        self.vr_color_at_px(Vec2::new(c.x.into(), c.y.into()))
    }
}
