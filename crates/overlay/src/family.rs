//! 親子関係（ダイアログ・ポップオーバー・ドックスロット）とナビバーの配置。

use super::*;

/// 表示専用子（[`Parent::hud`]）とナビバーを親面からどれだけ手前（m）に配置するか。
/// その上を指したカーソル（[`CURSOR_LIFT`]）より奥にする。
const HUD_LIFT: f32 = LIFT_STEP;
/// 面に重ねる子（[`Parent::over`]）を親面からどれだけ手前（m）に置くか。ナビバー・表示専用子（[`HUD_LIFT`]）より手前にし、
/// 同じ親に重ねる子どうしも、後から開いたものほど [`LIFT_STEP`] ずつ手前にする（描画順 [`OVER_SORT`] と同じ並び）。
const OVER_LIFT: f32 = HUD_LIFT + LIFT_STEP;
/// スロット子をアンドックしたとき、離脱を明示するために面の法線方向（頭側）へ移動する距離（m）。
const UNDOCK_PULL: f32 = 0.2;
/// スタック子（[`Parent::stack`]）が変化した配置先へ追従する時定数（秒）。
const STACK_FOLLOW: f32 = 0.12;
/// 持ち主もバーも非ポイント状態になってからナビバー（縮んだ線）がフェードアウト開始するまでの待機時間。
// ponytail: 固定値。早すぎる・遅すぎればここを調整する。
const NAV_BAR_LINGER: std::time::Duration = std::time::Duration::from_millis(1500);

impl Overlay {
    /// `id` を `parent` の子にする（`None` で独立）。現在位置を保持し以降は親に追従する。取り付けは解除される。
    /// 自分自身・子孫・閉じたパネルは親に指定不可（何もしない）。
    pub fn set_parent(&mut self, vr: &mut dyn VrBackend, id: PanelId, parent: Option<Parent>) {
        if let Some(p) = parent
            && (p.panel == id
                || self.panels[p.panel].closed
                || ancestors(&self.panels, p.panel).contains(&id))
        {
            return;
        }
        let at = parent.map(|p| self.panels[p.panel].pose);
        // ドック・アンドック時は可視ならトランジションで移動する。どちらも少し行き過ぎてから収まり（[`Motion::Pop`]）、
        // アンドック時は離脱を示すため手前へ飛び出す。グラブ中は手に追従するため移動しない。
        let held = self.grabbed(id);
        let q = &self.panels[id];
        let (was_slot, to_slot) = (
            q.parent.is_some_and(|p| p.slot.is_some()),
            parent.is_some_and(|p| p.slot.is_some()),
        );
        let to = match (was_slot, to_slot) {
            _ if held => None,
            (true, false) if parent.is_none() && !q.hidden => Some((
                Some(q.pose * Pose::new(Vec3::Z * UNDOCK_PULL, Quat::IDENTITY)),
                Motion::Pop,
            )),
            (false, true) => Some((None, Motion::Pop)),
            _ => None,
        };
        if let Some((to, motion)) = to {
            self.glide_to(id, to, motion);
        }
        let q = &mut self.panels[id];
        q.parent = parent;
        // 表示専用子・ポップオーバー（固定して外側プレスで閉じる子）・スタック子は他パネルより前に描画する。ランタイムは
        // 深度ではなく描画順で合成するため、同一順だと重なり部分がフリッカーする（ドックパネル上のメニューや、ドックパネルの
        // 出入りで上に移動するスタック子など）。面に重ねる子の描画順は cover が決める。
        let front = parent.is_some_and(|p| p.hud || p.stack || (p.docked && p.dismiss));
        q.surface
            .set_sort(vr, if front { HUD_SORT } else { PANEL_SORT });
        if let Some(at) = at {
            q.attached = None;
            q.offset = at.inverse() * q.pose;
        }
        self.cover(vr, id);
    }

