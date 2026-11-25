//! 配置計算ユーティリティ: アンカー方向・補間・向き・面との交差判定。

use super::*;

/// ツールチップ方向切替の閾値: 頭から見た手の方向がパネル上下へ水平からこの値（sin。約 12°）を超えると切り替わる。
/// ヒステリシス帯域内は前の方向を維持（水平付近での往復を防止）。
// ponytail: 固定の閾値。切り替わりが敏感・鈍ければここを調整する。
const ANCHOR_BAND: f32 = 0.2;

/// 頭 `eye` から `point` を見て、手 `hand` がパネルの上 `up` に対し上下どちらにあるかでツールチップ方向を決定する。
/// 手が下（レーザーが下から入射）なら下に、上なら上に配置する。不明確なら `prev` を維持。
pub(crate) fn anchor(prev: Anchor, eye: Vec3, hand: Vec3, point: Vec3, up: Vec3) -> Anchor {
    let view = (point - eye).normalize_or_zero();
    let flat = |v: Vec3| v - view * v.dot(view);
    let (hand, up) = (flat(hand - point), flat(up).normalize_or_zero());
    let len = hand.length();
    // 手が視線上にある（novr のように目から指す場合）と、レーザーは点にしか見えない。
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

/// `from` から `to` へ経過 `dt` 秒分（[`SMOOTH`]）だけ補間する。十分近ければ `to` にスナップ。
pub(crate) fn approach(from: Pose, to: Pose, dt: f32) -> Pose {
    if from.position.distance(to.position) < SNAP
        && from.orientation.angle_between(to.orientation) < SNAP
    {
        return to;
    }
    let k = smoothing(dt);
    Pose::new(
        from.position.lerp(to.position, k),
        from.orientation.slerp(to.orientation, k),
    )
}

/// 経過 `dt` 秒で目標へ近づく割合（[`SMOOTH`] を経過時間で換算したもの）。
pub(crate) fn smoothing(dt: f32) -> f32 {
    1.0 - (1.0 - SMOOTH).powf(dt / TICK_MIN)
}

/// パネル面の縁とクアッドの縁の間隔（テクスチャの画素。シャドウマージン）。取り込み中の配信映像は全面が面なので無いが、
/// 面に重ねる子（[`Parent::over`]）は取り込み中でも影を持つ（[`with_shadow`]）。
// ponytail: キャプチャで決める。マージンの有無が入力の扱いと食い違うケースが他にも出たらパネルにフラグを持たせる。
pub(crate) fn margin_px(p: &Panel) -> f32 {
    if p.capture && !p.parent.is_some_and(|q| q.over.is_some()) {
        0.0
    } else {
        PANEL_MARGIN
    }
}

/// パネル面の縁とクアッドの縁の間隔（m。[`margin_px`]）。
pub(crate) fn face_margin(p: &Panel) -> f32 {
    margin_px(p) * p.size.y / p.resolution.1 as f32
}

/// `at` に配置し表面（+Z）を `eye` に向け、上（+Y）を `up` に近づけた回転。`up` と視線が平行なら `None`。
pub(crate) fn facing(at: Vec3, eye: Vec3, up: Vec3) -> Option<Quat> {
    let z = (eye - at).try_normalize()?;
    let x = up.cross(z).try_normalize()?;
    Some(Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)))
}

/// レイ（aim の前方）とパネル表面（+Z 側）の交差判定。ヒットピクセルとレイ上の距離を返す。裏面からはヒットしない。
pub(crate) fn hit_test(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    if aim.forward().dot(p.pose.orientation * Vec3::Z) >= 0.0 {
        return None;
    }
    let (px, t) = plane_hit(p, aim)?;
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    ((0.0..=w).contains(&px.x) && (0.0..=h).contains(&px.y)).then_some((px, t))
}

/// レイとパネルを含む平面の交点（パネル外も含む）のピクセル座標とレイ上の距離。レイが面と平行または後方なら `None`。
pub(crate) fn plane_hit(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    let (local, t) = plane_point(p, aim)?;
    let size = p.displayed_size();
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    Some((
        Vec2::new((local.x / size.x + 0.5) * w, (0.5 - local.y / size.y) * h),
        t,
    ))
}

/// レイとパネルを含む平面の交点（パネルローカル座標、m）とレイ上の距離。レイが面と平行または後方なら `None`。
pub(crate) fn plane_point(p: &Panel, aim: &Pose) -> Option<(Vec3, f32)> {
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
    Some((p.pose.inverse().transform_point(aim.position + dir * t), t))
}

/// 等倍設計の姿勢 `pose` とグラブ基準 `anchor`（パネルローカル姿勢。等倍）から、UI スケール `scale` 適用時の
/// パネル姿勢を返す。基準の配置は不変で、パネル中心が基準から `scale` 倍離れる（基準中心の相似拡大）。
pub fn anchored(pose: Pose, anchor: Pose, scale: f32) -> Pose {
    pose * anchor * Pose::new(anchor.position * scale, anchor.orientation).inverse()
}

#[cfg(test)]
mod tests;
