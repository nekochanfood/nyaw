//! 角枠: 角丸矩形の隅だけを、丸みとそこから辺に沿って少し伸ばした線で囲む（フルスクリーンアイコンに似た形）。
//! はまる所の見本（[`crate::dock`]）が使う。同じ見た目の線を出すものは、これを使う。
//! 線の太さとパネル面からの隙間はオーバーレイ層も配置に使うので、`design`（[`BRACKET_LINE_PX`]・[`design::BRACKET_PAD_PX`]）に置く。

use design::{BRACKET_LINE_PX, Theme, radius};
use gpui::{
    Bounds, Canvas, PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Window, canvas, point,
    px, rgba,
};
use lyon::tessellation::LineCap;

/// 角の丸みから辺に沿って伸ばす線の長さ（px）。短辺の直線部分の [`ARM_MAX`] 以下に制限する（隣の角の線と接続しないようにする）。
pub const ARM_PX: f32 = 40.0;
const ARM_MAX: f32 = 0.35;
/// 強調しないときの線の不透明度（無彩色の文字色に乗算。0〜255）。
const LOOSE_ALPHA: u32 = 0xb0;

/// 角枠で囲む隅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    BottomRight,
    BottomLeft,
    TopLeft,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopRight,
        Corner::BottomRight,
        Corner::BottomLeft,
        Corner::TopLeft,
    ];
}

/// 線の色。ドッキングが成立する・角を掴んでいるなど、強調するなら強調色、それ以外は無彩色。
pub fn color(theme: &Theme, strong: bool) -> u32 {
    if strong {
        theme.accent_solid
    } else {
        theme.neutral_text & !0xff | LOOSE_ALPHA
    }
}

/// 要素の領域全体を角丸（`radius::XL`）矩形とし、その隅のうち `corners` を色 `color` の角枠で囲む要素。
pub fn frame(corners: &'static [Corner], color: u32) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(bounds, corners, color, window),
    )
}

/// `bounds` 全体の角丸矩形の隅のうち `corners` を、丸みとそこから辺に沿って [`ARM_PX`] 伸ばした線で描く。線の端は丸める。
fn paint(bounds: Bounds<Pixels>, corners: &[Corner], color: u32, window: &mut Window) {
    let half = BRACKET_LINE_PX / 2.0;
    let (x0, y0) = (
        f32::from(bounds.origin.x) + half,
        f32::from(bounds.origin.y) + half,
    );
    let (w, h) = (
        f32::from(bounds.size.width) - BRACKET_LINE_PX,
        f32::from(bounds.size.height) - BRACKET_LINE_PX,
    );
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = (radius::XL - half).clamp(0.0, w.min(h) / 2.0);
    let edges = Perimeter { x0, y0, w, h, r };
    let (starts, len) = edges.brackets();
    let style = StrokeOptions::default()
        .with_line_width(BRACKET_LINE_PX)
        .with_line_cap(LineCap::Round);
    for &corner in corners {
        let start = starts[corner as usize];
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        let mut path =
            PathBuilder::stroke(px(BRACKET_LINE_PX)).with_style(PathStyle::Stroke(style));
        path.move_to(edges.at(start));
        for i in 1..=steps {
            path.line_to(edges.at(start + len * i as f32 / steps as f32));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, rgba(color));
        }
    }
}

/// 角丸矩形（線の中心）の周長。上辺左角の終端から時計回りに進行する。
struct Perimeter {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl Perimeter {
    /// 四隅の角線（右上・右下・左下・左上。[`Corner`] の順）の開始位置とその長さ。丸みの前後に辺方向へ [`ARM_PX`] ずつ延伸する。
    fn brackets(&self) -> ([f32; 4], f32) {
        let (sw, sh, arc) = (
            self.w - 2.0 * self.r,
            self.h - 2.0 * self.r,
            std::f32::consts::FRAC_PI_2 * self.r,
        );
        let arm = ARM_PX.min(ARM_MAX * sw.min(sh)).max(0.0);
        // 右上・右下・左下・左上の丸みの開始位置。
        let corners = [
            sw,
            sw + arc + sh,
            2.0 * sw + 2.0 * arc + sh,
            2.0 * sw + 3.0 * arc + 2.0 * sh,
        ];
        (corners.map(|c| c - arm), arm + arc + arm)
    }

