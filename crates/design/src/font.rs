//! 文字。Gen Interface JP（Inter + Noto Sans JP 系の欧文・和文を一つにまとめた書体）を使う。
//! フォントはバイナリに埋め込む（静的フォント、OFL。`fonts/LICENSE-gen-interface-jp`）。
//! 使う太さ（NORMAL / MEDIUM / BOLD）だけを積む。ほかの太さを使うなら同じ配布物から足す。
//! 数字を桁揃えで見せる計測表示には等幅の 0xProto（OFL。`fonts/LICENSE-0xproto`）の Regular だけを使う。

use std::borrow::Cow;

use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight};

pub const FAMILY: &str = "Gen Interface JP";
pub const MONO_FAMILY: &str = "0xProto";

/// フォントを GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Medium.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Bold.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/0xProto-Regular.ttf").as_slice()),
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

/// 等幅の書体（計測表示など）。和文は持たないので既定の書体へ回す
/// （Windows はシステムの書体からしか回せず、和文はシステムの書体で出る）。
pub fn mono() -> Font {
    Font {
        family: MONO_FAMILY.into(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![FAMILY.into()])),
        ..font()
    }
}
