//! 角を掴んで縦横の解像度を変える（[`Overlay::set_corner_resize`]）。中心は動かさず、手が動いた軸だけ両側へ対称に
//! 広げる・縮める。1m あたりのピクセル数は保つので、UI の表示領域（ビューポート）が広がる・狭まる。
//! 掴んでいる間は解像度を変えず（画面に描き直させない）、中身を半透明の黒い板（代わりの板）へフェードで切り替えて板だけを
//! 伸び縮みさせる。離したときに解像度を変え、新しい大きさの絵が届いたら中身へフェードで戻す。

use super::magnet::GHOST_SINK;
use super::*;
use metrics::{CORNER_HANDLE_PX, PANEL_RADIUS};

/// 角とみなす範囲: パネル面の角から内側へこの幅（px）と、その外側の影の余白。
// ponytail: 固定の幅。掴みにくい・誤って掴むならここで調整する。
const ZONE_PX: f32 = 64.0;
/// 代わりの板の不透明度と、板の絵の画素数（パネルの画素数に対する割合。板は無地なので粗くてよい）。
const STAND_IN_ALPHA: f32 = 0.6;
const STAND_IN_SCALE: f32 = 0.25;
/// 離した後、新しい大きさの絵を待つ上限（秒）。絵が届かなくても（隠れている間など）、これを過ぎたら中身へ戻す。
const STAND_IN_WAIT: f32 = 1.0;

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

/// 角を掴んで大きさを変えている途中の状態。パネルそのものは離すまで変えない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stretch {
    pub(crate) panel: PanelId,
    /// 掴んだ角（パネルローカルの x・y の向き。右上なら (1, 1)）。
    pub(crate) corner: Vec2,
    /// 掴んだ点から角までの距離（m）。レーザーが面に当たる点にこれを足すと大きさの半分になる（掴んだ角がレーザーに付いてくる）。
    grip: Vec2,
    /// 補間中の大きさ（m。影の余白を含むパネル全体。解像度に丸める前）。代わりの板と角の線はこの大きさに置く。
    pub(crate) size: Vec2,
    /// 掴んだときの大きさ（m。縦横比を保つときの比率）。
    start: Vec2,
    /// 離したときに使う解像度（`size` を丸めて範囲に収めたもの）。
    res: (u32, u32),
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
            res: p.resolution,
        })
    }

    /// レーザーが面に当たる点から決まる大きさへ、経過 `dt` 秒分だけ近づける。
    /// 返すのはレーザーの当たる点（トラッキング空間）とレーザー上の距離。
    pub(crate) fn follow(&mut self, p: &Panel, aim: &Pose, dt: f32) -> Option<(Vec3, f32)> {
        let (local, t) = plane_point(p, aim)?;
        let limits = p.corner_resize?;
        let density = density(p);
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
        self.res = (res.x as u32, res.y as u32);
        Some((aim.position + aim.forward() * t, t))
    }

    /// 離したときに、今の大きさの解像度を `p` に反映する。変わったら `true`。
    pub(crate) fn commit(&self, p: &mut Panel) -> bool {
        if self.res == p.resolution {
            return false;
        }
        p.size = Vec2::new(self.res.0 as f32, self.res.1 as f32) / density(p);
        p.resolution = self.res;
        true
    }
}

/// 1m あたりのピクセル数（縦横別）。
fn density(p: &Panel) -> Vec2 {
    Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32) / p.size
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

/// 手ごとの角の線（[`Pointer::Corner`]）の出具合。
#[derive(Debug)]
pub(crate) struct Line {
    /// 出具合（角を指している・掴んでいる間は 1 へ、外れると 0 へ）。
    shown: Transition,
    /// 最後に置いた角（パネルと角の向き）。外れてから消えきるまで同じ角に置く。
    at: Option<(PanelId, Vec2)>,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            shown: Transition::rest(0.0),
            at: None,
        }
    }
}

