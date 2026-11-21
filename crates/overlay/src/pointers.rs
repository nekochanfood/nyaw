//! レーザー・カーソル・ツールチップ・ドラッグ表示および OS カーソルの配置。

use super::*;

/// レーザークアッドの回転。ローカル +X（テクスチャ右）→ 前方 −Z、法線 +Z → 上 +Y。
const LASER_ROT: Quat = Quat::from_xyzw(-0.5, 0.5, 0.5, 0.5);
/// OS カーソルの最近傍拡大時の目標辺長（px）。
const OS_CURSOR_PX: u32 = 256;
/// ツールチップのパネル面からの浮かせ距離（m）。カーソルより手前に配置。
const TOOLTIP_LIFT: f32 = LIFT_STEP * 3.0;
/// ドラッグ表示のパネル面からの浮かせ距離（m）。カーソルより奥に配置。
const DRAG_LIFT: f32 = LIFT_STEP;

impl Overlay {
    /// 今 tick のレーザーに合わせてレーザー・カーソルパネルの配置・伸縮・表示を更新する。
    pub(crate) fn place_pointers(&mut self, vr: &mut dyn VrBackend) {
        // 向きは毎 tick 内積で算出するため、ポインティング中は常に再計算する。
        let eye = self.hmd.position;
        let laser_length = self.laser_length();
        let steadied = self.steadied;
        // 骨格由来のレーザー手はランタイムのデバイス姿勢と無関係（デバイス相対配置だと先端からずれる）なため、
        // ワールド空間に配置する。
        let skeletal: Vec<Hand> = self
            .controllers
            .iter()
            .filter(|c| c.metrics.is_some())
            .map(|c| c.hand)
            .collect();
        for h in self.hands.values_mut() {
            if let Some(r) = h.ray
                && let Some((target, point)) = r.hit
            {
                let up = self.panels[target].pose.orientation * Vec3::Y;
                h.anchor = anchor(h.anchor, eye, r.hand.position, point, up);
            }
        }
        for &id in &self.present {
            let p = &self.panels[id];
            // ドックゴーストはレーザーではなくグラブ中のパネルに付随する（place_ghosts）。
            let Some((hand, part)) = p
                .pointer
                .filter(|&(_, part)| !p.closed && part != Pointer::Dock)
            else {
                continue;
            };
            let ray = self.hands.get(&hand).and_then(|h| h.ray);
            let drag = self.hands.get(&hand).and_then(|h| h.drag);
            let drawn = ray.map(|r| (r, r.aim, r.len));
            let placed = match (part, drawn) {
                (Pointer::Laser, Some((r, aim, len))) => {
                    // 横から見て面が消えないよう、レーザー軸回りで面を頭方向へ回転させる。
                    let h = aim.inverse().transform_point(self.hmd.position);
                    let roll = Quat::from_rotation_z((-h.x).atan2(h.y));
                    let len = (len - LASER_GAP).clamp(0.0, laser_length);
                    let pose = aim * Pose::new(Vec3::new(0.0, 0.0, -len / 2.0), roll * LASER_ROT);
                    Some((r, pose, len))
                }
                // OS カーソルを持つパネル上では OS カーソル画像を表示する（place_os_cursors）。
                (Pointer::Cursor, Some((r, ..)))
                    if r.hit.is_some_and(|(t, _)| {
                        self.panels[t]
                            .os_cursor
                            .as_ref()
                            .is_some_and(|(h, _)| *h == hand)
                    }) =>
                {
                    None
                }
                (Pointer::Cursor | Pointer::Tooltip, Some((r, aim, len))) => {
                    r.hit.map(|(target, _)| {
                        // ツールチップはカーソルの子。同一点・同一縮尺で、わずかに手前に配置する。
                        let lift = if part == Pointer::Cursor {
                            CURSOR_LIFT
                        } else {
                            TOOLTIP_LIFT + LIFT_STEP * tooltip_step(hand) as f32
                        };
                        let pose = on_panel(&self.panels[target].pose, &aim, len, lift);
                        // 頭からの見かけサイズを一定に保つため距離に比例して拡大する（`size` は 1m 先での値）。
                        // 距離はヒット点基準（浮かし分を含まない）なので、カーソルとツールチップの縮尺は一致する。
                        let hit = aim.position + aim.forward() * len;
                        let width = self.panels[id].size.x * hit.distance(self.hmd.position);
                        let scale = if part == Pointer::Cursor {
                            self.cursor_scale()
                        } else {
                            1.0
                        };
                        (r, pose, width * scale)
                    })
                }
                (Pointer::Drag, Some((r, aim, len))) => r.hit.zip(drag).map(|((target, _), w)| {
                    (
                        r,
                        on_panel(&self.panels[target].pose, &aim, len, DRAG_LIFT),
                        w,
                    )
                }),
                _ => None,
            }
            .map(|(r, pose, width)| {
                // レーザーはデバイス相対配置にし、ランタイム追従で手元から離れないようにする。
                // 面上要素（カーソル等）は手ブレ抑制時にワールド配置する。デバイス追従だと tick 間に
                // ランタイムが手ブレどおりに動かし、次 tick で抑制位置へ戻されてガタつくため。
                // 手ブレ抑制が無効ならデバイス追従にし、レーザー先端とのずれを防ぐ。
                self.panels[id].follow = ((part == Pointer::Laser || !steadied)
                    && !skeletal.contains(&hand))
                .then(|| (Device::Controller(hand), r.hand.inverse() * pose));
                (pose, width)
            });
            let p = &mut self.panels[id];
            if let Some((pose, _)) = placed {
                p.pose = pose;
            }
            p.surface.show(
                vr,
                placed.is_some(),
                placed.map(|(_, width)| width),
                p.size.x,
            );
        }
        self.place_os_cursors(vr, &skeletal);
    }

