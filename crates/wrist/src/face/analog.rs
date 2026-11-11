//! アナログの文字盤: 針で時刻を指す。デザインごとに、題材にした時計（素っ気ない時計・ドレスウォッチ・駅の時計）が違う。
//! どれも秒針と中心の軸は塗りの色。日付は 6 時の側に小さく出す。

use design::{font, font_size};
use gpui::{Div, prelude::*, rgba};

use super::{BASE, Face};

/// 6 時の側（中心と目盛りの間）の、控えめな色の日付。
fn date(f: &Face) -> Div {
    f.date()
        .absolute()
        .top(f.px(BASE * 0.66))
        .text_size(f.px(font_size::MD))
        .text_color(rgba(f.theme.neutral_text_low))
}

/// ミニマル: 12 の短い目盛りと、端の丸い針だけ。
pub fn minimal(f: &Face) -> Div {
    let t = f.theme;
    let (ink, low, accent) = (t.neutral_text, t.neutral_text_low, t.accent_solid);
    let [hour, minute, second] = f.hands();
    let hands = f.paint(move |pen| {
        for i in 0..12 {
            let (from, width) = if i % 3 == 0 { (0.76, 6.0) } else { (0.83, 4.0) };
            pen.rounded_spoke(i as f32 / 12.0, from, 0.9, width, low);
        }
        pen.rounded_spoke(hour, 0.0, 0.48, 12.0, ink);
        pen.rounded_spoke(minute, 0.0, 0.72, 8.0, ink);
        // 秒針は反対側へ少し伸ばし、軸で留める。
        pen.rounded_spoke(second, -0.16, 0.8, 3.0, accent);
        pen.disc(0.0, 0.0, 8.0, accent);
    });
    f.frame().child(date(f)).child(hands)
}

/// ドレス: セリフのローマ数字（12・3・6・9）と、細い目盛り・細い針。
pub fn dress(f: &Face) -> Div {
    let t = f.theme;
    let (ink, low, accent) = (t.neutral_text, t.neutral_text_low, t.accent_solid);
    let [hour, minute, second] = f.hands();
    let numerals = [(0, "XII"), (3, "III"), (6, "VI"), (9, "IX")].map(|(at, numeral)| {
        f.label(at as f32 / 12.0, 0.7, numeral)
            .font(font::serif())
            .text_size(f.px(40.0))
    });
    let hands = f.paint(move |pen| {
        for i in 0..60 {
            // 数字のある所には目盛りを引かない。
            if i % 15 != 0 {
                let (from, width) = if i % 5 == 0 { (0.82, 2.5) } else { (0.89, 1.5) };
                pen.spoke(i as f32 / 60.0, from, 0.92, width, low);
            }
        }
        pen.rounded_spoke(hour, 0.0, 0.46, 7.0, ink);
        pen.rounded_spoke(minute, 0.0, 0.74, 5.0, ink);
        pen.spoke(second, -0.18, 0.82, 2.0, accent);
        pen.disc(0.0, 0.0, 6.0, accent);
    });
    f.frame()
        .children(numerals)
        .child(date(f).top(f.px(BASE * 0.6)))
        .child(hands)
}

/// 鉄道時計: 駅の時計のような太い目盛りと、まっすぐな太い針。秒針の先に円が付く。
pub fn railway(f: &Face) -> Div {
    let t = f.theme;
    let (ink, accent) = (t.neutral_text, t.accent_solid);
    let [hour, minute, second] = f.hands();
    let hands = f.paint(move |pen| {
        for i in 0..60 {
            let (from, width) = if i % 5 == 0 { (0.72, 9.0) } else { (0.87, 3.0) };
            pen.spoke(i as f32 / 60.0, from, 0.94, width, ink);
        }
        pen.spoke(hour, -0.12, 0.54, 16.0, ink);
        pen.spoke(minute, -0.12, 0.86, 11.0, ink);
        pen.spoke(second, -0.2, 0.6, 4.0, accent);
        pen.disc(second, 0.6, 18.0, accent);
        pen.disc(0.0, 0.0, 7.0, accent);
    });
    f.frame().child(date(f)).child(hands)
}