/// 角を掴んでいる間、中身の代わりに出す半透明の黒い板。
#[derive(Debug)]
pub(crate) struct StandIn {
    surface: Surface,
    /// 板の出具合（1 で板だけが見える。中身の不透明度は 1 から引いたもの。[`Panel::stand_in`]）。
    shown: Transition,
    /// 最後に送った板の絵の画素数。
    sent: Option<(u32, u32)>,
    /// 離して解像度を変えた後、新しい大きさの絵を待っている（変える前の絵の画素数と、離した時刻）。
    pub(crate) waiting: Option<(Option<(u32, u32)>, Instant)>,
}

impl Overlay {
    /// 角で大きさを変えられるようにする（`None` で戻す）。変えた解像度は `limits` の範囲に収める。
    pub fn set_corner_resize(&mut self, id: PanelId, limits: Option<ResizeLimits>) {
        self.panels[id].corner_resize = limits;
    }

    /// いずれかの手が `id` の角を掴んで大きさを変えているなら、その大きさ（代わりの板の大きさ。[`Stretch::size`]）。
    /// ナビゲーションバーと角の線は、パネルの大きさの代わりにこれに合わせる。
    pub(crate) fn stretched(&self, id: PanelId) -> Option<Vec2> {
        self.hands
            .values()
            .find_map(|h| h.stretch.filter(|s| s.panel == id))
            .map(|s| s.size)
    }

    /// `hand` で角を掴んで大きさを変えている途中か（角の線の色を変える）。
    pub fn corner_held(&self, hand: Hand) -> bool {
        self.hands.get(&hand).is_some_and(|h| h.stretch.is_some())
    }

    /// 角を掴んでいるパネルの中身を代わりの板へフェードで切り替え、板を掴んでいる大きさに置く。離した後は、新しい大きさの
    /// 絵が届いたら（[`Overlay::submit`]）中身へ戻し、消えきった板は捨てる。
    pub(crate) fn place_stand_ins(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        let held: Vec<Stretch> = self.hands.values().filter_map(|h| h.stretch).collect();
        for s in &held {
            if self.stand_ins.contains_key(&s.panel) {
                continue;
            }
            let p = &self.panels[s.panel];
            match Surface::new(vr, &format!("nyaw.stand-in.{}", s.panel), p.size.x, p.pose) {
                Ok(surface) => {
                    let stand_in = StandIn {
                        surface,
                        shown: Transition::rest(0.0),
                        sent: None,
                        waiting: None,
                    };
                    self.stand_ins.insert(s.panel, stand_in);
                }
                Err(e) => logger::warn!("代わりの板を作れない", error = ?e),
            }
        }
        let panels = &mut self.panels;
        self.stand_ins.retain(|&id, si| {
            let p = &mut panels[id];
            let stretch = held.iter().find(|s| s.panel == id);
            if stretch.is_some() {
                si.waiting = None;
            }
            if si.waiting.is_some_and(|(before, at)| {
                p.surface.texture_size() != before
                    || now.saturating_duration_since(at).as_secs_f32() >= STAND_IN_WAIT
            }) {
                si.waiting = None;
            }
            let on = !p.closed && (stretch.is_some() || si.waiting.is_some());
            // 中身が消えて板が出る・中身が戻る。
            let (to, motion) = if on {
                (1.0, Motion::Exit)
            } else {
                (0.0, Motion::Enter)
            };
            si.shown.go(to, motion, now, p.animated);
            let shown = si.shown.at(now).0;
            if shown != p.stand_in {
                p.stand_in = shown;
                Self::present(vr, p, false);
            }
            if shown == 0.0 && to == 0.0 {
                si.surface.release(vr);
                return false;
            }
            // 影の余白を除いたパネル面の大きさ。
            let size = stretch.map_or(p.size, |s| s.size);
            let face = size - Vec2::splat(2.0 * face_margin(p));
            let px = (face * density(p) * STAND_IN_SCALE).round().max(Vec2::ONE);
            let px = (px.x as u32, px.y as u32);
            // 中身のすぐ上。そのパネルに重ねるもの（ナビバーなど）より下（[`Self::place_layers`]）。
            let sort = p.surface.sort() + 1;
            if si.surface.sort() != sort {
                si.surface.set_sort(vr, sort);
            }
            let upload = si.sent != Some(px);
            if upload {
                let rgba = rounded_rect(px, PANEL_RADIUS * STAND_IN_SCALE, STAND_IN_ALPHA);
                si.surface.store(px.0, px.1, &rgba);
                si.sent = Some(px);
            }
            si.surface.show(vr, true, Some(face.x), face.x);
            si.surface.present(vr, shown * p.opacity, 0.0, upload);
            // 中身と同じ面に重ねると、深度で並べるランタイムで中身とちらつくので少し手前に置く。
            let lift = Pose::new(Vec3::Z * LIFT_STEP / 2.0, Quat::IDENTITY);
            si.surface.place(vr, p.pose * lift, None);
            true
        });
    }

