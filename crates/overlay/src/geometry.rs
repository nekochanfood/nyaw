//! 置き場所の計算の小さな道具: 取り付けの基準・近づき方・向き・面との交差。

use super::*;

/// ツールチップの向きを変える目安: 頭から見た手の方向が、パネルの上下へ水平からこれ（sin。約 12°）より傾いたら切り替える。
/// 間は前の向きのまま（水平近くで行ったり来たりしないように）。
// ponytail: 固定の幅。切り替わりが敏感・鈍いと感じたらここを調整する。
const ANCHOR_BAND: f32 = 0.2;

/// 頭 `eye` から点 `point` を見たとき、手 `hand` がパネルの上 `up` に対して上下どちらに見えるかでツールチップの向きを決める。
/// 手が下に見えれば（レーザーは下から来る）下に、上に見えれば上に出す（手の側）。どちらとも言えなければ `prev` のまま。
pub(crate) fn anchor(prev: Anchor, eye: Vec3, hand: Vec3, point: Vec3, up: Vec3) -> Anchor {
    let view = (point - eye).normalize_or_zero();
    let flat = |v: Vec3| v - view * v.dot(view);
    let (hand, up) = (flat(hand - point), flat(up).normalize_or_zero());
    let len = hand.length();
    // 手が視線の上にある（novr のように目から指す）と、レーザーは点にしか見えない。
    if len < 1e-4 {
        return prev;
    }
    let s = hand.dot(up) / len;
    if s < -ANCHOR_BAND {
        Anchor::Below
    } else if s > ANCHOR_BAND {
        Anchor::Above
    } else {
        prev
    }
}

/// `from` から `to` へ、経過 `dt` 秒の分（[`SMOOTH`]）だけ近づける。十分近ければ `to`。
pub(crate) fn approach(from: Pose, to: Pose, dt: f32) -> Pose {
    if from.position.distance(to.position) < SNAP
        && from.orientation.angle_between(to.orientation) < SNAP
    {
        return to;
    }
    let k = 1.0 - (1.0 - SMOOTH).powf(dt / TICK_MIN);
    Pose::new(
        from.position.lerp(to.position, k),
        from.orientation.slerp(to.orientation, k),
    )
}

/// パネルの地の縁と、クアッドの縁の間（m。影の余白）。
// ponytail: 配信中（`capture`）は絵を窓いっぱいに送り、影の余白が無い。余白の無い中身がほかにも出たら、パネルに余白の有無を持たせる。
pub(crate) fn face_margin(p: &Panel) -> f32 {
    if p.capture {
        0.0
    } else {
        PANEL_MARGIN * p.size.y / p.resolution.1 as f32
    }
}

/// `at` に置いて表面（+Z）を `eye` に向け、上（+Y）を `up` に寄せた向き。`up` と視線が平行なら `None`。
pub(crate) fn facing(at: Vec3, eye: Vec3, up: Vec3) -> Option<Quat> {
    let z = (eye - at).try_normalize()?;
    let x = up.cross(z).try_normalize()?;
    Some(Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)))
}

/// レイ（aim の前方）とパネルの表面（+Z 側）の交差判定。ヒットしたピクセルとレイ上の距離を返す。裏からは当たらない。
pub(crate) fn hit_test(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    if aim.forward().dot(p.pose.orientation * Vec3::Z) >= 0.0 {
        return None;
    }
    let (px, t) = plane_hit(p, aim)?;
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    ((0.0..=w).contains(&px.x) && (0.0..=h).contains(&px.y)).then_some((px, t))
}

/// レイとパネルを含む平面の交点（パネルの外でもよい）のピクセル座標とレイ上の距離。レイが面と平行か後ろなら `None`。
pub(crate) fn plane_hit(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
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
    let size = p.displayed_size();
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    Some((
        Vec2::new((local.x / size.x + 0.5) * w, (0.5 - local.y / size.y) * h),
        t,
    ))
}

/// 等倍のつもりで決めた姿勢 `pose` と掴む基準 `anchor`（パネルから見た姿勢。等倍）から、UI スケール `scale` のときの
/// パネルの姿勢を返す。基準の置き場所は変えず、パネルの中心が基準から `scale` 倍だけ離れる（組が基準を中心に相似に大きくなる）。
pub fn anchored(pose: Pose, anchor: Pose, scale: f32) -> Pose {
    pose * anchor * Pose::new(anchor.position * scale, anchor.orientation).inverse()
}

#[cfg(test)]
mod tests;