    /// OS カーソルを持つパネルを指す手に、ホットスポットがヒット点に一致するよう OS カーソル画像を配置する。
    fn place_os_cursors(&mut self, vr: &mut dyn VrBackend, skeletal: &[Hand]) {
        for (&hand, h) in &self.hands {
            let target = h.ray.and_then(|r| {
                let t = r.hit?.0;
                let (owner, cursor) = self.panels[t].os_cursor.clone()?;
                (owner == hand).then_some((r, t, cursor))
            });
            let Some((r, target, cursor)) = target else {
                if let Some((surface, _)) = self.os_cursors.get_mut(&hand) {
                    surface.show(vr, false, None, 0.0);
                }
                continue;
            };
            let (surface, sent) = match self.os_cursors.entry(hand) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let key = format!("nyaw.os-cursor.{hand:?}");
                    match Surface::new(vr, &key, 0.01, Pose::IDENTITY) {
                        Ok(mut s) => {
                            s.set_sort(vr, CURSOR_SORT);
                            e.insert((s, None))
                        }
                        Err(err) => {
                            logger::warn!("OS カーソルのサーフェスを作成できない", error = ?err);
                            continue;
                        }
                    }
                }
            };
            if sent.as_ref() != Some(&cursor) {
                let (side, rgba) = hotspot_canvas(&cursor);
                surface.store(side, side, &rgba);
                surface.present(vr, 1.0, 0.0, true);
                *sent = Some(cursor.clone());
            }
            // 画像はホットスポット中心の正方形。形状が変わっても位置とサイズは維持する
            // （変更すると新画像到着まで旧画像がずれた位置に表示されちらつく）。
            let p = &self.panels[target];
            let m = p.displayed_size().x / p.resolution.0 as f32;
            let width = canvas_px(&cursor.image) as f32 * m;
            let pose = on_panel(&p.pose, &r.aim, r.len, CURSOR_LIFT);
            // カーソル同様、手ブレ抑制が無効ならデバイス追従にする（place_pointers 参照）。
            let follow = (!self.steadied && !skeletal.contains(&hand))
                .then(|| (Device::Controller(hand), r.hand.inverse() * pose));
            surface.show(vr, true, Some(width), width);
            surface.place(vr, pose, follow.as_ref());
        }
    }
}

/// 両手ツールチップの前後関係を確定するため、右手側を描画順・浮かせ距離とも一段手前にする。
/// 同順・同面では Z ファイティングが発生する。
pub(crate) fn tooltip_step(hand: Hand) -> u32 {
    u32::from(hand == Hand::Right)
}

/// OS カーソルを収める正方形の辺長（元画素単位）。ホットスポット位置によらず画像全体が収まるサイズ。
fn canvas_px(image: &subsystems::desktop::Icon) -> u32 {
    image.width.max(image.height).max(1) * 2
}

/// ホットスポット中心の正方形に画像を配置し、辺長が約 2 × [`OS_CURSOR_PX`] になるまで整数倍に拡大する
/// （事前乗算アルファ）。ランタイムの双線形補間による拡大前に最近傍拡大を行い、画素境界のぼけを防ぐ。
fn hotspot_canvas(cursor: &Cursor) -> (u32, Vec<u8>) {
    let image = &cursor.image;
    let n = canvas_px(image) as usize;
    let k = (OS_CURSOR_PX as usize * 2 / n).max(1);
    let (w, h) = (image.width as usize, image.height as usize);
    // 画像左上隅の正方形内オフセット。
    let (ox, oy) = (
        (n / 2) as isize - cursor.hotspot.0 as isize,
        (n / 2) as isize - cursor.hotspot.1 as isize,
    );
    let side = n * k;
    let mut out = vec![0; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let (sx, sy) = ((x / k) as isize - ox, (y / k) as isize - oy);
            if sx < 0 || sy < 0 || sx >= w as isize || sy >= h as isize {
                continue;
            }
            let i = (sy as usize * w + sx as usize) * 4;
            let [r, g, b, a] = [0, 1, 2, 3].map(|c| image.rgba[i + c] as u32);
            let pm = |v: u32| ((v * a + 127) / 255) as u8;
            out[(y * side + x) * 4..][..4].copy_from_slice(&[pm(r), pm(g), pm(b), a as u8]);
        }
    }
    (side as u32, out)
}

/// `aim` 前方 `len` のヒット点で、パネル面に沿いレーザー入射側へ `lift` 浮かせた姿勢を返す。裏面からの入射なら裏側に浮く。
fn on_panel(panel: &Pose, aim: &Pose, len: f32, lift: f32) -> Pose {
    let point = aim.position + aim.forward() * len;
    let normal = panel.orientation * Vec3::Z;
    let side = (aim.position - point).dot(normal).signum();
    Pose::new(point + normal * side * lift, panel.orientation)
}

#[cfg(test)]
mod tests;
