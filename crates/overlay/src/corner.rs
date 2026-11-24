//! 角を掴んで縦横の解像度を変える（[`Overlay::set_corner_resize`]）。中心は動かさず、手が動いた軸だけ両側へ対称に
//! 広げる・縮める。1m あたりのピクセル数は保つので、UI の表示領域（ビューポート）がそのまま広がる・狭まる。

use super::*;
use design::CORNER_HANDLE_PX;

/// 角とみなす範囲: パネル面の角から内側へこの幅（px）と、その外側の影の余白。
// ponytail: 固定の幅。掴みにくい・誤って掴むならここで調整する。
const ZONE_PX: f32 = 64.0;

/// 角を掴んで変えられる解像度の範囲と、縦横比を保つか（[`Overlay::set_corner_resize`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeLimits {
    pub min: (u32, u32),
    pub max: (u32, u32),
    /// 掴んだときの縦横比のまま大きさを変える。
    pub keep_aspect: bool,
}

impl ResizeLimits {
    /// 解像度の下限と上限（下限は 1px 以上、上限は下限以上）。
    fn bounds(&self) -> (Vec2, Vec2) {
        let lo = Vec2::new(self.min.0.max(1) as f32, self.min.1.max(1) as f32);
        (lo, Vec2::new(self.max.0 as f32, self.max.1 as f32).max(lo))
    }
}

/// 角を掴んで大きさを変えている途中の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stretch {
    pub(crate) panel: PanelId,
    /// 掴んだ角（パネルローカルの x・y の向き。右上なら (1, 1)）。
    pub(crate) corner: Vec2,
    /// 掴んだ点から角までの距離（m）。レーザーが面に当たる点にこれを足すと大きさの半分になる（掴んだ角がレーザーに付いてくる）。
    grip: Vec2,
    /// 補間中の大きさ（m。解像度に丸める前）。
    size: Vec2,
    /// 掴んだときの大きさ（m。縦横比を保つときの比率）。
    start: Vec2,
}

impl Stretch {
    /// `p` の角 `corner` を `aim` で掴む。
    pub(crate) fn grab(panel: PanelId, p: &Panel, corner: Vec2, aim: &Pose) -> Option<Self> {
        let (local, _) = plane_point(p, aim)?;
        Some(Self {
            panel,
            corner,
            grip: p.size / 2.0 - local.truncate() * corner,
            size: p.size,
            start: p.size,
        })
    }

    /// レーザーが面に当たる点から決まる大きさへ、経過 `dt` 秒分だけ近づけ、その大きさに合う解像度にする。
    /// 返すのはレーザーの当たる点（トラッキング空間）とレーザー上の距離。
    pub(crate) fn follow(&mut self, p: &mut Panel, aim: &Pose, dt: f32) -> Option<(Vec3, f32)> {
        let (local, t) = plane_point(p, aim)?;
        let limits = p.corner_resize?;
        let density = Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32) / p.size;
        // 範囲の外へは目標ごと止める（範囲の外で手を動かしても、戻すときに遅れない）。
        let (lo_px, hi_px) = limits.bounds();
        let (lo, hi) = (lo_px / density, hi_px / density);
        let goal = (local.truncate() * self.corner + self.grip) * 2.0;
        let goal = if limits.keep_aspect {
            // 掴んだときの縦横比の大きさのうち、レーザーの当たる点から決まる大きさに最も近いもの。
            let s = goal.dot(self.start) / self.start.length_squared();
            let (s_lo, s_hi) = (
                (lo / self.start).max_element(),
                (hi / self.start).min_element(),
            );
            self.start * s.clamp(s_lo, s_hi.max(s_lo))
        } else {
            goal.clamp(lo, hi)
        };
        self.size = if self.size.abs_diff_eq(goal, SNAP) {
            goal
        } else {
            self.size.lerp(goal, smoothing(dt))
        };
        let res = (self.size * density).round().clamp(lo_px, hi_px);
        let res = (res.x as u32, res.y as u32);
        if res != p.resolution {
            p.size = Vec2::new(res.0 as f32, res.1 as f32) / density;
            p.resolution = res;
        }
        Some((aim.position + aim.forward() * t, t))
    }
}

/// パネルのピクセル `px` が角の近くなら、その角の向き（[`Stretch::corner`]）。角で大きさを変えられないパネルは `None`。
pub(crate) fn corner_at(p: &Panel, px: Vec2) -> Option<Vec2> {
    p.corner_resize?;
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    let reach = face_margin(p) * h / p.size.y + ZONE_PX;
    let side = |v: f32, len: f32| {
        (v.min(len - v) <= reach).then_some(if v < len / 2.0 { -1.0 } else { 1.0 })
    };
    // ピクセルは下向き、ローカルは上向き。
    Some(Vec2::new(side(px.x, w)?, -side(px.y, h)?))
}

impl Overlay {
    /// 角で大きさを変えられるようにする（`None` で戻す）。変えた解像度は `limits` の範囲に収める。
    pub fn set_corner_resize(&mut self, id: PanelId, limits: Option<ResizeLimits>) {
        self.panels[id].corner_resize = limits;
    }

    /// 角を指している手・角を掴んでいる手のパネルに、その角を囲む線（[`Pointer::Corner`]）を置く。
    pub(crate) fn place_corners(&mut self, vr: &mut dyn VrBackend) {
        for &id in &self.present {
            let Some((hand, Pointer::Corner)) =
                self.panels[id].pointer.filter(|_| !self.panels[id].closed)
            else {
                continue;
            };
            let at = self
                .hands
                .get(&hand)
                .and_then(|h| h.stretch.map(|s| (s.panel, s.corner)).or(h.corner));
            let placed = at.map(|(target, corner)| {
                let t = &self.panels[target];
                let shown = t.displayed_size();
                let k = shown.x / t.size.x;
                // 線の窓の中心をパネル面の角（丸める前の角）に合わせ、窓 1px をパネル 1px にする。
                let face = shown / 2.0 - Vec2::splat(face_margin(t) * k);
                let roll = corner.y.atan2(corner.x) + std::f32::consts::FRAC_PI_4;
                let pose = t.pose
                    * Pose::new(
                        (face * corner).extend(LIFT_STEP),
                        Quat::from_rotation_z(roll),
                    );
                let width = CORNER_HANDLE_PX as f32 * t.size.x / t.resolution.0 as f32 * k;
                (pose, width)
            });
            let p = &mut self.panels[id];
            p.follow = None;
            if let Some((pose, _)) = placed {
                p.pose = pose;
            }
            p.surface
                .show(vr, placed.is_some(), placed.map(|(_, w)| w), p.size.x);
        }
    }
}

#[cfg(test)]
mod tests;