    /// 親の面の一部に重ねる子（[`Parent::over`]）の面をその範囲に合わせ、親の前に置く。同じ親に重ねる子のうち、
    /// 先に開いた（PanelId の小さい）ものの数だけ、描画順を上げて手前に出す。
    fn cover(&mut self, vr: &mut dyn VrBackend, id: PanelId) {
        let Some(Parent {
            panel,
            over: Some([x, y, w, h]),
            ..
        }) = self.panels[id].parent
        else {
            return;
        };
        let below = self
            .present
            .iter()
            .filter(|&&c| {
                let q = &self.panels[c];
                c < id
                    && !q.closed
                    && q.parent
                        .is_some_and(|q| q.panel == panel && q.over.is_some())
            })
            .count() as u32;
        let layer = below.min(OVER_LAYERS - 1);
        let outer = &self.panels[panel];
        let face = outer.size - Vec2::splat(2.0 * face_margin(outer));
        let offset = Pose::new(
            Vec3::new(
                (x + w / 2.0 - 0.5) * face.x,
                (0.5 - y - h / 2.0) * face.y,
                OVER_LIFT + LIFT_STEP * layer as f32,
            ),
            Quat::IDENTITY,
        );
        let pose = outer.pose * offset;
        let p = &mut self.panels[id];
        // 影の余白（面の外）の分だけ広げる。余白は画素で決まるので、面の画素数との比で広げる。
        let (res, pad) = (
            Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32),
            2.0 * margin_px(p),
        );
        let size = face * Vec2::new(w, h) * res / (res - Vec2::splat(pad)).max(Vec2::ONE);
        (p.offset, p.pose) = (offset, pose);
        if !p.size.abs_diff_eq(size, 1e-6) {
            p.size = size;
            p.surface.set_width(vr, p.displayed_size().x);
        }
        let sort = OVER_SORT + layer;
        if p.surface.sort() != sort {
            p.surface.set_sort(vr, sort);
        }
    }

    /// 親子の姿勢を同期する。グラブ中の子は carry なら親を子に合わせ、そうでなければ親との相対位置を再計算する。
    /// その後、非グラブの子を親（浅い順）に合わせる。親がグラブされてもデバイス追従で動いても子はここで追従する。
    // ponytail: 親のリサイズでは子の相対位置を変えない（子はそのサイズのまま）。
    /// 磁石で引いている子（`pulled`。[`Magnet`]）は親を連れず、親との位置関係も変更しない。
    pub(crate) fn settle_family(
        &mut self,
        vr: &mut dyn VrBackend,
        dragged: &[PanelId],
        pulled: &[PanelId],
    ) {
        for &id in dragged.iter().filter(|id| !pulled.contains(id)) {
            let mut cur = id;
            while let Some(Parent {
                panel, carry, slot, ..
            }) = self.panels[cur].parent
            {
                // グラブ中もスロット子のサイズに合わせて親との位置関係を再計算（リサイズしても親が追従する）。
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
                    let to = self.slot_offset(id, parent.panel, tilt);
                    let p = &self.panels[id];
                    // スタック子は配置先が出入りしてもジャンプせず追従する。フェードイン完了まではそのまま合わせる
                    // （再表示時に前の位置からスライドしてくるのを防止）。
                    self.panels[id].offset =
                        if parent.stack && p.animated && !p.hidden && p.opacity >= 1.0 {
                            let k = 1.0 - (-self.dt / STACK_FOLLOW).exp();
                            Pose::new(
                                p.offset.position.lerp(to.position, k),
                                p.offset.orientation.slerp(to.orientation, k),
                            )
                        } else {
                            to
                        };
                } else if parent.hud {
                    // 親のアスペクト比はキャプチャ元に合わせて変わるため、角からの位置を毎 tick 再計算する。
                    let (outer, inner) = (self.panels[parent.panel].size, self.panels[id].size);
                    self.panels[id].offset = Pose::new(
                        Vec3::new(
                            (inner.x - outer.x) / 2.0,
                            (outer.y - inner.y) / 2.0,
                            HUD_LIFT,
                        ),
                        Quat::IDENTITY,
                    );
                } else if parent.over.is_some() {
                    self.cover(vr, id);
                }
                self.panels[id].pose = self.panels[parent.panel].pose * self.panels[id].offset;
            }
        }
    }

    /// 親・持ち主に追従するもの（子・ナビバー）は、ルートパネルがデバイス追従中（`follow`）なら
    /// 同じデバイスへの相対配置にする。ルートはランタイムが合成直前の姿勢で移動するため、毎 tick の姿勢で配置すると遅延が見える。
    pub(crate) fn follow_roots(&mut self) {
        let follows: Vec<_> = self
            .present
            .iter()
            .filter_map(|&id| {
                let p = &self.panels[id];
                let r = root(&self.panels, id);
                let (device, offset) = self.panels[r].follow.clone().filter(|_| r != id)?;
                let r = &self.panels[r];
                Some((id, (device, offset * r.pose.inverse() * p.pose)))
            })
            .collect();
        for (id, follow) in follows {
            self.panels[id].follow = Some(follow);
        }
    }

    /// スロット子（[`Parent::slot`]）の親から見た姿勢（[`slot_offset_in`]）。
    fn slot_offset(&self, id: PanelId, parent: PanelId, tilt: f32) -> Pose {
        slot_offset_in(&self.panels, &self.present, self.scale(), id, parent, tilt)
    }

    /// `id` を現在位置から `to`（`None` なら毎 tick 算出のドック位置）へ `motion` で移動する（[`Glide`]）。非表示ならその場で移動。
    pub(crate) fn glide_to(&mut self, id: PanelId, to: Option<Pose>, motion: Motion) {
        let q = &mut self.panels[id];
        if q.animated && q.surface.has_texture() && !q.hidden {
            let tween = Animation::new(0.0, 1.0, motion);
            let (from, last) = (q.pose, Instant::now());
            self.glides.insert(
                id,
                Glide {
                    from,
                    to,
                    tween,
                    last,
                },
            );
        } else if let Some(pose) = to {
            q.pose = pose;
        }
    }

    /// 位置トランジションを進行させる（[`Glide`]）。グラブされたら即座に中止する。
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
            // オーバーシュートする動き（[`Motion::Pop`]）でも向きはオーバーシュートさせない。
            p.pose = Pose::new(
                g.from.position.lerp(to.position, t),
                g.from.orientation.slerp(to.orientation, t.clamp(0.0, 1.0)),
            );
            !g.tween.is_done()
        });
    }

    /// ナビバーを持ち主の下端へ配置する。持ち主面の下端（シャドウマージン [`PANEL_MARGIN`] の内側）の左右中央を
    /// アンカーに、バーの上端の左右中央を合わせる。頭から同じ見かけサイズになるようアンカーまでの距離に比例して拡大する。
    /// 持ち主の出入りアニメーション（75%〜100%）で縮んだ下端にも追従する。
    /// 持ち主もバーも非ポイント状態で [`NAV_BAR_LINGER`] 経過するとフェードアウトし（[`Motion::Fade`]）、完了後に
    /// ランタイムのオーバーレイを解放する（[`Surface::park`]。パネル・ビュー・最終画像は保持）。
    /// ポイントされたら再生成して出現する（[`Motion::Enter`]）。
    pub(crate) fn place_nav_bars(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        for &id in &self.present {
            let Some(owner) = self.panels[id]
                .nav_bar_of
                .filter(|_| !self.panels[id].surface.released())
            else {
                continue;
            };
            let pointed = self.pointed(id);
            let o = &self.panels[owner];
            let (opacity, dim) = (o.opacity, o.dim);
            let up = o.pose.orientation * Vec3::Y;
            // キャプチャ中のパネルはシャドウマージンなし。マージンありとして内側に配置するとバーが描画に食い込む（face_margin）。
            // 角を掴んで大きさを変えている間は、代わりの板の下端に付ける。
            let k = o.displayed_size().y / o.size.y;
            let height = self.stretched(owner).map_or(o.size.y, |s| s.y) * k;
            let anchor = o.pose.position - up * (height / 2.0 - face_margin(o) * k);
            let rotation = o.pose.orientation;
            let eye = self.hmd.position;
            // ドック子（ダッシュボードにはめた画面）のバーは頭からの距離でサイズ変更しない（1m 先でのサイズ固定。
            // ダッシュボードと一体の部品として画面との比率を維持する）。
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
            // 非ポイント状態が続くとフェードアウトして消える。ポイントされると出現する。
            if pointed {
                p.pointed_at = now;
            }
            let asleep = now.saturating_duration_since(p.pointed_at) >= NAV_BAR_LINGER;
            let to = if asleep { 0.0 } else { 1.0 };
            let woke = p.wake.target() != to && !asleep;
            let motion = if asleep { Motion::Fade } else { Motion::Enter };
            p.wake.go(to, motion, now, p.animated);
            let (awake, _) = p.wake.at(now);
            // 出入りと暗転度は持ち主と同期（バーは close されても持ち主が消えきるまで残存する）。
            let faded = p.opacity != opacity
                || p.dim != dim
                || p.awake != awake
                || (p.closed && opacity == 0.0);
            (p.opacity, p.dim, p.dim_to, p.awake) = (opacity, dim, dim, awake);
            // 上端は持ち主のシャドウマージンと重なる。同一面だと Z ファイティングするため表示専用子と同じ分だけ手前に浮かせる。
            p.pose = Pose::new(
                anchor - up * (size.y * (0.75 + 0.25 * opacity) / 2.0)
                    + rotation * Vec3::Z * HUD_LIFT,
                rotation,
            );
            if woke
                && let Err(e) =
                    p.surface
                        .restore(vr, p.displayed_size().x, p.pose, opacity * awake, dim)
            {
                logger::warn!("ナビバーのオーバーレイを再生成できない", error = ?e, panel = id);
            }
            if (faded && p.surface.has_texture()) || (p.closed && opacity == 0.0) {
                Self::present(vr, p, false);
            } else if resized {
                p.surface.set_width(vr, p.displayed_size().x);
            }
            if asleep && awake == 0.0 {
                p.surface.park(vr);
            }
        }
    }
}

