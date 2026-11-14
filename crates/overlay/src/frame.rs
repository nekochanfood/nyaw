//! 1 フレームの進行（[`Overlay::tick`]）と、ランタイムへの送り出し・表示のアニメーション・暗転の置き場所。

use super::*;

/// 空間全体の暗転の濃さ（黒の不透明度）。周りがうっすら見える程度に落とす。
// ponytail: 固定の濃さ。暗すぎる・明るすぎると感じたらここを調整する。
const VEIL: f32 = 0.5;
/// 暗転の幕は黒い正方形で、毎 tick 頭の正面この距離（m）に置き直す。描画順だけでなく奥行きでもパネルより奥にする
/// （SteamVR は頭に相対配置したオーバーレイを描画順に関係なく手前に合成し、パネルが幕の向こうで消えた）。
// ponytail: 固定の距離。これより遠いパネルは幕の手前に出ない。
const VEIL_DISTANCE: f32 = 50.0;
/// 幕の一辺（m）。正面から atan(2500 / 50) ≈ 89° まで覆う。
const VEIL_SIZE: f32 = 5000.0;

/// 1 tick の経過の下限・上限（秒）。テストのように続けて tick しても 1 フレーム分は進め、止まった後に飛ばない。
pub(crate) const TICK_MIN: f32 = 1.0 / 120.0;

pub(crate) const TICK_MAX: f32 = 1.0 / 30.0;

