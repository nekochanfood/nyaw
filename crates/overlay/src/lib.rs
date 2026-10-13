//! Layer 3: オーバーレイの土台。3D 空間上のパネルを持ち、各コントローラのレーザーとの交差を判定し、
//! ヒットを UI 層向けの 2D ポインタイベントに変換する。パネルを掴んで動かす操作も扱う。
//! パネルはデバイス（HMD・コントローラ・トラッカー）に取り付けて追従させられる。
//! [`Overlay::tick`] が下位パイプライン1フレーム分: VR poll → 取り付け先への追従 → 入力マップ → ヒット判定 → イベント。

use std::collections::HashMap;

use vr::{Device, Frame, Hand, OverlayId, Pose, Vec2, Vec3, VrBackend, VrError};

pub use input::ControllerState;

pub type PanelId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// クアッドの中心（トラッキング空間）。表面はローカル +Z を向く。
    /// 取り付け中は毎 tick `取り付け先の姿勢 * オフセット` で更新される。
    pub pose: Pose,
    /// メートル単位。
    pub size: Vec2,
    /// クアッドに貼る UI サーフェスのピクセル数。
    pub resolution: (u32, u32),
    /// 取り付け先のデバイスと、そこから見たパネルの相対姿勢。
    pub attached: Option<(Device, Pose)>,
    /// 掴んでも動かない。取り付けていなければ空間に固定される。
    pub locked: bool,
    overlay: OverlayId,
    /// ランタイムへ最後に送った姿勢。変わったときだけ送る。
    sent: Option<Pose>,
}

impl Panel {
    /// パネル上のピクセル（左上原点）をトラッキング空間の点に変換する。ヒット判定の逆。
    pub fn pixel_to_world(&self, px: Vec2) -> Vec3 {
        let (w, h) = (self.resolution.0 as f32, self.resolution.1 as f32);
        let local = Vec3::new(
            (px.x / w - 0.5) * self.size.x,
            (0.5 - px.y / h) * self.size.y,
            0.0,
        );
        self.pose.transform_point(local)
    }

    /// ランタイム側のオーバーレイ。実ランタイムから描画を読み戻して確かめるとき用。
    pub fn overlay_id(&self) -> OverlayId {
        self.overlay
    }
}

/// 座標はパネルのピクセル、原点は左上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Move { pos: Vec2 },
    Down { pos: Vec2 },
    Up { pos: Vec2 },
    Scroll { pos: Vec2, delta: Vec2 },
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiEvent {
    pub panel: PanelId,
    pub hand: Hand,
    pub event: PointerEvent,
}

#[derive(Debug, Default)]
struct HandState {
    hover: Option<PanelId>,
    select: bool,
    grab_held: bool,
    menu: bool,
    /// ドラッグ中のパネルと、コントローラから見たその相対姿勢。
    dragging: Option<(PanelId, Pose)>,
}

#[derive(Debug, Default)]
pub struct Overlay {
    panels: Vec<Panel>,
    hands: HashMap<Hand, HandState>,
    /// 最後の tick で見えていたトラッカーの ID（取り付け先の候補）。
    trackers: Vec<String>,
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_panel(
        &mut self,
        vr: &mut dyn VrBackend,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        let overlay = vr.create_overlay(key, size.x)?;
        vr.set_overlay_pose(overlay, pose);
        self.panels.push(Panel {
            pose,
            size,
            resolution,
            attached: None,
            locked: false,
            overlay,
            sent: None,
        });
        Ok(self.panels.len() - 1)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        &self.panels[id]
    }

    /// 最後の tick で見えていたトラッカーの ID。
    pub fn trackers(&self) -> &[String] {
        &self.trackers
    }

    /// どちらかの手でメニューボタンを押している。
    pub fn menu_held(&self) -> bool {
        self.hands.values().any(|h| h.menu)
    }

    /// `id` をどちらかの手で掴んでいる。
    pub fn grabbed(&self, id: PanelId) -> bool {
        self.hands
            .values()
            .any(|h| h.dragging.is_some_and(|d| d.0 == id))
    }

