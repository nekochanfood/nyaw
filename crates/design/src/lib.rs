//! デザイン層。UI の見た目の責務を 3 層に分ける。
//!
//! 1. **Constants**（[`scale`] / [`radius`] / [`spacing`] / [`font_size`] / [`letter_spacing`] / [`duration_ms`]）:
//!    生の値だけ。色の用途は持たない。個性（スケール・角丸・字間）はここと [`components`] の装飾で出す。
//! 2. **Utilities**（[`Theme`]）: 役割（Radix Colors に倣い、`accent` → `blue` のようなスケールの別名、`accent-solid` のような段の用途名、`panel` のようなテーマで変わる名前）→ 色。部品は役割だけを見る。
//!    役割の組のコントラスト比は build.rs が検証するので、スケールを変えても読めない組み合わせは作れない。
//!
//! 動き（[`animation`]）: 出る・消える・動かすの 3 種の [`animation::Motion`] と、値を時間で動かす [`animation::Animation`]。実体は `animation` クレート。
//! 3. **Components**（[`components`]）: ボタン・スイッチ・スライダー。
//!
//! アイコンは Phosphor を [`icon`] モジュールで path として描く（使うものだけをビルドに含める）。
//!
//! 値は `tokens.json` が唯一の情報源（色は Radix Colors を OKLCH にしたもの）で、build.rs がこのクレートのコードにする。
//! 色は `0xRRGGBBAA`（`gpui::rgba` にそのまま渡せる）。

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

pub use ::animation;
pub use animation::duration_ms;
#[cfg(feature = "gpui")]
pub mod catalog;
#[cfg(feature = "gpui")]
pub mod components;
#[cfg(feature = "gpui")]
pub mod cue;
#[cfg(feature = "gpui")]
pub mod drive;
#[cfg(feature = "gpui")]
pub mod emoji;
#[cfg(feature = "gpui")]
pub mod font;
#[cfg(feature = "gpui")]
pub mod icon;

#[cfg(feature = "gpui")]
pub use emoji::{emoji, emoji_text};

#[cfg(feature = "gpui")]
pub use icon::{Icon, Weight, icon};

#[cfg(feature = "gpui")]
include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// パネルの影のぼかし（ガウスの σ）。ずらさず四方へ広がる影で、重なったパネルどうしの境界を見せる。
pub const PANEL_SHADOW_BLUR: f32 = 8.0;
/// パネルの地をウィンドウの縁から離す幅。影はパネルのテクスチャの中に描くので、ここに収める。
/// GPUI は影を σ の 3 倍まで描くので、それと同じにして影が切れないようにする。
/// オーバーレイ層もナビゲーションバーの置き場所に使うので、GPUI を持たない値の側に置く。
pub const PANEL_MARGIN: f32 = 3.0 * PANEL_SHADOW_BLUR;

/// 部品のトランジションを有効にする GPUI のグローバル（置いたときだけ動く。テストは置かないので即座に切り替わる）。
/// 動いている間のウィンドウを `animating` に積む。フレームループ（`ui`）は、これを見て毎フレーム描き直し、読み出したら消す。
#[cfg(feature = "gpui")]
#[derive(Default)]
pub struct Motion {
    pub animating: std::collections::HashSet<gpui::WindowId>,
}

#[cfg(feature = "gpui")]
impl gpui::Global for Motion {}

/// ライトかダークか。`Theme::of(mode)` で役割の色を引く。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    pub fn toggled(self) -> Self {
        match self {
            Mode::Dark => Mode::Light,
            Mode::Light => Mode::Dark,
        }
    }
}

/// 今のテーマ（GPUI のグローバル）。`ui` のフレームループがカーネルの設定から書く。置かれていなければダーク。
#[cfg(feature = "gpui")]
impl gpui::Global for Mode {}

#[cfg(feature = "gpui")]
impl Theme {
    /// 画面が描くときの今のテーマ。テーマが切り替わるとウィンドウが描き直されるので、`render` で毎回引く。
    pub fn current(cx: &gpui::App) -> Theme {
        Theme::of(cx.try_global::<Mode>().copied().unwrap_or_default())
    }

    pub const fn of(mode: Mode) -> Theme {
        match mode {
            Mode::Dark => Theme::DARK,
            Mode::Light => Theme::LIGHT,
        }
    }
}

impl Theme {
    /// 白黒にしたテーマ（止まった画面など、もう操作できないものに使う）。
    pub fn grayscale(self) -> Theme {
        self.map(grayscale)
    }
}

/// 色（0xRRGGBBAA）を同じ明るさの灰色にする（Rec. 709 の重み。不透明度は保つ）。
pub fn grayscale(rgba: u32) -> u32 {
    let [r, g, b, a] = rgba.to_be_bytes();
    let y = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as u8;
    u32::from_be_bytes([y, y, y, a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_keeps_brightness_and_alpha() {
        assert_eq!(grayscale(0xffffff80), 0xffffff80);
        assert_eq!(grayscale(0x00000000), 0x00000000);
        let [r, g, b, a] = grayscale(0x3e63ddff).to_be_bytes();
        assert!(r == g && g == b && a == 0xff);
        let gray = Theme::DARK.grayscale();
        let [r, g, b, _] = gray.accent_solid.to_be_bytes();
        assert!(r == g && g == b, "アクセントも灰色になる");
    }

    #[test]
    fn themes_differ_but_share_roles() {
        assert_ne!(Theme::DARK.neutral_base, Theme::LIGHT.neutral_base);
        assert_eq!(Theme::DARK.on_accent & 0xff, 0xff);
    }

    #[cfg(feature = "gpui")]
    #[gpui::test]
    fn current_follows_the_global_mode(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(Theme::current(cx), Theme::DARK, "dark until set");
            cx.set_global(Mode::Light);
            assert_eq!(Theme::current(cx), Theme::LIGHT);
        });
    }

    #[test]
    fn mode_picks_theme() {
        assert_eq!(Theme::of(Mode::Dark), Theme::DARK);
        assert_eq!(Theme::of(Mode::Dark.toggled()), Theme::LIGHT);
    }

    #[test]
    fn constants_come_from_tokens() {
        assert_eq!(scale::WHITE, 0xffffffff);
        assert_eq!(radius::MD, 8.0);
        assert_eq!(duration_ms::NORMAL, 200);
    }
}
