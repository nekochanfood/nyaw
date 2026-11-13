//! 文字。Gen Interface JP（Inter + Noto Sans JP 系の欧文・和文を一つにまとめた書体）を使う。
//! フォントはバイナリに埋め込む（静的フォント、OFL。`fonts/LICENSE-gen-interface-jp`）。
//! 使う太さ（NORMAL / MEDIUM / BOLD）だけを積む。ほかの太さを使うなら同じ配布物から足す。
//! 数字を桁揃えで見せる計測表示には等幅の 0xProto（OFL。`fonts/LICENSE-0xproto`）の Regular だけを使う。
//! 飾りの書体（時計の文字盤など）は [`serif`]（Vercel の Geist Serif）と [`segment`]（DSEG7 Classic）。どちらも OFL で、
//! 画面の本文には使わない。足すときも OFL のものに限り、`fonts/` にライセンスを置く。

use std::borrow::Cow;

use gpui::{
    App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, TextRun,
    Window, px,
};

pub const FAMILY: &str = "Gen Interface JP";
pub const MONO_FAMILY: &str = "0xProto";
pub const SERIF_FAMILY: &str = "Geist Serif";
pub const SEGMENT_FAMILY: &str = "DSEG7 Classic";

/// フォントを GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Medium.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Bold.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/0xProto-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GeistSerif-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/DSEG7Classic-Bold.ttf").as_slice()),
        ])
        .expect("embedded fonts are valid");
}

/// 文字の大きさの倍率と太字化を全域に効かせる（`ui` が全体の設定から書く）。掛けるのは GPUI のテキストシステム（文字を組む所・
/// 書体を決める所）なので、画面のコードは今までどおり等倍の大きさ・太さを書くだけでよく、効かせ忘れは起きない。外すのは
/// `.text_exact()` を付けた部分木だけ（絵の中の文字）。変わったら全ウィンドウを描き直し、`true` を返す。AGENT.md の「文字の大きさと太字化」。
pub fn set_text(scale: f32, bold: bool, cx: &mut App) -> bool {
    let before = gpui::text_settings();
    gpui::set_text_scale(scale);
    gpui::set_bold_text(bold);
    if gpui::text_settings() == before {
        return false;
    }
    cx.refresh_windows();
    true
}

/// 文字に合わせて決めた長さ `length`（px。行の高さから作った箱など）を、今の文字の大きさの倍率に合わせる。
/// 文字そのものには要らない（テキストシステムが掛ける）。文字ではないが文字に付いていくべき寸法にだけ使う。
pub fn scaled(length: f32) -> Pixels {
    px(length * gpui::text_scale())
}

/// `text` を既定の書体の `size` px・太さ `weight` で 1 行に組んだときの幅。
/// 組むのはテキストシステムなので、文字の大きさの倍率・太字化も掛かった、画面に出るとおりの幅になる。
pub fn width(
    text: impl Into<SharedString>,
    size: f32,
    weight: FontWeight,
    window: &Window,
) -> Pixels {
    let text = text.into();
    let run = TextRun {
        len: text.len(),
        font: Font { weight, ..font() },
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text, px(size), &[run], None);
    line.width
}

/// 既定の書体。
pub fn font() -> Font {
    Font {
        family: FAMILY.into(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// 既定の書体へ回す別の書体（持っていない字は既定の書体で出る。Windows はシステムの書体からしか回せない）。
fn with_fallback(family: &'static str, weight: FontWeight) -> Font {
    Font {
        family: family.into(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![FAMILY.into()])),
        weight,
        ..font()
    }
}

/// 等幅の書体（計測表示など）。和文は持たない。
pub fn mono() -> Font {
    with_fallback(MONO_FAMILY, FontWeight::NORMAL)
}

/// セリフの見出し書体（Vercel の Geist Serif。OFL、`fonts/LICENSE-geist`）。欧文と数字だけ。
pub fn serif() -> Font {
    with_fallback(SERIF_FAMILY, FontWeight::NORMAL)
}

/// 7 セグメントの液晶の数字（DSEG7 Classic の Bold。OFL、`fonts/LICENSE-dseg`）。数字と `:` を同じ幅で出す。
pub fn segment() -> Font {
    with_fallback(SEGMENT_FAMILY, FontWeight::BOLD)
}
