//! はめた画面を掴んで引っこ抜く・近づけてはめる磁石と、はまる所の見本（[`Pointer::Dock`]）の置き場所。
//! はめる場所に出入りできる画面（[`Panel::dockable`]）そのものを掴んだときだけ効く（バー・キーボードを掴めばダッシュボードごと動く）。

use super::*;

/// はまっている（吸い付いている）画面を、はまる所から手をこれだけ離すと抜ける（m）。それまで画面ははまる所から動かない。
const TEAR: f32 = 0.18;
/// 外れた画面がはまる所に引かれ始める距離と、吸い付く距離（m）。吸い付く距離は抜ける距離より短い（境目で行き来しない）。
// ponytail: 距離はどれも目分量。抜けにくい・吸い付きすぎると感じたらここで合わせる。
const ATTRACT: f32 = 0.35;
const CATCH: f32 = 0.1;
/// 吸い付く直前に、手の位置からはまる所へ寄せる割合。
const PULL: f32 = 0.5;

/// 見本の上に足す帯（「引っ張って外す」などの一行）の高さ（px）。帯の中身は上の層の画面が描く。
pub const DOCK_HINT_PX: u32 = 72;
/// 見本の長い辺のピクセル数の上限（大きな配信でも窓を大きくしすぎない）。
const GHOST_MAX_PX: f32 = 1024.0;
/// 見本を、はまる所からどれだけ奥に置くか（m）。同じ面に置くと、はまった画面と Z ファイティングする。
const GHOST_SINK: f32 = 0.01;
/// 見本の枠を、画面の地の縁からどれだけ外へ広げるか（m）。はまっている間も、画面の周りに枠が見える。
const GHOST_PAD: f32 = 0.01;
/// 掴んでいる画面がはまる所にこれより近づくと見本をフェードインし、これより遠ざかるとフェードアウトする（m）。
/// 濃さは距離で変えず、近いか遠いかの二択。境目で行き来しないよう、消える距離を少し遠くする。
const GHOST_SHOW: f32 = 0.45;
const GHOST_HIDE: f32 = 0.55;

/// 見本に出す一行。離すとはまるか（[`DockHint::docks`]）で、上の層は枠の色も変える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHint {
    /// はまっている画面を掴んでいる（引っ張って外す）。離してもはまったまま。
    Pull,
    /// 外れた画面が吸い付いている（離してくっつける）。離すとはまる。
    Release,
    /// 外れた画面を掴んでいる（近づけてくっつける）。離すと独立したまま。
    Attach,
}

impl DockHint {
    /// 今離すと、はまる（はまったままになる）。
    pub fn docks(self) -> bool {
        self != DockHint::Attach
    }
}

/// 掴んでいるはめられる画面と、はまる所の間の磁石。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Magnet {
    /// はめる場所の持ち主と、その上端に立てる角度。
    owner: PanelId,
    tilt: f32,
    /// はまる所に付いている（はまっていた画面を掴んだ・近づけて吸い付いた）。抜けるまで、はまる所から動かない。
    pub(crate) stuck: bool,
    /// 抜けた・吸い付いたときの動き（始めた姿勢・動き・経過秒）。終わるまで、置きたい所へこの動きで向かう。
    jump: Option<(Pose, Motion, f32)>,
}

/// 手ごとの見本の出具合。置き場所が分かるよう、出入りのフェードのほかは動かさない。
#[derive(Debug)]
pub(crate) struct Ghost {
    /// 出具合（掴んでいる間は 1 へ、離すと 0 へ）と、最後に送った不透明度。
    shown: Transition,
    alpha: f32,
    /// 今の置き場所の大きさで描いた絵が届いている（[`Overlay::submit`]）。出始めは届くまで待つ（前の絵を一瞬見せない）。
    pub(crate) ready: bool,
    /// 掴んでいる画面がはまる所に近い（[`GHOST_SHOW`] / [`GHOST_HIDE`]）。近い間だけ出す。
    near: bool,
    /// 最後に置いた所（姿勢・幅・解像度）。離してから消えきるまで同じ所に置く。
    at: Option<(Pose, f32, (u32, u32))>,
}

impl Default for Ghost {
    fn default() -> Self {
        Self {
            shown: Transition::rest(0.0),
            alpha: 0.0,
            ready: false,
            near: false,
            at: None,
        }
    }
}

/// `p` が `owner` の上端にはまっている。
fn in_slot(p: &Panel, owner: PanelId) -> bool {
    p.parent
        .is_some_and(|q| q.panel == owner && q.slot.is_some())
}

/// 抜けた画面 `p` が `owner` にはまっていれば、その場で親から外す（手に付いて動き続ける）。外したら `true`。
pub(crate) fn unplug(p: &mut Panel, owner: PanelId) -> bool {
    let docked = in_slot(p, owner);
    if docked {
        p.parent = None;
    }
    docked
}

/// `owner` の上端に、`id` のほかにはまっている画面が無い。
fn slot_free(panels: &[Panel], present: &[PanelId], owner: PanelId, id: PanelId) -> bool {
    !present.iter().any(|&c| {
        let q = &panels[c];
        c != id && !q.closed && !q.hidden && in_slot(q, owner)
    })
}

