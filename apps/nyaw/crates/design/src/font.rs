//! フォント。Gen Interface JP（Inter + Noto Sans JP 系の欧文・和文を統合した書体）を使用する。
//! フォントはバイナリに埋め込む（静的フォント、OFL。`fonts/LICENSE-gen-interface-jp`）。
//! 使用するウェイト（NORMAL / MEDIUM / BOLD）のみを同梱する。他のウェイトが必要なら同一配布物から追加する。
//! 数字を桁揃えで表示する計測表示には等幅の 0xProto（OFL。`fonts/LICENSE-0xproto`）の Regular のみを使用する。
//! 装飾用書体（時計の文字盤等）は [`serif`]（Vercel の Geist Serif）と [`segment`]（DSEG7 Classic）。いずれも OFL で、
//! 画面の本文には使用しない。追加する際も OFL のものに限り、`fonts/` にライセンスを配置する。

use std::borrow::Cow;

use gpui::{
    App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, TextRun,
    Window, px,
};

pub const FAMILY: &str = "Gen Interface JP";
pub const MONO_FAMILY: &str = "0xProto";
pub const SERIF_FAMILY: &str = "Geist Serif";
pub const SEGMENT_FAMILY: &str = "DSEG7 Classic";

/// フォントを GPUI に登録する。ウィンドウ作成前に一度呼び出す。
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

/// 文字サイズの倍率と太字化をグローバルに適用する（`ui` が全体設定から呼び出す）。適用先は GPUI のテキストシステム（組版・
/// 書体選択）のため、画面のコードは等倍の大きさ・太さを記述するだけでよく、適用漏れは発生しない。除外対象は
/// `.text_exact()` を付与した部分木のみ（画像内の文字）。変更があれば全ウィンドウを再描画し、`true` を返す。AGENT.md の「文字の大きさと太字化」。
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

/// 文字に合わせて決めた長さ `length`（px。行高由来のボックス等）を、現在の文字サイズ倍率に合わせる。
/// 文字自体には不要（テキストシステムが適用する）。文字に追従すべき非テキスト寸法にのみ使用する。
pub fn scaled(length: f32) -> Pixels {
    px(length * gpui::text_scale())
}

/// `text` を既定の書体の `size` px・太さ `weight` で 1 行に組版したときの幅。
/// 組版はテキストシステムが行うため、文字サイズ倍率・太字化が適用された画面表示どおりの幅を返す。
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

/// 既定の書体へフォールバックする書体（未収録のグリフは既定の書体で描画する。Windows はシステム書体からのみフォールバック可能）。
fn with_fallback(family: &'static str, weight: FontWeight) -> Font {
    Font {
        family: family.into(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![FAMILY.into()])),
        weight,
        ..font()
    }
}

/// 等幅書体（計測表示等）。和文グリフは未収録。
pub fn mono() -> Font {
    with_fallback(MONO_FAMILY, FontWeight::NORMAL)
}

/// セリフ見出し書体（Vercel の Geist Serif。OFL、`fonts/LICENSE-geist`）。欧文と数字のみ。
pub fn serif() -> Font {
    with_fallback(SERIF_FAMILY, FontWeight::NORMAL)
}

/// 7 セグメント液晶数字書体（DSEG7 Classic の Bold。OFL、`fonts/LICENSE-dseg`）。数字と `:` を等幅で描画する。
pub fn segment() -> Font {
    with_fallback(SEGMENT_FAMILY, FontWeight::BOLD)
}
