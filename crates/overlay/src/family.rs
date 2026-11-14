//! 親子関係（ダイアログ・ポップオーバー・はめる場所）と、ナビゲーションバーの置き場所。

use super::*;

/// 表示だけの子（[`Parent::hud`]）とナビゲーションバーを、親の面からどれだけ手前（m）に置くか。
/// その上を指したカーソル（[`CURSOR_LIFT`]）より奥にする。
const HUD_LIFT: f32 = LIFT_STEP;
/// 親の上端に立てた子を外したとき、離れたと分かるよう面の向き（頭の側）へ動かす距離（m）。
const UNDOCK_PULL: f32 = 0.2;

impl Overlay {
    /// `id` を `parent` の子にする（`None` で独立）。今の位置を保ち、以降は親に追従する。取り付けは外す。
    /// 自分自身・子孫・閉じたパネルは親にできない（何もしない）。
    pub fn set_parent(&mut self, vr: &mut dyn VrBackend, id: PanelId, parent: Option<Parent>) {
        if let Some(p) = parent
            && (p.panel == id
                || self.panels[p.panel].closed
                || ancestors(&self.panels, p.panel).contains(&id))
        {
            return;
        }
        let at = parent.map(|p| self.panels[p.panel].pose);
        let q = &mut self.panels[id];
        // はめる・外すときは、見えていれば滑らかに動かす。外すときは離れたと分かるよう手前へ出す。
        let (was_slot, to_slot) = (
            q.parent.is_some_and(|p| p.slot.is_some()),
            parent.is_some_and(|p| p.slot.is_some()),
        );
        let to = match (was_slot, to_slot) {
            (true, false) if parent.is_none() && !q.hidden => Some(Some(
                q.pose * Pose::new(Vec3::Z * UNDOCK_PULL, Quat::IDENTITY),
            )),
            (false, true) => Some(None),
            _ => None,
        };
        if let Some(to) = to {
            if q.animated && q.surface.has_texture() && !q.hidden {
                self.glides.insert(
                    id,
                    Glide {
                        from: q.pose,
                        to,
                        tween: Animation::new(0.0, 1.0, Motion::Move),
                        last: Instant::now(),
                    },
                );
            } else if let Some(pose) = to {
                q.pose = pose;
            }
        }
        q.parent = parent;
        // 表示だけの子と、ポップオーバー（固定して外側を押すと閉じる子）は、ほかのパネルより前に描く。ランタイムは
        // 奥行きではなく描画順で重ねるので、同じ順だと重なった所がちらつく（はめた画面の上に出すメニューなど）。
        let front = parent.is_some_and(|p| p.hud || (p.docked && p.dismiss));
        q.surface
            .set_sort(vr, if front { HUD_SORT } else { PANEL_SORT });
        if let Some(at) = at {
            q.attached = None;
            q.offset = at.inverse() * q.pose;
        }
    }

    /// 親子の姿勢を合わせる。掴まれている子は、親を連れていくなら親を子に合わせ、そうでなければ親との相対位置を付け直す。
    /// そのあと、掴まれていない子を親（浅い順）に合わせる。親が掴まれても取り付けで動いても、子はここで追従する。
    // ponytail: 親の拡大縮小では子の相対位置を変えない（子はその場の大きさのまま）。
    pub(crate) fn settle_family(&mut self, dragged: &[PanelId]) {
        for &id in dragged {
            let mut cur = id;
            while let Some(Parent {
                panel, carry, slot, ..
            }) = self.panels[cur].parent
            {
                // 掴んでいる間も、立てた子の大きさに合わせて親との位置関係を決め直す（大きさを変えても親が付いてくる）。
                if let Some(tilt) = slot {
                    self.panels[cur].offset = self.slot_offset(cur, panel, tilt);
                }
                if carry {
                    let (pose, offset) = (self.panels[cur].pose, self.panels[cur].offset);
                    self.panels[panel].pose = pose * offset.inverse();
                } else {
                    self.panels[cur].offset =
                        self.panels[panel].pose.inverse() * self.panels[cur].pose;
                    break;
                }
                cur = panel;
            }
        }
        let mut kids: Vec<PanelId> = self
            .present
            .iter()
            .copied()
            .filter(|&i| self.panels[i].parent.is_some() && !dragged.contains(&i))
            .collect();
        kids.sort_by_key(|&i| ancestors(&self.panels, i).len());
        for id in kids {
            if let Some(parent) = self.panels[id].parent {
                if let Some(tilt) = parent.slot {
                    self.panels[id].offset = self.slot_offset(id, parent.panel, tilt);
                } else if parent.hud {
                    // 親の縦横比は配信元に合わせて変わるので、角からの位置を毎 tick 決め直す。
                    let (outer, inner) = (self.panels[parent.panel].size, self.panels[id].size);
                    self.panels[id].offset = Pose::new(
                        Vec3::new(
                            (inner.x - outer.x) / 2.0,
                            (outer.y - inner.y) / 2.0,
                            HUD_LIFT,
                        ),
                        Quat::IDENTITY,
                    );
                }
                self.panels[id].pose = self.panels[parent.panel].pose * self.panels[id].offset;
            }
        }
    }

