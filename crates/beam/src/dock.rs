//! はまる所の見本（[`Pointer::Dock`]）。はめられる画面を掴んでいる間、はまる所に画面の地より少し大きく出る破線の枠と、上の一行。
//! 離すとはまる（はまったままになる）間は強調色、離すと独立する間は無彩色で、境目が分かる。
//! ここは絵だけを描き、置き場所・大きさ・出入りのフェードはオーバーレイ層が決める（描き直すのは一行が変わったときだけ）。

use design::{Theme, font_size, icons, radius, spacing};
use gpui::{
    Bounds, Context, PathBuilder, Pixels, Point, Window, canvas, div, point, prelude::*, px, rgba,
};
use kernel::overlay::DOCK_HINT_PX;
use kernel::{DockHint, Hand, PanelId, Pointer, Pose, Vec2};
use ui::PanelSpec;

/// 窓の最初の大きさ（px）。掴んだ画面に合わせてオーバーレイ層が変える。
const GHOST_PX: (u32, u32) = (400, 300 + DOCK_HINT_PX);
/// 破線の太さと、1 本の長さ・間の長さ（px）。GPUI の破線（長さ = 太さの 2 倍）では細かすぎるので自分で描く。
const BORDER_PX: f32 = 6.0;
const DASH_PX: f32 = 36.0;
const GAP_PX: f32 = 20.0;
/// 離すと独立する間の破線の濃さ（無彩色の文字色に掛ける不透明度。0〜255）。
const LOOSE_ALPHA: u32 = 0xb0;

/// `hand` の見本の起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, |panel, _, _| DockGhost {
        panel,
        hint: DockHint::Attach,
    })
    .sized(Vec2::splat(0.4), GHOST_PX)
    .pointer(hand, Pointer::Dock)
}

/// 上の帯に一行（引っ張って外す・離してくっつける・近づけてくっつける）、その下にはまる所の破線の枠（中は透ける）。
pub struct DockGhost {
    panel: PanelId,
    /// 最後に出した一行（離して薄れていく間も同じものを出す）。
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
        // 離すとはまるなら強調色、離すと独立するなら無彩色。
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
                // ツールチップと同じ地で、どこに重なっても読める。アイコンは破線と同じ色。
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
                            .color(rgba(line | 0xff)),
                    )
                    .child(i18n::t(cx, key)),
            );
        let slot = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| dashed_outline(bounds, line, window),
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

/// `bounds` いっぱいの角丸（`radius::XL`）の長方形を、[`DASH_PX`] の線と [`GAP_PX`] の間の破線で縁取る（色 `color`）。
/// 一周にちょうど収まるよう、線と間の長さを少し伸び縮みさせる。
fn dashed_outline(bounds: Bounds<Pixels>, color: u32, window: &mut Window) {
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
    let total = edges.length();
    let n = (total / (DASH_PX + GAP_PX)).round().max(1.0);
    let period = total / n;
    let dash = period * DASH_PX / (DASH_PX + GAP_PX);
    for k in 0..n as usize {
        let start = k as f32 * period;
        let steps = (dash / 4.0).ceil().max(1.0) as usize;
        let mut path = PathBuilder::stroke(px(BORDER_PX));
        path.move_to(edges.at(start));
        for i in 1..=steps {
            path.line_to(edges.at(start + dash * i as f32 / steps as f32));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, rgba(color));
        }
    }
}

/// 角丸の長方形（線の中心）の周。上辺の左の角の終わりから時計回りに進む。
struct Perimeter {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl Perimeter {
    fn length(&self) -> f32 {
        2.0 * (self.w + self.h - 4.0 * self.r) + std::f32::consts::TAU * self.r
    }

    /// 周に沿って `s` 進んだ点。
    fn at(&self, s: f32) -> Point<Pixels> {
        let Self { x0, y0, w, h, r } = *self;
        let (sw, sh, arc) = (w - 2.0 * r, h - 2.0 * r, std::f32::consts::FRAC_PI_2 * r);
        let corner = |cx: f32, cy: f32, from: f32, s: f32| {
            let a = from + if r > 0.0 { s / r } else { 0.0 };
            point(px(cx + r * a.cos()), px(cy + r * a.sin()))
        };
        // 上辺・右上・右辺・右下・下辺・左下・左辺・左上（画面の下向きが +y）の長さ。
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
        // 上辺の左端から始まり、右上の角を回って右辺へ、一周で戻る。
        assert!(close(p.at(0.0), 10.0, 0.0));
        assert!(close(p.at(80.0), 90.0, 0.0));
        let corner = std::f32::consts::FRAC_PI_2 * 10.0;
        assert!(close(p.at(80.0 + corner), 100.0, 10.0));
        assert!(close(p.at(80.0 + corner + 40.0), 100.0, 50.0));
        assert!(close(p.at(total), 10.0, 0.0));
        // どの点も、角丸の長方形の線の上（外へはみ出さない）。
        for i in 0..200 {
            let a = p.at(total * i as f32 / 200.0);
            let (x, y) = (f32::from(a.x), f32::from(a.y));
            assert!((-1e-3..=100.001).contains(&x) && (-1e-3..=60.001).contains(&y));
        }
    }
}
