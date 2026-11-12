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
mod accent;
pub use accent::{Accent, Fill, fill};
#[cfg(feature = "gpui")]
pub mod catalog;
#[cfg(feature = "gpui")]
pub mod components;
#[cfg(feature = "gpui")]
pub mod corner;
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
pub mod logo;

#[cfg(feature = "gpui")]
pub use emoji::{emoji, emoji_text};

#[cfg(feature = "gpui")]
pub use icon::{Icon, Weight, icon};

#[cfg(feature = "gpui")]
pub use logo::{Logo, logo};

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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

/// 今のアクセントの塗りの色（GPUI のグローバル）。[`Mode`] と同じく `ui` が書く。置かれていなければ既定（青）。
#[cfg(feature = "gpui")]
impl gpui::Global for Accent {}

/// テーマの切り替えの移り変わり: 切り替えた時点の色 `from` から今の [`Mode`] と [`Accent`] のテーマへ `start` から動いている。
#[cfg(feature = "gpui")]
struct ThemeFade {
    from: Theme,
    start: std::time::Instant,
}

#[cfg(feature = "gpui")]
impl gpui::Global for ThemeFade {}

#[cfg(feature = "gpui")]
impl Theme {
    /// 画面が描くときの今のテーマ。テーマの切り替え中は前の色から補間した色になるので、`render` で毎回引く。
    pub fn current(cx: &gpui::App) -> Theme {
        let to = Theme::with(
            cx.try_global::<Mode>().copied().unwrap_or_default(),
            cx.try_global::<Accent>().copied().unwrap_or_default(),
        );
        match Theme::fade_progress(cx) {
            Some(t) => cx.global::<ThemeFade>().from.zip(to, |a, b| {
                components::mix(a, b, animation::Motion::Enter.ease(t))
            }),
            None => to,
        }
    }

    /// テーマを `mode` に切り替える。[`Motion`] があれば、どの役割の色も今の色から [`duration_ms::SLOW`] かけて移る。
    pub fn set_mode(mode: Mode, cx: &mut gpui::App) {
        if cx.try_global::<Mode>().copied().unwrap_or_default() == mode {
            return;
        }
        Theme::begin_fade(cx);
        cx.set_global(mode);
    }

    /// アクセントの塗りを `accent` に切り替える（[`Self::set_mode`] と同じく、今の色から移る）。
    pub fn set_accent(accent: Accent, cx: &mut gpui::App) {
        if cx.try_global::<Accent>().copied().unwrap_or_default() == accent {
            return;
        }
        Theme::begin_fade(cx);
        cx.set_global(accent);
    }

    /// 今の色を起点に、移り変わりを始める（[`Motion`] が無ければ即座に切り替わる）。
    fn begin_fade(cx: &mut gpui::App) {
        if cx.has_global::<Motion>() {
            let from = Theme::current(cx);
            cx.set_global(ThemeFade {
                from,
                start: drive::now(),
            });
        }
    }

    /// テーマが移り変わっている途中か。フレームループはこの間、全パネルを毎フレーム描き直す。
    pub fn fading(cx: &gpui::App) -> bool {
        Theme::fade_progress(cx).is_some()
    }

    /// 切り替えの進み具合（0..1）。落ち着いていれば `None`。
    fn fade_progress(cx: &gpui::App) -> Option<f32> {
        let fade = cx.try_global::<ThemeFade>()?;
        let t = drive::now()
            .saturating_duration_since(fade.start)
            .as_secs_f32()
            * 1000.0
            / duration_ms::SLOW as f32;
        (t < 1.0).then_some(t)
    }
}

impl Theme {
    pub const fn of(mode: Mode) -> Theme {
        match mode {
            Mode::Dark => Theme::DARK,
            Mode::Light => Theme::LIGHT,
        }
    }

    /// `mode` のテーマの、アクセントの塗りと上に載る色を `accent` にしたもの（[`fill`]）。毎回の描画で引けるよう、作ったものは覚えておく。
    pub fn with(mode: Mode, accent: Accent) -> Theme {
        // 直前の色の（ダーク, ライト）。画面は描くたびに同じ色で引くので、1 つ覚えれば足りる。
        thread_local! {
            static LAST: std::cell::Cell<Option<(Accent, [Theme; 2])>> = const { std::cell::Cell::new(None) };
        }
        if accent == Accent::DEFAULT {
            return Theme::of(mode);
        }
        let make = |m| {
            let f = fill(m, accent);
            Theme {
                accent_solid: f.solid,
                accent_solid_hover: f.hover,
                accent_solid_active: f.active,
                on_accent: f.on,
                ..Theme::of(m)
            }
        };
        let themes = match LAST.get() {
            Some((a, themes)) if a == accent => themes,
            _ => {
                let themes = [make(Mode::Dark), make(Mode::Light)];
                LAST.set(Some((accent, themes)));
                themes
            }
        };
        themes[usize::from(mode == Mode::Light)]
    }

