//! 弧に沿う並べ方（[`arc`]）: 大きな円の上の縁に項目を並べる。円を描くことから決まる計算は、ここにだけ書く。
//! 中央の項目が弧の頂点にあり、隣は弧に沿って左右へ下がりながら薄くなり、弧の向きに傾く（大きさはどこでも同じ）。その先は消えて見えない。

use super::{Place, Shape};

/// 隣の項目の不透明度（中央に対する割合）。
const SIDE_ALPHA: f32 = 0.55;
/// 中央と隣の間の隙間（直径に対する割合）。負なので、隣は中央の後ろに少し潜る（大きさを保ったまま、
/// 設定の画面の左右のボタンに掛からない幅に収める）。
const GAP: f32 = -0.13;
/// 弧の半径（中央の直径に対する割合）。大きいほど平らに並ぶ。
const RADIUS: f32 = 2.4;
/// 隣より先の項目が消えきるずれ（項目の数）。
const FADE: f32 = 1.5;
/// 隣（ずれ 1）の中心までの横の距離（直径に対する割合）: 直径 + 隙間。
const STEP: f32 = 1.0 + GAP;

/// 中央から `d` 個ずれた項目（直径を 1 として）: 中心の横のずれ、弧に沿って下がる量、不透明度、傾き（ラジアン。時計回りが正）。
fn slot(d: f32) -> (f32, f32, f32, f32) {
    // 隣がちょうど [`STEP`] だけ横にずれる角度ずつ、円の上を進む。
    let angle = d * (STEP / RADIUS).asin();
    let near = d.abs().min(1.0);
    let fade = ((FADE - d.abs()) / (FADE - 1.0)).clamp(0.0, 1.0);
    (
        RADIUS * angle.sin(),
        RADIUS * (1.0 - angle.cos()),
        (1.0 - (1.0 - SIDE_ALPHA) * near) * fade,
        // 円の中心から外を向く（弧の頂点からの角度だけ傾く）。
        angle,
    )
}

/// 大きな円の弧に沿う並べ方。項目の一辺が `diameter`（px）。
pub fn arc(diameter: f32) -> Shape {
    // 隣は弧に沿って下がるので、その下端まで高さを取る。
    let (_, drop, ..) = slot(1.0);
    let height = (1.0 + drop) * diameter;
    Shape::new(STEP * diameter, height, FADE, move |d| {
        let (x, y, alpha, angle) = slot(d);
        Place {
            x: x * diameter,
            y: (0.5 + y) * diameter,
            side: diameter,
            alpha,
            angle,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbors_follow_the_arc_down_and_fade() {
        assert_eq!(slot(0.0), (0.0, 0.0, 1.0, 0.0));
        let (left, right) = (slot(-1.0), slot(1.0));
        assert!((right.0 - STEP).abs() < 1e-5, "{}", right.0);
        assert!((left.0 + right.0).abs() < 1e-6 && (left.1 - right.1).abs() < 1e-6);
        assert!(right.1 > 0.1 && right.1 < 0.4, "{}", right.1);
        assert_eq!(right.2, SIDE_ALPHA);
        // 途中は滑らかにつながり、隣より先は消える。
        let half = slot(0.5);
        assert!(half.2 < 1.0 && half.2 > SIDE_ALPHA);
        assert!(half.1 > 0.0 && half.1 < right.1);
        assert_eq!(slot(FADE).2, 0.0);
        assert_eq!(slot(-2.0).2, 0.0);
    }

    /// 項目は弧の中心（頂点の真下 [`RADIUS`]）から同じ距離にあり、中心から外を向く（右は時計回り、左は反時計回りに傾く）。
    #[test]
    fn every_item_sits_on_the_circle_facing_out() {
        for d in [-1.4, -1.0, -0.3, 0.0, 0.5, 1.0] {
            let (x, y, .., angle) = slot(d);
            let from_center = x.hypot(RADIUS - y);
            assert!((from_center - RADIUS).abs() < 1e-5, "{d}: {from_center}");
            // 中心から項目への向きが、真上を `angle` だけ時計回りに回した向き。
            let out = x.atan2(RADIUS - y);
            assert!((out - angle).abs() < 1e-5, "{d}: {out} / {angle}");
        }
        assert!(slot(1.0).3 > 0.0 && slot(-1.0).3 < 0.0);
    }

    #[test]
    fn shape_is_scaled_by_the_diameter() {
        let shape = arc(200.0);
        assert_eq!(shape.step, STEP * 200.0);
        assert_eq!(shape.reach, FADE);
        let center = shape.place(0.0);
        assert_eq!((center.x, center.y, center.side), (0.0, 100.0, 200.0));
        let side = shape.place(1.0);
        assert!((side.x - shape.step).abs() < 1e-3);
        assert_eq!(side.side, 200.0, "隣も同じ大きさ");
        assert_eq!(shape.place(0.4).side, 200.0, "動いている途中も同じ大きさ");
        // 隣の下端まで収まる高さ。
        assert!(shape.height >= side.y + side.side / 2.0 - 1e-3);
        assert!(shape.height >= 200.0);
    }
}