    /// パネルを `device` に取り付ける。以降の tick で `device` の姿勢 * `offset` に追従する。
    /// 取り付け中のパネルを掴んで動かすと、離した位置でオフセットが付け直される。
    // ponytail: 姿勢は tick（= vsync ごとの poll）の姿勢でオーバーレイ層が合成して送るので、1フレーム分遅れる。
    // 揺れが気になったらランタイムの相対配置（OpenVR の SetOverlayTransformTrackedDeviceRelative 等）に任せる。
    pub fn attach(&mut self, id: PanelId, device: Device, offset: Pose) {
        self.panels[id].attached = Some((device, offset));
    }

    /// 取り付けを外す。パネルはその場（最後の姿勢）に残る。
    pub fn detach(&mut self, id: PanelId) {
        self.panels[id].attached = None;
    }

    /// 掴んで動かせないようにする（`false` で戻す）。レーザーでの操作はそのまま使える。
    pub fn set_locked(&mut self, id: PanelId, locked: bool) {
        self.panels[id].locked = locked;
    }

    /// `panel` の描画済み UI フレームを VR ランタイムへ送る。
    /// テクスチャの大きさは `resolution`（ヒット判定用の論理ピクセル）と独立してよい。
    /// HiDPI では UI が `resolution × 拡大率` で描かれるため。
    pub fn submit(
        &self,
        vr: &mut dyn VrBackend,
        panel: PanelId,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        vr.submit_overlay_pixels(self.panels[panel].overlay, width, height, rgba);
    }

    pub fn tick(&mut self, vr: &mut dyn VrBackend) -> Vec<UiEvent> {
        let frame = vr.poll();
        self.trackers = frame.trackers.iter().map(|(id, _)| id.clone()).collect();
        // 掴み中のパネルは手に従う（離したフレームで update_hand がオフセットを付け直す）。
        // 取り付け先が見失われたフレームは最後の姿勢のまま。
        let dragged: Vec<PanelId> = self
            .hands
            .values()
            .filter_map(|h| h.dragging)
            .map(|d| d.0)
            .collect();
        for (id, p) in self.panels.iter_mut().enumerate() {
            if dragged.contains(&id) {
                continue;
            }
            if let Some(pose) = p
                .attached
                .as_ref()
                .and_then(|(d, o)| Some(frame.pose(d)? * *o))
            {
                p.pose = pose;
            }
        }
        let mut events = Vec::new();
        for c in frame.controllers.iter().map(input::map) {
            self.update_hand(&c, &frame, &mut events);
        }
        for p in &mut self.panels {
            if p.sent != Some(p.pose) {
                vr.set_overlay_pose(p.overlay, p.pose);
                p.sent = Some(p.pose);
            }
        }
        events
    }