/// `edge0` で 0、`edge1` で 1 になる滑らかな段（`edge0 > edge1` でもよい）。
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Magnet {
    /// `id` を掴んだときの磁石。はめる場所が出ていて、`id` がはめられる画面で、そこにはまっているか、外れていて場所が空いていれば。
    pub(crate) fn grab(
        panels: &[Panel],
        present: &[PanelId],
        slot: Option<(PanelId, f32)>,
        id: PanelId,
    ) -> Option<Self> {
        let (owner, tilt) = slot?;
        let (o, p) = (&panels[owner], &panels[id]);
        if owner == id || !p.dockable || p.hidden || o.closed || o.hidden {
            return None;
        }
        let docked = in_slot(p, owner);
        (docked || p.parent.is_none() && slot_free(panels, present, owner, id)).then_some(Self {
            owner,
            tilt,
            stuck: docked,
            jump: None,
        })
    }

    /// 手が置きたい所 `target` と、はまる所 `slot` から、この tick の画面の姿勢（今は `cur`）。
    /// はまる所に付いている間ははまる所から動かず、抜けると手へ飛び出す（[`Motion::Pop`]）。外れている間は近づくほど
    /// はまる所へ引き寄せ、吸い付くとはまる所へ飛び込む（[`Motion::Attract`]）。抜けた・吸い付いたら `Some(stuck)`。
    pub(crate) fn follow(
        &mut self,
        cur: Pose,
        slot: Pose,
        target: Pose,
        dt: f32,
    ) -> (Pose, Option<bool>) {
        let x = target.position.distance(slot.position);
        let flipped = match self.stuck {
            true if x > TEAR => Some(false),
            false if x < CATCH => Some(true),
            _ => None,
        };
        if let Some(stuck) = flipped {
            self.stuck = stuck;
            let motion = if stuck { Motion::Attract } else { Motion::Pop };
            self.jump = Some((cur, motion, 0.0));
        }
        let want = if self.stuck {
            slot
        } else {
            let k = PULL * smoothstep(ATTRACT, CATCH, x);
            Pose::new(
                target.position.lerp(slot.position, k),
                target.orientation.slerp(slot.orientation, k),
            )
        };
        let pose = match &mut self.jump {
            Some((from, motion, t)) => {
                *t += dt;
                let p = *t * 1000.0 / motion.duration_ms() as f32;
                if p >= 1.0 {
                    self.jump = None;
                    want
                } else {
                    let e = motion.ease(p);
                    Pose::new(
                        from.position.lerp(want.position, e),
                        from.orientation.slerp(want.orientation, e.clamp(0.0, 1.0)),
                    )
                }
            }
            None if self.stuck => want,
            None => approach(cur, want, dt),
        };
        (pose, flipped)
    }

    /// はまる所（この tick の `id` の姿勢として）。
    pub(crate) fn slot(
        &self,
        panels: &[Panel],
        present: &[PanelId],
        scale: f32,
        id: PanelId,
    ) -> Pose {
        panels[self.owner].pose * slot_offset_in(panels, present, scale, id, self.owner, self.tilt)
    }

    /// はめる場所の持ち主。
    pub(crate) fn owner(&self) -> PanelId {
        self.owner
    }

    /// 掴んでいる画面 `p` に出す一行。
    fn hint(&self, p: &Panel) -> DockHint {
        match (self.stuck, in_slot(p, self.owner)) {
            (_, true) => DockHint::Pull,
            (true, false) => DockHint::Release,
            (false, false) => DockHint::Attach,
        }
    }
}

impl Overlay {
    /// `owner` の上端を、画面をはめる場所にする（`tilt` だけ起こして立てる。[`Parent::slot`]）。`None` でやめる。
    /// はめられる画面（[`Self::set_dockable`]）は、そこから掴んで引っこ抜け、近づけて離すとはまる。
    pub fn set_dock_slot(&mut self, owner: PanelId, tilt: Option<f32>) {
        self.dock_slot = match tilt {
            Some(tilt) => Some((owner, tilt)),
            None => self.dock_slot.filter(|&(o, _)| o != owner),
        };
    }

    /// はめる場所から掴んで引っこ抜ける・近づけてはめられる画面にする。
    pub fn set_dockable(&mut self, id: PanelId, dockable: bool) {
        self.panels[id].dockable = dockable;
    }

    /// 掴んで引っこ抜いた（`false`）・はめた（`true`）画面。受け取ると空になる（上の層が、はまっている画面の記録を合わせる）。
    pub fn take_dock_changes(&mut self) -> Vec<(PanelId, bool)> {
        std::mem::take(&mut self.dock_changes)
    }

    /// `hand` の見本に出す一行（その手ではめられる画面を掴んでいなければ `None`）。
    pub fn dock_hint(&self, hand: Hand) -> Option<DockHint> {
        let h = self.hands.get(&hand)?;
        let ((id, ..), m) = h.dragging.zip(h.magnet)?;
        Some(m.hint(&self.panels[id]))
    }

