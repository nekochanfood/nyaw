//! ドッキングスロットのゴースト表示（[`Pointer::Dock`]）。ドッキング可能なパネルを掴んでいる間、スロットにパネル背景より少し大きい角枠とヒントテキストを描く。
//! ドッキング成立時は強調色、解除時は無彩色で状態を視覚的に区別する。
//! 描画のみを担い、配置・寸法・フェード制御はオーバーレイ層が行う（再描画はヒントテキスト変更時のみ）。

use design::{Theme, font_size, icons, radius, spacing};
use gpui::{
    Bounds, Context, PathBuilder, Pixels, Point, Window, canvas, div, point, prelude::*, px, rgba,
};
use kernel::overlay::DOCK_HINT_PX;
use kernel::{DockHint, Hand, PanelId, Pointer, Pose, Vec2};
use ui::PanelSpec;

/// ウィンドウの初期サイズ（px）。掴んだパネルに合わせてオーバーレイ層が変更する。
const GHOST_PX: (u32, u32) = (400, 300 + DOCK_HINT_PX);
/// 角線の太さと、角の丸みから辺に沿って伸ばす長さ（px）。伸ばす長さは短辺の直線部分の [`ARM_MAX`] 以下に制限する
/// （隣の角線と接続しないようにする）。
const BORDER_PX: f32 = 6.0;
const ARM_PX: f32 = 96.0;
const ARM_MAX: f32 = 0.35;
/// ドッキング解除状態の枠の不透明度（無彩色の文字色に乗算。0〜255）。
const LOOSE_ALPHA: u32 = 0xb0;

/// `hand` のドッキングゴーストの起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, |panel, _, _| DockGhost {
        panel,
        hint: DockHint::Attach,
    })
    .sized(Vec2::splat(0.4), GHOST_PX)
    .pointer(hand, Pointer::Dock)
}

/// 上部にヒントテキスト帯（Pull / Release / Attach）、下部に角枠のみのスロット表示（中央は透過）。
pub struct DockGhost {
    panel: PanelId,
    /// 最後に表示したヒント（フェードアウト中も同じものを維持する）。
    hint: DockHint,
}

impl Render for DockGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hint) = cx
            .try_global::<ui::PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .and_then(|s| s.status.dock_hint)
        {
            self.hint = hint;
        }
        let theme = Theme::current(cx);
        let (glyph, key) = match self.hint {
            DockHint::Pull => (&icons::ARROW_SQUARE_OUT, "dock.pull"),
            DockHint::Release => (&icons::LINK, "dock.release"),
            DockHint::Attach => (&icons::MAGNET, "dock.attach"),
        };
        // ドッキング成立なら強調色、解除なら無彩色。
        let line = if self.hint.docks() {
            theme.accent_solid
        } else {
            theme.neutral_text & !0xff | LOOSE_ALPHA
        };
        let band = div()
            .h(px(DOCK_HINT_PX as f32))
            .flex()
            .items_center()
            .justify_center()
            .child(
                // ツールチップと同じ背景色で、どの面に重なっても可読性を確保する。アイコンは文字色と統一。
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .px(px(spacing::LG))
                    .py(px(spacing::SM))
                    .rounded(px(radius::FULL))
                    .bg(rgba(
                        theme.tooltip & !0xff | design::components::TOOLTIP_ALPHA,
                    ))
                    .text_color(rgba(theme.on_tooltip))
                    .text_size(px(font_size::LG))
                    .whitespace_nowrap()
                    .child(
                        design::icon(glyph)
                            .size(px(font_size::LG))
                            .color(rgba(theme.on_tooltip)),
                    )
                    .child(i18n::t(cx, key)),
            );
        let slot = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| corner_frame(bounds, line, window),
        )
        .flex_1()
        .w_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font(design::font::font())
            .child(band)
            .child(slot)
    }
}

/// `bounds` 全体の角丸（`radius::XL`）矩形の四隅のみを、丸みとそこから辺に沿って [`ARM_PX`] 延伸した線で描画する
/// （フルスクリーンアイコンに類似した角枠。色 `color`）。
fn corner_frame(bounds: Bounds<Pixels>, color: u32, window: &mut Window) {
    let half = BORDER_PX / 2.0;
    let (x0, y0) = (
        f32::from(bounds.origin.x) + half,
        f32::from(bounds.origin.y) + half,
    );
    let (w, h) = (
        f32::from(bounds.size.width) - BORDER_PX,
        f32::from(bounds.size.height) - BORDER_PX,
    );
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = (radius::XL - half).clamp(0.0, w.min(h) / 2.0);
    let edges = Perimeter { x0, y0, w, h, r };
    let (starts, len) = edges.brackets();
    for start in starts {
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        let mut path = PathBuilder::stroke(px(BORDER_PX));
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
    /// 四隅の角線（右上・右下・左下・左上）の開始位置とその長さ。丸みの前後に辺方向へ [`ARM_PX`] ずつ延伸する。
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
