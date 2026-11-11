//! スタンダードの文字盤: 中央に日付と大きな時刻。デザインごとに、縁での秒の見せ方が違う。

use design::{components, font_size, radius, spacing};
use gpui::{Div, FontWeight, prelude::*, px, rgba};

use super::{BASE, Face};

/// 中央の時刻（大きな数字）。
fn time(f: &Face, weight: FontWeight) -> Div {
    f.time()
        .text_size(f.px(96.0))
        .font_weight(weight)
        .line_height(f.px(104.0))
}

/// 時刻の上の、控えめな色の日付。
fn date(f: &Face) -> Div {
    f.date()
        .text_size(f.px(font_size::LG))
        .text_color(rgba(f.theme.neutral_text_low))
}

/// ドット（既定）: 縁に 12 の点の目盛りと、今の秒を指す点（塗りの色）。
pub fn dots(f: &Face) -> Div {
    // 目盛りの中心は、縁から `spacing::LG` 内側。
    const REACH: f32 = 0.88;
    let (low, accent, second) = (
        f.theme.neutral_text_low,
        f.theme.accent_solid,
        f.minute_progress(),
    );
    let marks = f.paint(move |pen| {
        for i in 0..12 {
            let radius = if i % 3 == 0 { 7.0 } else { 4.0 };
            pen.disc(i as f32 / 12.0, REACH, radius, low);
        }
        pen.disc(second, REACH, 6.0, accent);
    });
    f.frame()
        .child(marks)
        .child(date(f))
        .child(time(f, FontWeight::BOLD))
}

/// リング: 縁の輪が秒で満ちる（塗りの色）。日付は塗りの色の札に載せる。
pub fn ring(f: &Face) -> Div {
    let inset = spacing::MD;
    let ring = components::progress_ring(
        f.theme,
        f.size(BASE - 2.0 * inset),
        f.size(10.0),
        f.minute_progress(),
    );
    let tag = f
        .date()
        .px(f.px(spacing::MD))
        .py(f.px(spacing::XS))
        .rounded(px(radius::FULL))
        .bg(rgba(f.theme.accent_solid))
        .text_color(rgba(f.theme.on_accent))
        .text_size(f.px(font_size::MD))
        .font_weight(FontWeight::BOLD);
    f.frame()
        .gap(f.px(spacing::SM))
        .child(gpui::div().absolute().child(ring))
        .child(tag)
        .child(time(f, FontWeight::BOLD))
}

/// ゲージ: 縁に 60 の細い目盛り。0 秒から今の秒までが塗りの色で灯る。
pub fn gauge(f: &Face) -> Div {
    let (off, on, second) = (
        f.theme.neutral_border,
        f.theme.accent_solid,
        f.minute_progress(),
    );
    let marks = f.paint(move |pen| {
        for i in 0..60 {
            let turn = i as f32 / 60.0;
            let (from, width) = if i % 5 == 0 { (0.8, 4.0) } else { (0.86, 2.5) };
            let color = if turn <= second { on } else { off };
            pen.spoke(turn, from, 0.94, width, color);
        }
    });
    f.frame()
        .child(marks)
        .child(date(f))
        .child(time(f, FontWeight::MEDIUM))
}