    fn update_hand(&mut self, c: &ControllerState, frame: &Frame, out: &mut Vec<UiEvent>) {
        let st = self.hands.entry(c.hand).or_default();
        let grab_started = c.grab && !st.grab_held;
        st.grab_held = c.grab;
        st.menu = c.menu;

        if let Some((id, offset)) = st.dragging {
            if c.grab {
                self.panels[id].pose = c.aim * offset;
                return;
            }
            st.dragging = None;
            // 取り付け中なら、離した位置を新しいオフセットにする。
            let p = &mut self.panels[id];
            if let Some((device, offset)) = &mut p.attached
                && let Some(dp) = frame.pose(device)
            {
                *offset = dp.inverse() * p.pose;
            }
        }

        // レーザー上で最も近いパネル。
        let hit = self
            .panels
            .iter()
            .enumerate()
            .filter_map(|(id, p)| hit_test(p, &c.aim).map(|(pos, t)| (id, pos, t)))
            .min_by(|a, b| a.2.total_cmp(&b.2));
        // ロック中のパネルは掴めない（掴みボタンは無視して普通に指す）。
        let grab_started = grab_started && hit.is_some_and(|h| !self.panels[h.0].locked);

        let mut emit = |panel, event| {
            out.push(UiEvent {
                panel,
                hand: c.hand,
                event,
            })
        };
        let new_hover = hit.map(|h| h.0);
        if let Some(old) = st
            .hover
            .filter(|&old| Some(old) != new_hover || grab_started)
        {
            emit(old, PointerEvent::Leave);
        }

        let Some((id, pos, _)) = hit else {
            st.hover = None;
            st.select = false;
            return;
        };
        if grab_started {
            st.dragging = Some((id, c.aim.inverse() * self.panels[id].pose));
            st.hover = None;
            st.select = false;
            return;
        }

        emit(id, PointerEvent::Move { pos });
        if c.select != st.select {
            emit(
                id,
                if c.select {
                    PointerEvent::Down { pos }
                } else {
                    PointerEvent::Up { pos }
                },
            );
        }
        if c.scroll != Vec2::ZERO {
            emit(
                id,
                PointerEvent::Scroll {
                    pos,
                    delta: c.scroll,
                },
            );
        }
        st.hover = Some(id);
        st.select = c.select;
    }
}