    /// 磁石を効かせて掴んでいる画面（親を連れていかない）。
    pub(crate) fn pulled(&self) -> Vec<PanelId> {
        self.hands
            .values()
            .filter(|h| h.magnet.is_some())
            .filter_map(|h| h.dragging.map(|d| d.0))
            .collect()
    }

    /// はまる所に付けたまま離した画面をはめる（はまっていれば何もしない）。磁石に引かれて収まる（[`Motion::Attract`]）。
    /// 離すまでの間に、ほかの画面がはまった・はめる場所が隠れた（閉じた）なら何もしない。
    pub(crate) fn land(&mut self, vr: &mut dyn VrBackend) {
        for id in std::mem::take(&mut self.landed) {
            let Some((owner, tilt)) = self
                .dock_slot
                .filter(|&(o, _)| !self.panels[o].closed && !self.panels[o].hidden)
            else {
                continue;
            };
            if self.panels[id].parent.is_none() && slot_free(&self.panels, &self.present, owner, id)
            {
                let parent = Parent {
                    panel: owner,
                    carry: true,
                    docked: true,
                    slot: Some(tilt),
                    ..Default::default()
                };
                self.set_parent(vr, id, Some(parent));
                self.dock_changes.push((id, true));
            }
        }
    }

    /// 見本の絵が届いた（[`Self::submit`]）。今の置き場所の縦横比で描いたものなら、出し始めてよい。
    /// 送るときに不透明度が 1 に戻るので、フェードの途中の濃さに合わせ直す。
    pub(crate) fn ghost_drawn(
        &mut self,
        vr: &mut dyn VrBackend,
        id: PanelId,
        width: u32,
        height: u32,
    ) {
        let p = &mut self.panels[id];
        let Some((hand, Pointer::Dock)) = p.pointer else {
            return;
        };
        let ghost = &mut self.hands.entry(hand).or_default().ghost;
        let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
        if (width as f32 / height as f32 - w / h).abs() < 0.01 {
            ghost.ready = true;
        }
        p.surface.present(vr, ghost.alpha, 0.0, false);
    }

    /// はまる所の見本（[`Pointer::Dock`]）を、その手で掴んでいる画面のはまる所の少し奥に、地より少し大きく置く（上に一行の帯を
    /// 足す）。置き場所が分かるよう動かさない: 画面がはまる所に近づくとフェードインし、離れるとフェードアウトする。
    /// 出始めは、その大きさで描いた絵が届いてからフェードインする。
    pub(crate) fn place_ghosts(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        let scale = self.scale();
        for &id in &self.present {
            let Some((hand, Pointer::Dock)) =
                self.panels[id].pointer.filter(|_| !self.panels[id].closed)
            else {
                continue;
            };
            let h = self.hands.entry(hand).or_default();
            let held = h.dragging.zip(h.magnet).filter(|((t, ..), m)| {
                let (t, o) = (&self.panels[*t], &self.panels[m.owner]);
                !t.closed && !t.hidden && !o.closed && !o.hidden
            });
            if let Some(((t, ..), m)) = held {
                let target = &self.panels[t];
                let slot = m.slot(&self.panels, &self.present, scale, t);
                let rect = target.size - Vec2::splat(2.0 * (face_margin(target) - GHOST_PAD));
                let density = (target.resolution.0 as f32 / target.size.x)
                    .min(GHOST_MAX_PX / rect.max_element());
                let res = (
                    (rect.x * density).round().max(1.0) as u32,
                    (rect.y * density).round().max(1.0) as u32 + DOCK_HINT_PX,
                );
                // 帯の分だけ上に伸ばし（枠の部分がはまる所に重なる）、少し奥へ下げる。
                let up = Vec3::Y * (DOCK_HINT_PX as f32 / density / 2.0);
                let at = slot.transform_point(up - Vec3::Z * GHOST_SINK);
                h.ghost.at = Some((Pose::new(at, slot.orientation), rect.x, res));
                let d = target.pose.position.distance(slot.position);
                h.ghost.near = d < if h.ghost.near { GHOST_HIDE } else { GHOST_SHOW };
                // 窓の大きさは UI 層が解像度に合わせる。
                self.panels[id].resolution = res;
            }
            let p = &mut self.panels[id];
            // 掴み始めは、その大きさで描いた絵が届くまで待つ（[`Ghost::ready`]。届くのは窓を合わせて描き直した後）。
            let (to, motion) = if held.is_some() && h.ghost.ready && h.ghost.near {
                (1.0, Motion::Enter)
            } else {
                (0.0, Motion::Exit)
            };
            h.ghost.shown.go(to, motion, now, true);
            let alpha = h.ghost.shown.at(now).0;
            let Some((pose, width, _)) = h.ghost.at.filter(|_| alpha > 0.0) else {
                p.surface.show(vr, false, None, p.size.x);
                continue;
            };
            p.pose = pose;
            p.surface.show(vr, true, Some(width), width);
            if alpha != h.ghost.alpha {
                h.ghost.alpha = alpha;
                p.surface.present(vr, alpha, 0.0, false);
            }
        }
    }
}

#[cfg(test)]
mod tests;
