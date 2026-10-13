//! Layer 3: オーバーレイの土台。3D 空間上のパネルを持ち、各コントローラのレーザーとの交差を判定し、
//! ヒットを UI 層向けの 2D ポインタイベントに変換する。パネルを掴んで動かす操作も扱う。
//! [`Overlay::tick`] が下位パイプライン1フレーム分: VR poll → 入力マップ → ヒット判定 → イベント。

use std::collections::HashMap;

use vr::{Hand, OverlayId, Pose, Vec2, Vec3, VrBackend, VrError};

pub use input::ControllerState;

pub type PanelId = usize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panel {
    /// クアッドの中心。表面はローカル +Z を向く。
    pub pose: Pose,
    /// メートル単位。
    pub size: Vec2,
    /// クアッドに貼る UI サーフェスのピクセル数。
    pub resolution: (u32, u32),
    overlay: OverlayId,
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
    /// ドラッグ中のパネルと、コントローラから見たその相対姿勢。
    dragging: Option<(PanelId, Pose)>,
}

#[derive(Debug, Default)]
pub struct Overlay {
    panels: Vec<Panel>,
    hands: HashMap<Hand, HandState>,
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
            overlay,
        });
        Ok(self.panels.len() - 1)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        &self.panels[id]
    }

    /// `panel` の描画済み UI フレームを VR ランタイムへ送る。
    pub fn submit(&self, vr: &mut dyn VrBackend, panel: PanelId, rgba: &[u8]) {
        let p = &self.panels[panel];
        vr.submit_overlay_pixels(p.overlay, p.resolution.0, p.resolution.1, rgba);
    }

    pub fn tick(&mut self, vr: &mut dyn VrBackend) -> Vec<UiEvent> {
        let frame = vr.poll();
        let mut events = Vec::new();
        for c in frame.controllers.iter().map(input::map) {
            self.update_hand(&c, &mut events);
        }
        // ponytail: 毎フレーム全パネルの姿勢を送る。ランタイム呼び出しが重くなったら変更分だけにする。
        for p in &self.panels {
            vr.set_overlay_pose(p.overlay, p.pose);
        }
        events
    }

    fn update_hand(&mut self, c: &ControllerState, out: &mut Vec<UiEvent>) {
        let st = self.hands.entry(c.hand).or_default();
        let grab_started = c.grab && !st.grab_held;
        st.grab_held = c.grab;

        if let Some((id, offset)) = st.dragging {
            if c.grab {
                self.panels[id].pose = c.aim * offset;
                return;
            }
            st.dragging = None;
        }

        // レーザー上で最も近いパネル。
        let hit = self
            .panels
            .iter()
            .enumerate()
            .filter_map(|(id, p)| hit_test(p, &c.aim).map(|(pos, t)| (id, pos, t)))
            .min_by(|a, b| a.2.total_cmp(&b.2));

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
    fn submit_reaches_backend() {
        let (mut vr, ov) = setup();
        ov.submit(&mut vr, 0, &vec![0; 100 * 100 * 4]);
        assert_eq!(vr.overlays[0].submitted, Some((100, 100)));
    }
}
