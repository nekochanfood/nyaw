//! レーザー・カーソル・ツールチップ・ドラッグの見た目・OS のカーソルの置き場所。

use super::*;

/// レーザーのクアッドの向き: ローカル +X（テクスチャの右）を前方 -Z へ、面の法線 +Z を上 +Y へ。
const LASER_ROT: Quat = Quat::from_xyzw(-0.5, 0.5, 0.5, 0.5);
/// OS のカーソルを最近傍で拡大する目安の一辺（px）。
const OS_CURSOR_PX: u32 = 256;
/// ツールチップをパネル面から浮かせる距離（m）。カーソルより手前。
const TOOLTIP_LIFT: f32 = LIFT_STEP * 3.0;
/// ドラッグの見た目をパネル面から浮かせる距離（m）。カーソルより奥。
const DRAG_LIFT: f32 = LIFT_STEP;

impl Overlay {
    /// レーザーとカーソルのパネルを、この tick のレーザーに合わせて置く・伸ばす・隠す。
    pub(crate) fn place_pointers(&mut self, vr: &mut dyn VrBackend) {
        // 向きは毎 tick 数回の内積で決まるので、指している間はいつも見直す。
        let eye = self.hmd.position;
        let laser_length = self.laser_length();
        let steadied = self.steadied;
        // 骨格から作ったレーザーの手は、ランタイムのデバイスの姿勢と関係ない（ランタイムに手からの相対で置かせると、デバイスの
        // 先からずれる）ので、空間に置く。
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
            let Some((hand, part)) = p.pointer.filter(|_| !p.closed) else {
                continue;
            };
            let ray = self.hands.get(&hand).and_then(|h| h.ray);
            let drag = self.hands.get(&hand).and_then(|h| h.drag);
            let drawn = ray.map(|r| (r, r.aim, r.len));
            let placed = match (part, drawn) {
                (Pointer::Laser, Some((r, aim, len))) => {
                    // 横から見ると面が見えなくなるので、レーザーの軸回りに面を頭へ向ける。
                    let h = aim.inverse().transform_point(self.hmd.position);
                    let roll = Quat::from_rotation_z((-h.x).atan2(h.y));
                    let len = (len - LASER_GAP).clamp(0.0, laser_length);
                    let pose = aim * Pose::new(Vec3::new(0.0, 0.0, -len / 2.0), roll * LASER_ROT);
                    Some((r, pose, len))
                }
                // OS のカーソルを持つパネルの上では、代わりにその絵を出す（place_os_cursors）。
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
                        // ツールチップはカーソルの子: 同じ点に、同じ縮尺で、少しだけ手前に置く。
                        let lift = if part == Pointer::Cursor {
                            CURSOR_LIFT
                        } else {
                            TOOLTIP_LIFT + LIFT_STEP * tooltip_step(hand) as f32
                        };
                        let pose = on_panel(&self.panels[target].pose, &aim, len, lift);
                        // 頭から見て同じ大きさに見えるよう、離れるほど大きくする（`size` は 1m 先での大きさ）。
                        // 距離はヒット点まで（浮かせた分は含めない）なので、カーソルとツールチップの縮尺はそろう。
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
                // レーザーは手からの相対で置き、ランタイムの追従で手元から離れないようにする。
                // カーソルなど面の上のものは、手ブレ抑えが効いていれば空間に置く: 手に追従させると、tick の間
                // （ゲームで tick が遅れるほど長い）はランタイムが手ブレどおりに動かし、次の tick で抑えた位置へ
                // 戻されてガタつく。抑えていなければ手に追従させ、レーザーの先とずれないようにする。
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

    /// OS のカーソルを持つパネルを指している手に、その絵をホットスポットがヒット点に来るよう置く。
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
                        Ok(s) => {
                            s.set_sort(vr, CURSOR_SORT);
                            e.insert((s, None))
                        }
                        Err(err) => {
                            logger::warn!("OS のカーソルの面を作れない", error = ?err);
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
            // 絵はホットスポットを中心にした正方形なので、形が変わっても置き場所と大きさは変わらない
            // （変えると、新しい絵が届くまでの間に古い絵がずれた所へ出てちらつく）。
            let p = &self.panels[target];
            let m = p.displayed_size().x / p.resolution.0 as f32;
            let width = canvas_px(&cursor.image) as f32 * m;
            let pose = on_panel(&p.pose, &r.aim, r.len, CURSOR_LIFT);
            // カーソルと同じく、手ブレ抑えが効いていなければ手に追従させる（place_pointers）。
            let follow = (!self.steadied && !skeletal.contains(&hand))
                .then(|| (Device::Controller(hand), r.hand.inverse() * pose));
            surface.show(vr, true, Some(width), width);
            surface.place(vr, pose, follow.as_ref());
        }
    }
}

/// 両手のツールチップが重なったとき前後が決まるよう、右手のものを描画順・浮かせる距離とも一段手前にする
/// （同じ順・同じ面だと Z ファイティングする）。
pub(crate) fn tooltip_step(hand: Hand) -> u32 {
    u32::from(hand == Hand::Right)
}

/// OS のカーソルを載せる正方形の一辺（元の画素）。ホットスポットが絵のどこにあっても収まる。
fn canvas_px(image: &subsystems::desktop::Icon) -> u32 {
    image.width.max(image.height).max(1) * 2
}

/// ホットスポットを中心にした正方形に絵を置き、一辺がおよそ 2 × [`OS_CURSOR_PX`] になるまで画素を整数倍に複製する
/// （事前乗算アルファ）。ランタイムの合成は双線形で拡大するので、先に最近傍で大きくしておけば画素の境目がぼけない。
fn hotspot_canvas(cursor: &Cursor) -> (u32, Vec<u8>) {
    let image = &cursor.image;
    let n = canvas_px(image) as usize;
    let k = (OS_CURSOR_PX as usize * 2 / n).max(1);
    let (w, h) = (image.width as usize, image.height as usize);
    // 絵の左上の、正方形の中での位置。
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

/// レーザーの先（`aim` の前方 `len`）で、パネルの面に沿わせてレーザーの来る側（裏から指していれば裏側）へ `lift` 浮かせた姿勢。
fn on_panel(panel: &Pose, aim: &Pose, len: f32, lift: f32) -> Pose {
    let point = aim.position + aim.forward() * len;
    let normal = panel.orientation * Vec3::Z;
    let side = (aim.position - point).dot(normal).signum();
    Pose::new(point + normal * side * lift, panel.orientation)
}

#[cfg(test)]
mod tests;
