//! 手ごとの入力: レーザーのヒット判定とポインタイベント、手ブレ抑え、パネルを掴んで動かす・大きさを変える。

use super::*;

/// トリガーを引き切る直前（押下になる直前）の手ブレ抑え。引き始めからこれへ向けて強める。
/// 押下と判定される前の、トリガーを引く動きでクリック位置がずれないように（押した後は [`CLICK_SLOP`]）。
const TRIGGER_STEADY: f32 = 0.97;
/// 掴み中のスティックを倒し切ったとき、距離が変わる速さ（m/s。距離によらず一定）と、効き始める倒し量。
const PUSH: f32 = 2.0;
const PUSH_DEADZONE: f32 = 0.2;
/// 掴んだパネルを手から離せる距離（m）。
const GRAB_MIN: f32 = 0.15;
const GRAB_MAX: f32 = 10.0;
/// 掴み中にトリガーを押しながら手を 1m 前へ出したときの拡大率の指数（e^SCALE_RATE 倍）。後ろへ引くと縮む。
const SCALE_RATE: f32 = 3.0;
/// パネルの幅の範囲（m）。
/// 掴み中の拡大縮小の入力（`ControllerState::resize`）を倒し切ったときの、1 tick あたりの拡大率の指数。
// ponytail: tick 単位なので速さはフレームレート次第（90Hz で毎秒約 2.5 倍）。テスト用の入力なので時間では割らない。
const RESIZE_STEP: f32 = 0.01;
const SCALE_MIN: f32 = 0.05;
const SCALE_MAX: f32 = 10.0;