/// 親 `parent` の上端に立てる子 `id`（[`Parent::slot`]）の、親から見た姿勢。子のナビゲーションバーの分を空ける。
/// 隙間は UI スケール `scale` に合わせる。積む子（[`Parent::stack`]）は、ほかに立てた子があればその上端の上に置く。
pub(crate) fn slot_offset_in(
    panels: &[Panel],
    present: &[PanelId],
    scale: f32,
    id: PanelId,
    parent: PanelId,
    tilt: f32,
) -> Pose {
    let rotation = Quat::from_rotation_x(tilt);
    let up = rotation * Vec3::Y;
    let (outer, inner) = (&panels[parent], &panels[id]);
    // ナビゲーションバーは、はまっている間の大きさ（頭からの距離で変わらない）で空ける。外れて動かしている間も
    // はまる所が動かないように（外れたバーは距離で大きさが変わる）。
    let bar = present
        .iter()
        .find(|&&b| panels[b].nav_bar_of == Some(id))
        .map_or(0.0, |&b| panels[b].nav_bar_size.y);
    // 隙間も UI スケールに合わせ、組を相似に保つ。
    let lift = SLOT_GAP * scale + bar + inner.size.y / 2.0 - face_margin(inner);
    let below = inner.parent.filter(|p| p.stack).and_then(|_| {
        present.iter().copied().find(|&c| {
            let q = &panels[c];
            c != id
                && !q.closed
                && !q.hidden
                && q.parent
                    .is_some_and(|q| q.panel == parent && q.slot.is_some() && !q.stack)
        })
    });
    if let Some((below, tilt)) = below.and_then(|b| Some((b, panels[b].parent?.slot?))) {
        let under = slot_offset_in(panels, present, scale, below, parent, tilt);
        let b = &panels[below];
        let top = under.position + under.orientation * Vec3::Y * (b.size.y / 2.0 - face_margin(b));
        return Pose::new(top + up * lift, rotation);
    }
    let top = Vec3::Y * (outer.size.y / 2.0 - face_margin(outer));
    Pose::new(top + up * lift, rotation)
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
/// 親・持ち主（ナビゲーションバーの）をたどった根元。親も持ち主も無ければ `id` 自身。
pub(crate) fn root(panels: &[Panel], id: PanelId) -> PanelId {
    let up = |q: &Panel| q.nav_bar_of.or(q.parent.map(|r| r.panel));
    std::iter::successors(Some(id), |&r| up(&panels[r]))
        .take(panels.len())
        .last()
        .unwrap_or(id)
}

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

/// 不透明な画像 `rgba`（`width`×`height`）の周りに、パネルと同じ影（ずらさず四方へ広がる。ぼかしの σ
/// [`PANEL_SHADOW_BLUR`]、濃さ [`PANEL_SHADOW_ALPHA`]）を描いた余白（[`PANEL_MARGIN`]）を足した画像と、その大きさ。
/// GPUI で描かない画像（配信しているウィンドウのメニュー）を、面に重ねる子（[`Parent::over`]）として出すときに使う。
pub fn with_shadow(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let pad = PANEL_MARGIN as u32;
    let (w, h) = (width + 2 * pad, height + 2 * pad);
    // 長方形の影は縦と横に分けて求められる。軸ごとに、面の範囲をガウスでぼかしたときに掛かる割合を求め、掛け合わせる。
    let r = pad as i64;
    let kernel: Vec<f32> = (-r..=r)
        .map(|k| (-((k * k) as f32) / (2.0 * PANEL_SHADOW_BLUR * PANEL_SHADOW_BLUR)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    let blur = |len: u32, inside: u32| -> Vec<f32> {
        let face = r..r + inside as i64;
        (0..len as i64)
            .map(|i| {
                (-r..=r)
                    .zip(&kernel)
                    .filter(|(k, _)| face.contains(&(i + k)))
                    .map(|(_, g)| g)
                    .sum::<f32>()
                    / total
            })
            .collect()
    };
    let (across, down) = (blur(w, width), blur(h, height));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (ix, iy) = (x.wrapping_sub(pad), y.wrapping_sub(pad));
            if ix < width && iy < height {
                let i = ((iy * width + ix) * 4) as usize;
                out.extend_from_slice(&rgba[i..i + 4]);
            } else {
                // 黒なので、事前乗算アルファでもそのままの値でも同じ。
                let a = PANEL_SHADOW_ALPHA * across[x as usize] * down[y as usize];
                out.extend_from_slice(&[0, 0, 0, (a * 255.0).round() as u8]);
            }
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests;