    /// 角を指している手・角を掴んでいる手のパネルに、その角を囲む線（[`Pointer::Corner`]）をフェードで出し入れする。
    pub(crate) fn place_corners(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        for &id in &self.present {
            let Some((hand, Pointer::Corner)) =
                self.panels[id].pointer.filter(|_| !self.panels[id].closed)
            else {
                continue;
            };
            let h = self.hands.entry(hand).or_default();
            let target = h.stretch.map(|s| (s.panel, s.corner)).or(h.corner);
            if target.is_some() {
                h.line.at = target;
            }
            let (to, motion) = if target.is_some() {
                (1.0, Motion::Enter)
            } else {
                (0.0, Motion::Exit)
            };
            h.line.shown.go(to, motion, now, true);
            let alpha = h.line.shown.at(now).0;
            let at = h
                .line
                .at
                .filter(|&(t, _)| (alpha > 0.0 || to > 0.0) && !self.panels[t].closed);
            let placed = at.map(|(target, corner)| {
                let t = &self.panels[target];
                // 出入りのアニメーションの縮み。掴んでいる間は代わりの板の大きさに合わせる。
                let k = t.displayed_size().x / t.size.x;
                let size = self.stretched(target).unwrap_or(t.size) * k;
                // 線の窓の中心をパネル面の角（丸める前の角）に合わせ、窓 1px をパネル 1px にする。
                // ナビゲーションバーと重なっても前に出ないよう、はまる所の見本と同じだけ奥に置く（描画順もパネルの下）。
                let face = size / 2.0 - Vec2::splat(face_margin(t) * k);
                let roll = corner.y.atan2(corner.x) + std::f32::consts::FRAC_PI_4;
                let pose = t.pose
                    * Pose::new(
                        (face * corner).extend(-GHOST_SINK),
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
            // 線の窓の不透明度は出具合にする（窓の絵が届くたびにも同じ値で送る。[`Overlay::submit`]）。
            if placed.is_some() && p.opacity != alpha {
                p.opacity = alpha;
                Self::present(vr, p, false);
            }
        }
    }
}

/// 幅・高さ `px` の角丸矩形（半径 `r`）を不透明度 `alpha` の黒で塗った、事前乗算アルファの RGBA。角の縁は覆う割合で滑らかにする。
fn rounded_rect((w, h): (u32, u32), r: f32, alpha: f32) -> Vec<u8> {
    let r = r.min(w.min(h) as f32 / 2.0);
    let (fw, fh) = (w as f32, h as f32);
    let mut out = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // 画素の中心から、近い角の丸みの中心までの距離（角の丸みの外でなければ 0）。
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (r - cx).max(cx - (fw - r)).max(0.0);
            let dy = (r - cy).max(cy - (fh - r)).max(0.0);
            let cover = (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            out[((y * w + x) * 4 + 3) as usize] = (alpha * cover * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests;