impl Overlay {
    /// `panel` の描画済み UI フレームを VR ランタイムへ送る。
    /// テクスチャの大きさは `resolution`（ヒット判定用の論理ピクセル）と独立してよい。
    /// HiDPI では UI が `resolution × 拡大率` で描かれるため。
    pub fn submit(
        &mut self,
        vr: &mut dyn VrBackend,
        panel: PanelId,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "invalid RGBA buffer size"
        );
        let p = &mut self.panels[panel];
        if p.closed {
            return;
        }
        // ナビゲーションバーは持ち主の出入りに乗る（place_nav_bars）。
        let entering = !p.surface.has_texture() && p.animated && p.nav_bar_of.is_none();
        // 隠したまま最初の描画が届いたら、出し直すまで見せない。
        if !p.surface.has_texture() && p.hidden {
            p.opacity = 0.0;
        }
        if entering {
            p.opacity = 0.0;
        }
        p.surface.store(width, height, rgba);
        Self::present(vr, p, true);
        if entering && !p.hidden {
            // 初回の GPU 転送が終わってから時計を始める。
            self.animations.insert(
                panel,
                (Animation::new(0.0, 1.0, Motion::Enter), Instant::now()),
            );
        }
    }

    pub(crate) fn present(vr: &mut dyn VrBackend, p: &mut Panel, upload: bool) {
        if p.surface.released() {
            return;
        }
        if p.pointer.is_none() {
            p.surface.set_width(vr, p.displayed_size().x);
        }
        if p.closed && p.opacity == 0.0 {
            p.surface.release(vr);
        } else {
            p.surface.present(vr, p.opacity, p.dim, upload);
        }
    }

    pub(crate) fn animate(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        self.animations.retain(|id, (tween, last)| {
            let v = tween.advance((now - *last).as_secs_f32());
            *last = now;
            let p = &mut self.panels[*id];
            p.opacity = v;
            Self::present(vr, p, false);
            !tween.is_done()
        });
        self.dims.retain(|id, (tween, last)| {
            let v = tween.advance((now - *last).as_secs_f32());
            *last = now;
            let p = &mut self.panels[*id];
            p.dim = v;
            Self::present(vr, p, false);
            !tween.is_done()
        });
    }

    pub fn tick<B: VrBackend>(&mut self, subsystems: &mut Subsystems<B>) -> Vec<UiEvent> {
        let frame = subsystems.poll();
        self.tick_frame(&mut subsystems.backend, frame)
    }

    /// [`Self::tick`] の poll の後半（ランタイムの待ちとオーバーレイの処理を別々に測るため分けてある）。
    pub fn tick_frame(&mut self, vr: &mut dyn VrBackend, frame: Frame) -> Vec<UiEvent> {
        let now = Instant::now();
        self.animate(vr, now);
        self.present
            .retain(|&id| !self.panels[id].surface.released());
        self.dt = self
            .last_tick
            .map_or(TICK_MIN, |t| (now - t).as_secs_f32())
            .clamp(TICK_MIN, TICK_MAX);
        self.last_tick = Some(now);
        self.quit = frame.quit;
        self.hmd = frame.hmd;
        self.steadied = frame.stabilize_pointer && self.pointer_smoothing() > 0.0;
        self.trackers = frame.trackers.iter().map(|(id, _)| id.clone()).collect();
        self.skeletons.clone_from(&frame.hands);
        self.controllers.clone_from(&frame.controllers);
        // 掴み中のパネルは手に従う（離したフレームで update_hand がオフセットを付け直す）。
        // 取り付け先が見失われたフレームは最後の姿勢のまま。
        let mut dragged: Vec<PanelId> = self
            .hands
            .values()
            .filter_map(|h| h.dragging)
            .map(|d| d.0)
            .collect();
        // 子を掴んで親を連れていくときは、親も手に従う。
        for d in dragged.clone() {
            dragged.extend(carried(&self.panels, d));
        }
        let dt = self.dt;
        for &id in &self.present {
            let p = &mut self.panels[id];
            p.follow = None;
            // 子は取り付けではなく親に追従する（下の settle_family）。
            if dragged.contains(&id) || p.parent.is_some() || p.nav_bar_of.is_some() {
                continue;
            }
            // 頭にゆるく追従するパネルは、取り付けではなく頭の向きと位置から置く（ランタイムには任せず毎 tick 置く）。
            if let Some(lazy) = p.lazy_follow.as_mut() {
                p.pose = lazy.step(&frame.hmd, dt);
                continue;
            }
            // 骨格で操作している手も、ランタイムがその手のデバイスを追っていれば相対配置を任せる（`Frame::pose` も同じ姿勢）。
            // 無ければ `set_overlay_pose_relative` が断るので、骨格から求めた姿勢で置く。
            p.follow = p.attached.clone();
            if let Some(pose) = p
                .attached
                .as_ref()
                .and_then(|(d, o)| Some(frame.pose(d)? * *o))
            {
                p.pose = pose;
            }
        }
        let mut events = Vec::new();
        for h in self.hands.values_mut() {
            h.ray = None;
        }
        for c in &frame.controllers {
            self.update_hand(c, &frame, &mut events);
            if let Some((id, ..)) = self.hands[&c.hand].dragging
                && self.hands[&c.hand].scaling.is_some()
            {
                let p = &self.panels[id];
                p.surface.set_width(vr, p.displayed_size().x);
            }
        }
        // 見えなくなった手（電源が切れた・novr のマウスが画面の外へ出た）は、指していたパネルから離れる。
        for (hand, st) in &mut self.hands {
            if frame.controllers.iter().any(|c| c.hand == *hand) {
                continue;
            }
            if let Some(panel) = st.hover.take() {
                events.push(UiEvent {
                    panel,
                    hand: *hand,
                    event: PointerEvent::Leave,
                });
            }
            st.select = false;
        }
        for id in std::mem::take(&mut self.dismissed) {
            self.close(vr, id);
        }
        self.settle_family(&dragged);
        self.glide(&dragged, now);
        let mut parents = std::collections::HashSet::new();
        for &id in &self.present {
            let p = &self.panels[id];
            if !p.closed
                && !p.hidden
                && let Some(parent) = p.parent.filter(Parent::blocks)
            {
                parents.insert(parent.panel);
            }
        }
        let now = Instant::now();
        for &id in &self.present {
            let p = &mut self.panels[id];
            if p.closed {
                continue;
            }
            let parent = parents.contains(&p.nav_bar_of.unwrap_or(id));
            let to = if parent { DIM } else { 0.0 };
            // ナビゲーションバーの暗さは持ち主を写す（place_nav_bars）。
            if p.dim_to == to || p.nav_bar_of.is_some() {
                continue;
            }
            p.dim_to = to;
            if p.animated && !p.closed {
                // 戻るときも ease-out: 終わり際に急に明るくなると、子が消える瞬間と重なってちらつく。
                let tween = Animation::new(
                    p.dim,
                    to,
                    if parent {
                        Motion::Enter
                    } else {
                        Motion::Return
                    },
                );
                self.dims.insert(id, (tween, now));
            } else {
                self.dims.remove(&id);
                p.dim = to;
                Self::present(vr, p, false);
            }
        }
        self.place_nav_bars(vr);
        self.place_pointers(vr);
        for &id in &self.present {
            let p = &mut self.panels[id];
            p.surface.place(vr, p.pose, p.follow.as_ref());
        }
        self.place_veil(vr, now);
        events
    }

    /// 暗転を求めるパネルが出ている間だけ幕を出し、頭の正面に置く。
    fn place_veil(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        // パネルのフェードインと一緒に暗くするので、最初の描画が届くまで待つ。
        let on = self.present.iter().any(|&id| {
            let p = &self.panels[id];
            p.veil && !p.closed && !p.hidden && p.surface.has_texture()
        });
        let to = if on { VEIL } else { 0.0 };
        // 暗転している間（ダッシュボードが出ている間）は、ゲームへの入力も止める（設定で通せる）。
        let block = on && !self.pass_input;
        let v = &mut self.veil;
        if v.blocked != block {
            v.blocked = block;
            vr.block_app_input(block);
        }
        if v.to != to {
            v.to = to;
            if on && v.surface.is_none() {
                match Surface::new(vr, "nyaw.veil", VEIL_SIZE, self.hmd) {
                    Ok(mut s) => {
                        s.set_sort(vr, VEIL_SORT);
                        s.store(1, 1, &[0, 0, 0, 255]);
                        s.present(vr, 0.0, 0.0, true);
                        v.surface = Some(s);
                    }
                    Err(e) => logger::warn!("暗転の幕を作れない", error = ?e),
                }
            }
            let motion = if on { Motion::Enter } else { Motion::Exit };
            v.fade = Some((Animation::new(v.opacity, to, motion), now));
        }
        let Some(surface) = &mut v.surface else {
            return;
        };
        if let Some((tween, last)) = &mut v.fade {
            v.opacity = tween.advance((now - *last).as_secs_f32());
            *last = now;
            if tween.is_done() {
                v.fade = None;
            }
            surface.show(vr, v.opacity > 0.0, None, VEIL_SIZE);
            surface.present(vr, v.opacity, 0.0, false);
        }
        if v.opacity > 0.0 {
            let offset = Pose::new(Vec3::new(0.0, 0.0, -VEIL_DISTANCE), Quat::IDENTITY);
            surface.place(vr, self.hmd * offset, None);
        }
    }
}

#[cfg(test)]
mod tests;
