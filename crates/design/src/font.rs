//! 文字。Gen Interface JP（Inter + Noto Sans JP 系の欧文・和文を一つにまとめた書体）を使う。
//! フォントはバイナリに埋め込む（静的フォント、OFL。`fonts/LICENSE-gen-interface-jp`）。
//! 使う太さ（NORMAL / MEDIUM / BOLD）だけを積む。ほかの太さを使うなら同じ配布物から足す。

use std::borrow::Cow;

use gpui::{App, Font, FontFeatures, FontStyle, FontWeight};

pub const FAMILY: &str = "Gen Interface JP";

/// フォントを GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Regular.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Medium.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/GenInterfaceJP-Bold.ttf").as_slice()),
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