    /// カードの上に直接置く中身のためのテーマ。地の上に置く前提で組んだ部品（設定の部品など）を、地の色の枠で囲まずに
    /// カードの上へ置けるよう、面の明るさを 1 段ずらす: 地 → カード、カード → `control`（カードより少し暗い）。
    /// その中の部品と、面に直接置く部品の色（`control-*` と `control-on-background-*`）は入れ替わる。
    pub fn on_card(self) -> Theme {
        Theme {
            background: self.card,
            card: self.control,
            control: self.control_on_background,
            control_hover: self.control_on_background_hover,
            control_active: self.control_on_background_active,
            control_on_background: self.control,
            control_on_background_hover: self.control_hover,
            control_on_background_active: self.control_active,
            // 溝は、新しいカード（`control`）とも、面（カード）とも見分けられる色に。
            groove: self.control_on_background,
            ..self
        }
    }

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

    #[cfg(feature = "gpui")]
    #[gpui::test]
    fn set_mode_fades_every_role(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            Theme::set_mode(Mode::Light, cx);
            assert_eq!(Theme::current(cx), Theme::LIGHT, "Motion が無ければ即座に");
            cx.set_global(Motion::default());
            Theme::set_mode(Mode::Dark, cx);
            assert!(Theme::fading(cx));
            drive::advance(std::time::Duration::from_millis(duration_ms::SLOW / 2));
            let mid = Theme::current(cx);
            assert_ne!(mid.background, Theme::LIGHT.background);
            assert_ne!(mid.background, Theme::DARK.background);
            drive::advance(std::time::Duration::from_millis(duration_ms::SLOW));
            assert!(!Theme::fading(cx));
            assert_eq!(Theme::current(cx), Theme::DARK);
        });
    }

    #[cfg(feature = "gpui")]
    #[gpui::test]
    fn set_accent_retints_and_fades(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let (_, red) = Accent::PRESETS[3];
            Theme::set_accent(red, cx);
            let now = Theme::current(cx);
            assert_eq!(now, Theme::with(Mode::Dark, red), "Motion が無ければ即座に");
            assert_ne!(now.accent_solid, Theme::DARK.accent_solid);
            assert_eq!(now.accent_text, Theme::DARK.accent_text, "塗りのほかは同じ");
            cx.set_global(Mode::Light);
            assert_eq!(Theme::current(cx), Theme::with(Mode::Light, red));
            cx.set_global(Motion::default());
            Theme::set_accent(Accent::DEFAULT, cx);
            assert!(Theme::fading(cx));
            drive::advance(std::time::Duration::from_millis(duration_ms::SLOW));
            assert_eq!(Theme::current(cx), Theme::LIGHT);
        });
    }

    #[test]
    fn with_default_accent_is_the_token_theme_and_repeats() {
        assert_eq!(Theme::with(Mode::Dark, Accent::DEFAULT), Theme::DARK);
        let (_, pink) = Accent::PRESETS[2];
        let first = Theme::with(Mode::Light, pink);
        // 覚えているのは直前の色だけ。ほかの色を挟んでも、同じ色は同じテーマになる。
        assert_ne!(Theme::with(Mode::Light, Accent::PRESETS[5].1), first);
        assert_eq!(Theme::with(Mode::Light, pink), first);
        assert_ne!(first.accent_solid, Theme::LIGHT.accent_solid);
        // 塗りと上に載る色のほかは変えない。
        let rest = Theme {
            accent_solid: Theme::LIGHT.accent_solid,
            accent_solid_hover: Theme::LIGHT.accent_solid_hover,
            accent_solid_active: Theme::LIGHT.accent_solid_active,
            on_accent: Theme::LIGHT.on_accent,
            ..first
        };
        assert_eq!(rest, Theme::LIGHT);
    }

    /// カードの上に置く中身のテーマ（`on_card`）でも、隣り合う面と部品は見分けられ、文字は読める。
    #[test]
    fn on_card_keeps_surfaces_distinct_and_text_readable() {
        use palette::{Srgb, color_difference::Wcag21RelativeContrast};
        let contrast = |a: u32, b: u32| {
            let c = |c: u32| -> Srgb { Srgb::<u8>::from(c >> 8).into_format() };
            c(a).relative_contrast(c(b))
        };
        for base in [Theme::DARK, Theme::LIGHT] {
            let t = base.on_card();
            assert_eq!((t.background, t.card), (base.card, base.control));
            for (name, a, b, min) in [
                ("card / surface", t.card, t.background, 1.05),
                ("control / card", t.control, t.card, 1.05),
                ("control hover / card", t.control_hover, t.card, 1.05),
                ("control active / card", t.control_active, t.card, 1.05),
                ("control / hover", t.control, t.control_hover, 1.05),
                ("hover / active", t.control_hover, t.control_active, 1.05),
                (
                    "on surface / surface",
                    t.control_on_background,
                    t.background,
                    1.05,
                ),
                (
                    "on surface hover / surface",
                    t.control_on_background_hover,
                    t.background,
                    1.05,
                ),
                ("groove / card", t.groove, t.card, 1.05),
                ("groove / surface", t.groove, t.background, 1.05),
                ("text / surface", t.neutral_text, t.background, 7.0),
                ("text / card", t.neutral_text, t.card, 7.0),
                ("text low / surface", t.neutral_text_low, t.background, 4.5),
                ("text low / card", t.neutral_text_low, t.card, 4.5),
            ] {
                let got = contrast(a, b);
                assert!(got >= min, "{name}: {got} < {min}");
            }
        }
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
