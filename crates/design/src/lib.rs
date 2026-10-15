//! デザイン層。UI の見た目の責務を 3 層に分ける。
//!
//! 1. **Constants**（[`palette`] / [`radius`] / [`spacing`] / [`font_size`] / [`letter_spacing`] / [`duration_ms`]）:
//!    生の値だけ。色の用途は持たない。個性（パレット・角丸・字間）はここと [`components`] の装飾で出す。
//! 2. **Utilities**（[`Theme`]）: 役割（`primary` / `on_primary` ...）→ パレットの色。部品は役割だけを見る。
//!    役割の組のコントラスト比は build.rs が検証するので、パレットを変えても読めない組み合わせは作れない。
//! 3. **Components**（[`components`]）: ボタン・スイッチ・スライダー。
//!
//! 値は `tokens.json` が唯一の情報源（色は OKLCH）で、build.rs がこのクレートのコードにする。
//! Web 側も同じ JSON を読めば値を共有できる。色は `0xRRGGBBAA`（`gpui::rgba` にそのまま渡せる）。

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

pub mod components;

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
        assert_ne!(Theme::DARK.surface, Theme::LIGHT.surface);
        assert_eq!(Theme::DARK.on_primary & 0xff, 0xff);
    }

    #[test]
    fn mode_picks_theme() {
        assert_eq!(Theme::of(Mode::Dark), Theme::DARK);
        assert_eq!(Theme::of(Mode::Dark.toggled()), Theme::LIGHT);
    }

    #[test]
    fn constants_come_from_tokens() {
        assert_eq!(palette::GRAY_0, 0xffffffff);
        assert_eq!(radius::MD, 8.0);
        assert_eq!(duration_ms::NORMAL, 200);
    }
}