    fn length(&self) -> f32 {
        2.0 * (self.w + self.h - 4.0 * self.r) + std::f32::consts::TAU * self.r
    }

    /// 周上を `s` だけ進んだ座標。
    fn at(&self, s: f32) -> Point<Pixels> {
        let Self { x0, y0, w, h, r } = *self;
        let (sw, sh, arc) = (w - 2.0 * r, h - 2.0 * r, std::f32::consts::FRAC_PI_2 * r);
        let corner = |cx: f32, cy: f32, from: f32, s: f32| {
            let a = from + if r > 0.0 { s / r } else { 0.0 };
            point(px(cx + r * a.cos()), px(cy + r * a.sin()))
        };
        // 上辺・右上・右辺・右下・下辺・左下・左辺・左上（画面座標系で下方向が +y）の各区間長。
        let lens = [sw, arc, sh, arc, sw, arc, sh, arc];
        let mut s = s.rem_euclid(self.length());
        let mut i = 0;
        while i < lens.len() - 1 && s > lens[i] {
            s -= lens[i];
            i += 1;
        }
        let s = s.min(lens[i]);
        let pi = std::f32::consts::PI;
        match i {
            0 => point(px(x0 + r + s), px(y0)),
            1 => corner(x0 + w - r, y0 + r, -pi / 2.0, s),
            2 => point(px(x0 + w), px(y0 + r + s)),
            3 => corner(x0 + w - r, y0 + h - r, 0.0, s),
            4 => point(px(x0 + w - r - s), px(y0 + h)),
            5 => corner(x0 + r, y0 + h - r, pi / 2.0, s),
            6 => point(px(x0), px(y0 + h - r - s)),
            _ => corner(x0 + r, y0 + r, pi, s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_perimeter_walks_around_the_rounded_rect_clockwise() {
        let p = Perimeter {
            x0: 0.0,
            y0: 0.0,
            w: 100.0,
            h: 60.0,
            r: 10.0,
        };
        let total = p.length();
        let close = |a: Point<Pixels>, x: f32, y: f32| {
            (f32::from(a.x) - x).abs() < 1e-3 && (f32::from(a.y) - y).abs() < 1e-3
        };
        // 上辺左端から開始し、右上角を経て右辺へ進み、一周で始点に戻る。
        assert!(close(p.at(0.0), 10.0, 0.0));
        assert!(close(p.at(80.0), 90.0, 0.0));
        let corner = std::f32::consts::FRAC_PI_2 * 10.0;
        assert!(close(p.at(80.0 + corner), 100.0, 10.0));
        assert!(close(p.at(80.0 + corner + 40.0), 100.0, 50.0));
        assert!(close(p.at(total), 10.0, 0.0));
        // 四隅の角線はそれぞれの角を囲み、隣の角線と接続しない。
        let (starts, len) = p.brackets();
        assert!(close(
            p.at(starts[0] + len / 2.0),
            100.0 - 10.0 + 10.0 * std::f32::consts::FRAC_1_SQRT_2,
            10.0 - 10.0 * std::f32::consts::FRAC_1_SQRT_2
        ));
        for k in 0..4 {
            let next = (starts[(k + 1) % 4] - starts[k]).rem_euclid(total);
            assert!(len < next, "{k}: {len} >= {next}");
        }
        // すべての点が角丸矩形の輪郭上にある（外側にはみ出さない）。
        for i in 0..200 {
            let a = p.at(total * i as f32 / 200.0);
            let (x, y) = (f32::from(a.x), f32::from(a.y));
            assert!((-1e-3..=100.001).contains(&x) && (-1e-3..=60.001).contains(&y));
        }
    }
}
