//! 文字。Noto Sans をプライマリ、Noto Sans JP をセカンダリ（かな・漢字など Noto Sans に無い字のフォールバック）にする。
//! フォントはバイナリに埋め込む（可変フォント、OFL。`fonts/LICENSE-noto`）。

use std::borrow::Cow;

use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight};

pub const PRIMARY: &str = "Noto Sans";
pub const SECONDARY: &str = "Noto Sans JP";

/// フォントを GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/NotoSans.ttf").as_slice()),
            Cow::Borrowed(include_bytes!("../fonts/NotoSansJP.ttf").as_slice()),
        ])
        .expect("embedded fonts are valid");
}

/// 既定の書体（プライマリ + セカンダリのフォールバック）。
pub fn font() -> Font {
    Font {
        family: PRIMARY.into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![SECONDARY.into()])),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}
