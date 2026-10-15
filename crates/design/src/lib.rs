//! デザイン層。UI の見た目の責務を 3 層に分ける。
//!
//! 1. **Constants**（[`scale`] / [`radius`] / [`spacing`] / [`font_size`] / [`letter_spacing`] / [`duration_ms`]）:
//!    生の値だけ。色の用途は持たない。個性（スケール・角丸・字間）はここと [`components`] の装飾で出す。
//! 2. **Utilities**（[`Theme`]）: 役割（Radix Colors に倣い、`accent` → `blue` のようなスケールの別名、`accent-solid` のような段の用途名、`panel` のようなテーマで変わる名前）→ 色。部品は役割だけを見る。
//!    役割の組のコントラスト比は build.rs が検証するので、スケールを変えても読めない組み合わせは作れない。
//! 動き（[`animation`]）: 出る・消える・動かすの 3 種の [`animation::Motion`] と、値を時間で動かす [`animation::Animation`]。animato はここで隠す。
//! 3. **Components**（[`components`]）: ボタン・スイッチ・スライダー。
//!
//! アイコンは Phosphor を [`icon`] モジュールで path として描く（使うものだけをビルドに含める）。
//!
//! 値は `tokens.json` が唯一の情報源（色は Radix Colors を OKLCH にしたもの）で、build.rs がこのクレートのコードにする。
//! 色は `0xRRGGBBAA`（`gpui::rgba` にそのまま渡せる）。

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

pub mod animation;
pub mod components;
pub mod font;
pub mod icon;

pub use icon::{Icon, Weight, icon};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// 部品のトランジションを有効にする GPUI のグローバル（置いたときだけ動く。テストは置かないので即座に切り替わる）。
/// 動いている間のウィンドウを `animating` に積む。フレームループ（`ui`）は、これを見て毎フレーム描き直し、読み出したら消す。
#[derive(Default)]
pub struct Motion {
    pub animating: std::collections::HashSet<gpui::WindowId>,
}

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

impl Theme {
    pub const fn of(mode: Mode) -> Theme {
        match mode {
            Mode::Dark => Theme::DARK,
            Mode::Light => Theme::LIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_differ_but_share_roles() {
        assert_ne!(Theme::DARK.neutral_base, Theme::LIGHT.neutral_base);
        assert_eq!(Theme::DARK.on_accent & 0xff, 0xff);
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