impl Overlay {
    pub(crate) fn update_hand(
        &mut self,
        c: &ControllerState,
        frame: &Frame,
        out: &mut Vec<UiEvent>,
    ) {
        // 手ブレのない入力は補正すると遅れるため、サブシステムが示す能力に従う。
        let base = if !frame.stabilize_pointer {
            0.0
        } else {
            self.pointer_smoothing()
        };
        let release = self.pointer_release().to_radians();
        let st = self.hands.entry(c.hand).or_default();
        // 手ブレを抑えたレーザー。隠れていた後は追わずに手に合わせる。
        let aim = Pose::new(
            c.aim.position,
            st.drawn.map_or(c.aim.orientation, |q| {
                let s = steadiness(base, q.angle_between(c.aim.orientation), release, c);
                q.slerp(c.aim.orientation, 1.0 - s)
            }),
        );
        let grab_started = c.grab && !st.grab_held;
        let press = c.select && !st.pressed;
        st.pressed = c.select;
        if !c.select {
            st.swallow = false;
        }
        st.grab_held = c.grab;
        st.menu = c.menu;

        if let Some((id, offset, len, pivot, resize)) = st.dragging {
            if c.grab {
                // スティックの上下で、掴んだ点をレーザーに沿って遠く・近くへ。
                let push = if c.scroll.y.abs() < PUSH_DEADZONE {
                    0.0
                } else {
                    c.scroll.y
                };
                let mut new_len = len + PUSH * push * self.dt;
                // トリガーを押しながら手を前後に動かすと拡大縮小。パネルは手に付いて動かず、その場に留める。
                let p = &mut self.panels[id];
                let mut width = p.size.x;
                if c.select && resize {
                    if let Some(last) = st.scaling {
                        let delta = (c.aim.position - last).dot(aim.forward());
                        width *= (delta * SCALE_RATE).exp();
                        new_len -= delta;
                    }
                    st.scaling = Some(c.aim.position);
                } else {
                    st.scaling = None;
                }
                // 拡大縮小の入力（novr の PageUp / PageDown）。その場で大きさだけ変える。
                if c.resize != 0.0 && resize {
                    width *= (c.resize * RESIZE_STEP).exp();
                }
                let (mut offset, mut pivot) = (offset, pivot);
                let width = width.clamp(SCALE_MIN, SCALE_MAX);
                if width != p.size.x {
                    // 基準は中身の決まった点（キーボードのホームポジション）なので、大きさに合わせて動かす。
                    if let Some(anchor) = &mut p.grab_anchor {
                        let before = anchor.inverse();
                        anchor.position *= width / p.size.x;
                        if pivot == before {
                            pivot = anchor.inverse();
                        }
                    }
                    let bottom = p.size.y / 2.0 - face_margin(p);
                    p.size *= width / p.size.x;
                    // 親の上端に立てた画面は、親に接する下端を動かさずに大きさを変える（中心を基準にすると、離した時に跳ねる）。
                    if p.parent.is_some_and(|q| q.slot.is_some()) {
                        let lift = p.pose.orientation
                            * Vec3::Y
                            * (p.size.y / 2.0 - face_margin(p) - bottom);
                        p.pose.position += lift;
                        offset += aim.orientation.inverse() * lift;
                    }
                }
                let new_len = new_len.clamp(GRAB_MIN, GRAB_MAX);
                let offset = offset + Vec3::NEG_Z * (new_len - len);
                st.dragging = Some((id, offset, new_len, pivot, resize));
                // 手に付いて動き、頭の方を向く。頭を傾けてもパネルは傾けない（水平にスナップ）が、
                // 寝転がっている間は首の傾きに付いてくる（`Pose::panel_up`）。目標へはなめらかに追いつく。
                let at = aim.transform_point(offset);
                let reference = p.pose * pivot.inverse();
                let target = Pose::new(
                    at,
                    facing(at, frame.hmd.position, frame.hmd.panel_up())
                        .unwrap_or(reference.orientation),
                ) * pivot;
                p.pose = approach(p.pose, target, self.dt);
                // 取り付け先から届く範囲の外へは出さない。
                if let (Some(reach), Some((device, _))) = (p.reach, &p.attached)
                    && let Some(dp) = frame.pose(device)
                {
                    let from = p.pose.position - dp.position;
                    p.pose.position = dp.position + from.clamp_length_max(reach);
                }
                st.ray = Some(Ray {
                    hand: c.aim,
                    aim,
                    len: new_len,
                    hit: None,
                });
                st.drawn = Some(aim.orientation);
                return;
            }
            st.dragging = None;
            st.scaling = None;
            // 取り付け中なら、離した位置を新しいオフセットにする。
            for q in std::iter::once(id).chain(carried(&self.panels, id)) {
                let p = &mut self.panels[q];
                if let Some((device, offset)) = &mut p.attached
                    && let Some(dp) = frame.pose(device)
                {
                    *offset = dp.inverse() * p.pose;
                    if let Some(reach) = p.reach {
                        offset.position = offset.position.clamp_length_max(reach);
                    }
                }
            }
        }

        // キャプチャが有効なら、押している間は押したパネルの面を指し続ける（はみ出しても離すまでそのパネルへ送る）。
        // 離したフレームも押していたパネルへ送り、Up を届ける。ドラッグ＆ドロップでほかのパネルへ運ぶときは外す（release_capture）。
        let captured = st
            .hover
            .filter(|&id| {
                (self.capture || self.panels[id].capture)
                    && !st.free
                    && st.select
                    && !self.panels[id].closed
                    && !self.panels[id].hidden
            })
            .and_then(|id| plane_hit(&self.panels[id], &aim).map(|(pos, t)| (id, pos, t)));
        // そうでなければ、レーザー上で最も近いパネル。
        let hit = captured.or_else(|| {
            self.present
                .iter()
                .map(|&id| (id, &self.panels[id]))
                .filter(|(_, p)| {
                    !p.closed
                        && !p.hidden
                        && p.pointer.is_none()
                        && !p.parent.is_some_and(|q| q.hud)
                        && !p.nav_bar_of.is_some_and(|o| self.panels[o].hidden)
                })
                .filter_map(|(id, p)| hit_test(p, &aim).map(|(pos, t)| (id, pos, t)))
                .min_by(|a, b| a.2.total_cmp(&b.2))
        });
        // 押した瞬間、そこが子の外側ならその子を閉じる（`Parent::dismiss`）。
        if press {
            let on = hit.map(|h| h.0);
            for &i in &self.present {
                let q = &self.panels[i];
                let inside = on.is_some_and(|x| x == i || ancestors(&self.panels, x).contains(&i));
                if !q.closed && !q.hidden && !inside && q.parent.is_some_and(|p| p.dismiss) {
                    self.dismissed.push(i);
                    st.swallow = true;
                }
            }
        }
        // 何も指していなければレーザーは出さない。
        st.ray = hit.map(|(id, _, t)| Ray {
            hand: c.aim,
            aim,
            len: t,
            hit: Some((id, aim.position + aim.forward() * t)),
        });
        st.drawn = st.ray.map(|r| r.aim.orientation);
        // ロック中・ゆるく追従中のパネルは掴めない（掴みボタンは無視して普通に指す）。
        let grab_started = grab_started
            && hit.is_some_and(|h| {
                std::iter::once(self.panels[h.0].nav_bar_of.unwrap_or(h.0))
                    .chain(carried(&self.panels, h.0))
                    .all(|q| self.panels[q].grabbable())
            });

        let mut emit = |panel, event| {
            out.push(UiEvent {
                panel,
                hand: c.hand,
                event,
            })
        };
        // 子が開いている親（とそのナビゲーションバー）は入力を受けない（掴むことはできる）。HUD の子は塞がない。
        let blocked = |id| has_child(&self.panels, &self.present, id);
        let swallow = st.swallow;
        let new_hover = hit.map(|h| h.0).filter(|&id| !blocked(id) && !swallow);
        if let Some(old) = st
            .hover
            .filter(|&old| Some(old) != new_hover || grab_started)
        {
            emit(old, PointerEvent::Leave);
        }

        let Some((id, pos, t)) = hit else {
            // 何も無い所を押した（ダッシュボードの外を押して閉じる。何をするかは画面が決める）。
            if press && !st.swallow {
                self.outside_presses += 1;
            }
            st.hover = None;
            st.select = false;
            return;
        };
        if grab_started {
            let t = hit.map_or(0.0, |h| h.2);
            // ナビゲーションバーを掴むと持ち主を掴む（バーは持ち主に付いてくる）。
            let grabbed = self.panels[id].nav_bar_of.unwrap_or(id);
            let (id, pivot) = grab_target(&self.panels, &self.present, grabbed, frame);
            // 大きさを変えるのは、掴んだものそのものを動かすときだけ（ダッシュボードのバーを掴んで上の画面を変えない）。
            let resize = id == grabbed && self.panels[id].resizable;
            let reference = self.panels[id].pose * pivot.inverse();
            let offset = aim.inverse().transform_point(reference.position);
            st.dragging = Some((id, offset, t, pivot, resize));
            st.hover = None;
            st.select = false;
            return;
        }
        if blocked(id) || swallow {
            st.hover = None;
            st.select = false;
            return;
        }

        // 押した点から少し揺れただけなら、離すまで押した位置を指していることにする。
        let pos = match st.press {
            Some((at, point))
                if st.select
                    && st.hover == Some(id)
                    && (point - aim.position).angle_between(aim.forward()) < CLICK_SLOP =>
            {
                at
            }
            _ => {
                st.press = None;
                pos
            }
        };
        emit(id, PointerEvent::Move { pos });
        if c.select != st.select {
            // 手ブレのある入力だけ、押した位置を保持する。
            st.press = (c.select && frame.stabilize_pointer)
                .then(|| (pos, aim.position + aim.forward() * t));
            let event = if c.select {
                st.button = c.click_button;
                match st.button {
                    subsystems::input::ClickButton::Left => PointerEvent::Down { pos },
                    button => PointerEvent::DownButton { pos, button },
                }
            } else {
                match st.button {
                    subsystems::input::ClickButton::Left => PointerEvent::Up { pos },
                    button => PointerEvent::UpButton { pos, button },
                }
            };
            emit(id, event);
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
        if !c.select {
            st.free = false;
        }
    }
}

/// この tick の手ブレ抑え: 前の向きを残す割合。`moved` は前のレーザーから手の向きまでの角度。
/// 速く振るほど弱め（`release` ラジアン遅れると半分）、押下前にトリガーを引いている間は強める。
fn steadiness(base: f32, moved: f32, release: f32, c: &ControllerState) -> f32 {
    if base == 0.0 {
        return 0.0;
    }
    let s = base / (1.0 + moved / release);
    if c.select || c.grab {
        return s;
    }
    s.max(TRIGGER_STEADY * c.select_progress())
}

/// `grabbed` を掴んだとき、実際に手で動かすパネルと、基準（手で動かして頭へ向けるもの）から見たそのパネルの姿勢。
/// 固定した子（[`Parent::docked`]）でつながったもの（ダッシュボードのバー・そこにはめた画面・キーボード）は、どれを掴んでも
/// 上にはめた画面を基準に動かす（頭に向けて垂直に立て、ほかは付いてくる）。はめた画面が無ければ、根元の
/// [`Panel::grab_anchor`]（画面が立つ所）を基準にする。それも無ければ根元を今の傾きのまま動かす。
pub(crate) fn grab_target(
    panels: &[Panel],
    present: &[PanelId],
    grabbed: PanelId,
    frame: &Frame,
) -> (PanelId, Pose) {
    let docked = |id: PanelId| {
        present.iter().any(|&c| {
            panels[c].parent.is_some_and(|q| q.panel == id && q.docked) && !panels[c].closed
        })
    };
    if !panels[grabbed].parent.is_some_and(|q| q.docked)
        && !docked(grabbed)
        && panels[grabbed].grab_anchor.is_none()
    {
        return (grabbed, Pose::IDENTITY);
    }
    let root = carried(panels, grabbed).last().copied().unwrap_or(grabbed);
    let slot = present.iter().copied().find(|&c| {
        let q = &panels[c];
        !q.closed
            && !q.hidden
            && q.parent
                .is_some_and(|q| q.panel == root && q.slot.is_some())
    });
    if let Some(slot) = slot {
        return (slot, Pose::IDENTITY);
    }
    let p = &panels[root];
    if let Some(anchor) = p.grab_anchor {
        return (root, anchor.inverse());
    }
    let lean = facing(p.pose.position, frame.hmd.position, frame.hmd.panel_up())
        .map_or(Quat::IDENTITY, |q| q.inverse() * p.pose.orientation);
    (root, Pose::new(Vec3::ZERO, lean))
}

#[cfg(test)]
mod tests;
