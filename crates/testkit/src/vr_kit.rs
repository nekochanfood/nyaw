//! VR モードの E2E。全層（モック VR → 入力 → オーバーレイ → UI → GPUI）を丸ごと持ち、
//! レーザー・コントローラ機種・手・パネル操作・VR に映る描画など、VR 固有の機能も検証できる。
//! 毎フレーム UI の描画結果も VR モックに送られる（描画は [`crate::raster`] の CPU ラスタライザ）。

use gpui::{Bounds, Context, Entity, Pixels, Render, TestAppContext, VisualTestContext, Window};
use overlay::{Overlay, PanelId};
use ui::PointerBridge;
use vr::{
    RawController,
    mock::{MockBackend, MockTexture},
};

use crate::{Action, Device, Driver, Hand, Pose, Quat, Vec2, Vec3, raster};

/// 指定がないときの仮想コントローラの機種。
pub const DEFAULT_PROFILE: &str = "valve/index_controller";
const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
const PANEL_WIDTH_M: f32 = 1.0;

/// 仮想コントローラ。押しているアクションを、機種に応じた生入力に変換して VR モックに流す。
struct VirtualController {
    hand: Hand,
    profile: String,
    aim: Pose,
    held: Vec<Action>,
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
    view: Entity<V>,
    /// 直接覗いてよい（例: VR 側に送られたパネル姿勢の確認）。
    pub vr: MockBackend,
    pub overlay: Overlay,
    panel: PanelId,
    bridge: PointerBridge,
    controllers: Vec<VirtualController>,
    active: Hand,
}

impl<'a, V: Render> VrKit<'a, V> {
    /// `build` のビューを、正面 1m に置いたパネル1枚として起動する。パネル解像度 = ウィンドウサイズ。
    // ponytail: パネル1枚のみ。複数パネルのシナリオが要るなら panel を引数で選べるようにする。
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let size = Vec2::new(PANEL_WIDTH_M, PANEL_WIDTH_M * res.1 as f32 / res.0 as f32);

        let mut vr = MockBackend::default();
        let mut overlay = Overlay::new();
        let pose = Pose::new(PANEL_POS, Quat::IDENTITY);
        let panel = overlay
            .add_panel(&mut vr, "testkit", pose, size, res)
            .expect("モックはパネル作成に失敗しない");
        let mut kit = Self {
            cx,
            view,
            vr,
            overlay,
            panel,
            bridge: PointerBridge::default(),
            controllers: Vec::new(),
            active: Hand::Right,
        };
        kit.step(); // 最初のフレームを VR に送っておく
        kit
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
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("要素 {selector:?} が描画されていない"))
    }

    /// レーザーをパネル上のピクセルに向けて1フレーム進める。
    pub fn point_at(&mut self, px: Vec2) -> &mut Self {
        let target = self.overlay.panel(self.panel).pixel_to_world(px);
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
        let p = self.overlay.panel(self.panel);
        let center = Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32) / 2.0;
        let delta = to - p.pose.position;
        self.point_at(center).press(Action::Grab);
        self.ctrl().aim.position += delta;
        self.step().release(Action::Grab)
    }

    /// パネルを `device` に取り付けて1フレーム進める（`offset` は取り付け先から見た相対姿勢）。
    pub fn attach_panel(&mut self, device: Device, offset: Pose) -> &mut Self {
        self.overlay.attach(self.panel, device, offset);
        self.step()
    }

    pub fn panel_pose(&self) -> Pose {
        self.overlay.panel(self.panel).pose
    }

    /// 仮想コントローラの状態を VR モックに流し、本番と同じ `ui::pump` で1フレーム進めて GPUI に配送する。
    pub fn step(&mut self) -> &mut Self {
        self.vr.frame.controllers = self.controllers.iter().map(raw).collect();
        for input in ui::pump(
            &mut self.overlay,
            &mut self.vr,
            self.panel,
            &mut self.bridge,
        ) {
            self.cx.update(|window, cx| {
                window.dispatch_event(input, cx);
            });
        }
        self.cx.run_until_parked();
        let frame = self.cx.update(raster::capture);
        self.overlay.submit(
            &mut self.vr,
            self.panel,
            frame.width,
            frame.height,
            &frame.rgba,
        );
        self
    }

    /// VR パネルに最後に送られたテクスチャ。
    pub fn texture(&self) -> &MockTexture {
        // ponytail: VrKit が作るオーバーレイは1枚なので添字 0 固定。複数パネル対応時に PanelId から引く。
        self.vr.overlays[0]
            .texture
            .as_ref()
            .expect("VrKit::new で最初のフレームを送っている")
    }

    /// VR パネルに映っている、パネル上の論理ピクセル `px` の色（0xRRGGBB）。
    pub fn vr_color_at_px(&self, px: Vec2) -> u32 {
        let tex = self.texture();
        let (rw, rh) = self.overlay.panel(self.panel).resolution;
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
                });
                self.controllers.len() - 1
            }
        };
        &mut self.controllers[i]
    }
}

fn raw(c: &VirtualController) -> RawController {
    let inputs = c
        .held
        .iter()
        .map(|&a| {
            let name = input::source(&c.profile, a)
                .unwrap_or_else(|| panic!("{} には {a:?} に当たる入力がない", c.profile));
            (name.to_owned(), 1.0)
        })
        .collect();
    RawController {
        hand: c.hand,
        profile: c.profile.clone(),
        aim: c.aim,
        inputs,
    }
}

impl<V: Render> Driver<V> for VrKit<'_, V> {
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

    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.view.read_with(self.cx, |v, _| f(v))
    }

    /// VR パネルに映っている色。
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        self.vr_color_at_px(Vec2::new(c.x.into(), c.y.into()))
    }
}