    /// 親の上端に立てる子（[`Parent::slot`]）の、親から見た姿勢。子のナビゲーションバーの分を空ける。
    fn slot_offset(&self, id: PanelId, parent: PanelId, tilt: f32) -> Pose {
        let rotation = Quat::from_rotation_x(tilt);
        let up = rotation * Vec3::Y;
        let (outer, inner) = (&self.panels[parent], &self.panels[id]);
        let bar = self
            .present
            .iter()
            .find(|&&b| self.panels[b].nav_bar_of == Some(id))
            .map_or(0.0, |&b| self.panels[b].size.y);
        let top = Vec3::Y * (outer.size.y / 2.0 - face_margin(outer));
        // 隙間も UI スケールに合わせ、組を相似に保つ。
        let lift = SLOT_GAP * self.scale() + bar + inner.size.y / 2.0 - face_margin(inner);
        Pose::new(top + up * lift, rotation)
    }

    /// 位置のトランジションを進める（[`Glide`]）。掴まれたら、その場でやめる。
    pub(crate) fn glide(&mut self, dragged: &[PanelId], now: Instant) {
        let panels = &mut self.panels;
        self.glides.retain(|&id, g| {
            let p = &mut panels[id];
            if dragged.contains(&id) {
                return false;
            }
            let t = g
                .tween
                .advance(now.saturating_duration_since(g.last).as_secs_f32());
            g.last = now;
            let to = g.to.unwrap_or(p.pose);
            p.pose = Pose::new(
                g.from.position.lerp(to.position, t),
                g.from.orientation.slerp(to.orientation, t),
            );
            !g.tween.is_done()
        });
    }

    /// ナビゲーションバーを持ち主の下端へ置く。持ち主の地の下端（影の余白 [`PANEL_MARGIN`] の内側）の左右中央を
    /// アンカーに、バーの上端の左右中央を合わせる。頭から見て同じ大きさに見えるよう、アンカーまでの距離に比例して大きくする。
    /// 持ち主の出入りのアニメーション（75%〜100%）で縮んだ下端にも付いていく。
    pub(crate) fn place_nav_bars(&mut self, vr: &mut dyn VrBackend) {
        for &id in &self.present {
            let Some(owner) = self.panels[id]
                .nav_bar_of
                .filter(|_| !self.panels[id].surface.released())
            else {
                continue;
            };
            let o = &self.panels[owner];
            let (opacity, dim) = (o.opacity, o.dim);
            let up = o.pose.orientation * Vec3::Y;
            // 配信中のパネルは影の余白が無い。余白があるものとして内側に付けると、バーが絵に食い込む（face_margin）。
            let margin = face_margin(o) * o.displayed_size().y / o.size.y;
            let anchor = o.pose.position - up * (o.displayed_size().y / 2.0 - margin);
            let rotation = o.pose.orientation;
            let eye = self.hmd.position;
            // 固定した子（ダッシュボードにはめた画面）のバーは、頭からの距離で大きさを変えない（1m 先での大きさのまま。
            // ダッシュボードと一体の部品として、画面との比を保つ）。
            let docked = o.parent.is_some_and(|q| q.docked);
            let leaving = o.hidden || o.closed;
            let p = &mut self.panels[id];
            let size = if leaving {
                p.size
            } else if docked {
                p.nav_bar_size
            } else {
                p.nav_bar_size * anchor.distance(eye)
            };
            let resized = !p.size.abs_diff_eq(size, 1e-4);
            p.size = size;
            // 出入りと暗さは持ち主と一体（バーは閉じても、持ち主が消えきるまで残る）。
            let faded = p.opacity != opacity || p.dim != dim || (p.closed && opacity == 0.0);
            (p.opacity, p.dim, p.dim_to) = (opacity, dim, dim);
            // 上端は持ち主の影の余白と重なる。同じ面だと Z ファイティングするので、表示だけの子と同じだけ手前に浮かせる。
            p.pose = Pose::new(
                anchor - up * (size.y * (0.75 + 0.25 * opacity) / 2.0)
                    + rotation * Vec3::Z * HUD_LIFT,
                rotation,
            );
            if (faded && p.surface.has_texture()) || (p.closed && opacity == 0.0) {
                Self::present(vr, p, false);
            } else if resized {
                p.surface.set_width(vr, p.displayed_size().x);
            }
        }
    }
}

/// 開いている子がいる（暗くして入力を止める）。ナビゲーションバーは持ち主に子がいれば同じ扱い。
pub(crate) fn has_child(panels: &[Panel], present: &[PanelId], id: PanelId) -> bool {
    let id = panels[id].nav_bar_of.unwrap_or(id);
    present
        .iter()
        .map(|&id| &panels[id])
        .any(|q| !q.closed && !q.hidden && q.parent.is_some_and(|p| p.panel == id && p.blocks()))
}

/// `id` の親、その親…（近い順）。
pub(crate) fn ancestors(panels: &[Panel], id: PanelId) -> Vec<PanelId> {
    let mut v = Vec::new();
    let mut cur = id;
    while let Some(p) = panels[cur].parent {
        if v.contains(&p.panel) {
            break;
        }
        v.push(p.panel);
        cur = p.panel;
    }
    v
}

/// `id` を掴んだとき一緒に動く親たち（`carry` でつながっている間）。
pub(crate) fn carried(panels: &[Panel], id: PanelId) -> Vec<PanelId> {
    let mut v = Vec::new();
    let mut cur = id;
    while let Some(Parent {
        panel, carry: true, ..
    }) = panels[cur].parent
    {
        if v.contains(&panel) {
            break;
        }
        v.push(panel);
        cur = panel;
    }
    v
}

#[cfg(test)]
mod tests;
