//! 文字。Gen Interface JP（Inter + Noto Sans JP 系の欧文・和文を一つにまとめた書体）を使う。
//! フォントはバイナリに埋め込む（静的フォント、OFL。`fonts/LICENSE-gen-interface-jp`）。
//! 使う太さ（NORMAL / MEDIUM / BOLD）だけを積む。ほかの太さを使うなら同じ配布物から足す。
//! 数字を桁揃えで見せる計測表示には等幅の 0xProto（OFL。`fonts/LICENSE-0xproto`）の Regular だけを使う。
//! 飾りの書体（時計の文字盤など）は [`serif`]（DM Serif Display）と [`segment`]（DSEG7 Classic）。どちらも OFL で、
//! 画面の本文には使わない。足すときも OFL のものに限り、`fonts/` にライセンスを置く。

use std::borrow::Cow;

use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight};

pub const FAMILY: &str = "Gen Interface JP";
pub const MONO_FAMILY: &str = "0xProto";
pub const SERIF_FAMILY: &str = "DM Serif Display";
pub const SEGMENT_FAMILY: &str = "DSEG7 Classic";

/// フォントを GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Medium.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Bold.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/0xProto-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/DMSerifDisplay-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/DSEG7Classic-Bold.ttf").as_slice()),
        ])
        .expect("embedded fonts are valid");
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

/// セリフの見出し書体（DM Serif Display。OFL、`fonts/LICENSE-dm-serif-display`）。欧文と数字だけ。
pub fn serif() -> Font {
    with_fallback(SERIF_FAMILY, FontWeight::NORMAL)
}

/// 7 セグメントの液晶の数字（DSEG7 Classic の Bold。OFL、`fonts/LICENSE-dseg`）。数字と `:` を同じ幅で出す。
pub fn segment() -> Font {
    with_fallback(SEGMENT_FAMILY, FontWeight::BOLD)
}