/// レイ（aim の前方）とパネルの交差判定。ヒットしたピクセルとレイ上の距離を返す。
fn hit_test(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    let normal = p.pose.orientation * Vec3::Z;
    let dir = aim.forward();
    let denom = dir.dot(normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (p.pose.position - aim.position).dot(normal) / denom;
    if t < 0.0 {
        return None;
    }
    let local = p.pose.inverse().transform_point(aim.position + dir * t);
    if local.x.abs() > p.size.x / 2.0 || local.y.abs() > p.size.y / 2.0 {
        return None;
    }
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    Some((
        Vec2::new(
            (local.x / p.size.x + 0.5) * w,
            (0.5 - local.y / p.size.y) * h,
        ),
        t,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::{Quat, RawController, mock::MockBackend};

    fn setup() -> (MockBackend, Overlay) {
        let mut vr = MockBackend::default();
        let mut ov = Overlay::new();
        let pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
        ov.add_panel(&mut vr, "main", pose, Vec2::new(1.0, 1.0), (100, 100))
            .unwrap();
        (vr, ov)
    }

    fn controller(vr: &mut MockBackend, aim: Pose, inputs: &[(&str, f32)]) {
        vr.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: "valve/index_controller".into(),
            aim,
            inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }];
    }

    fn kinds(ev: &[UiEvent]) -> Vec<PointerEvent> {
        ev.iter().map(|e| e.event).collect()
    }

    #[test]
    fn laser_click_on_panel() {
        let (mut vr, mut ov) = setup();
        let center = Vec2::new(50.0, 50.0);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move { pos: center },
                PointerEvent::Down { pos: center }
            ]
        );
        controller(&mut vr, Pose::IDENTITY, &[]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move { pos: center },
                PointerEvent::Up { pos: center }
            ]
        );
    }

    #[test]
    fn hit_maps_to_top_left_pixels_and_leave() {
        let (mut vr, mut ov) = setup();
        controller(
            &mut vr,
            Pose::new(Vec3::new(-0.25, 0.25, 0.0), Quat::IDENTITY),
            &[],
        );
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(25.0, 25.0)
            }]
        );
        controller(
            &mut vr,
            Pose::new(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY),
            &[],
        );
        assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
    }

    #[test]
    fn grab_moves_panel_and_syncs_to_vr() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        let moved = Pose::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
        controller(&mut vr, moved, &[("squeeze/value", 1.0)]);
        assert!(ov.tick(&mut vr).is_empty());
        let expected = Vec3::new(1.0, 0.0, -1.0);
        assert!(ov.panel(0).pose.position.abs_diff_eq(expected, 1e-5));
        assert!(vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn pixel_to_world_inverts_hit_test() {
        let (_, ov) = setup();
        let p = ov.panel(0);
        let px = Vec2::new(10.0, 70.0);
        let target = p.pixel_to_world(px);
        let aim = Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_arc(Vec3::NEG_Z, target.normalize()),
        );
        let (hit, _) = hit_test(p, &aim).unwrap();
        assert!(hit.abs_diff_eq(px, 1e-3), "{hit}");
    }

    fn at(x: f32, y: f32, z: f32) -> Pose {
        Pose::new(Vec3::new(x, y, z), Quat::IDENTITY)
    }

    fn assert_at(ov: &Overlay, vr: &MockBackend, expected: Vec3) {
        let got = ov.panel(0).pose.position;
        assert!(got.abs_diff_eq(expected, 1e-5), "{got}");
        assert!(vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn attached_to_hmd_follows_head_including_rotation() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -0.5));
        vr.frame.hmd = Pose::new(
            Vec3::new(1.0, 1.5, 0.0),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        ov.tick(&mut vr);
        // 左を向いた頭の 0.5m 前方 = -X 方向。
        assert_at(&ov, &vr, Vec3::new(0.5, 1.5, 0.0));
        assert!(
            ov.panel(0)
                .pose
                .orientation
                .abs_diff_eq(vr.frame.hmd.orientation, 1e-5)
        );
    }

    #[test]
    fn attached_to_controller_follows_hand() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Controller(Hand::Right), at(0.0, 0.1, 0.0));
        controller(&mut vr, at(0.3, -0.2, -0.4), &[]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.3, -0.1, -0.4));
    }

    #[test]
    fn attached_to_tracker_and_keeps_pose_when_lost() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Tracker("waist".into()), at(0.0, 0.0, -0.2));
        vr.frame.trackers = vec![("waist".into(), at(0.0, 1.0, 0.0))];
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
        vr.frame.trackers.clear();
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
    }

    #[test]
    fn detach_leaves_panel_in_place() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        vr.frame.hmd = at(2.0, 0.0, 0.0);
        ov.tick(&mut vr);
        ov.detach(0);
        vr.frame.hmd = at(5.0, 0.0, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(2.0, 0.0, -1.0));
    }

    #[test]
    fn dragging_attached_panel_reanchors_offset() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        // 右手で掴んで 1m 右へ動かして離す。
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
        // 頭が動けば、新しいオフセットのまま追従する。
        vr.frame.hmd = at(0.0, 0.5, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.5, -1.0));
    }

    #[test]
    fn locked_panel_ignores_grab_but_still_takes_pointer() {
        let (mut vr, mut ov) = setup();
        ov.set_locked(0, true);
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(50.0, 50.0)
            }]
        );
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn unlocked_panel_can_be_grabbed_again() {
        let (mut vr, mut ov) = setup();
        ov.set_locked(0, true);
        ov.set_locked(0, false);
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
    }

    #[test]
    fn locked_attached_panel_still_follows_device() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        ov.set_locked(0, true);
        vr.frame.hmd = at(0.0, 0.5, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 0.5, -1.0));
    }

    #[test]
    fn reports_trackers_menu_and_grab() {
        let (mut vr, mut ov) = setup();
        vr.frame.trackers = vec![("LHR-1".into(), Pose::IDENTITY)];
        controller(
            &mut vr,
            Pose::IDENTITY,
            &[("b/click", 1.0), ("squeeze/value", 1.0)],
        );
        ov.tick(&mut vr);
        assert_eq!(ov.trackers(), ["LHR-1"]);
        assert!(ov.menu_held() && ov.grabbed(0));
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(!ov.menu_held() && !ov.grabbed(0));
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut vr, ov) = setup();
        let mut rgba = vec![0; 200 * 200 * 4];
        rgba[..4].copy_from_slice(&[1, 2, 3, 4]);
        ov.submit(&mut vr, 0, 200, 200, &rgba);
        let tex = vr.overlays[0].texture.as_ref().unwrap();
        assert_eq!((tex.width, tex.height), (200, 200));
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }
}
