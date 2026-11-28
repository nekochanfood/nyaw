//! デザイン層。UI の見た目の責務を 3 層に分離する。
//!
//! 1. **Constants**（[`scale`] / [`radius`] / [`spacing`] / [`font_size`] / [`letter_spacing`] / [`duration_ms`]）:
//!    生の値のみ。色の用途は持たない。個性（スケール・角丸・字間）はここと [`components`] の装飾で表現する。
//! 2. **Utilities**（[`Theme`]）: 役割（Radix Colors に倣い、`accent` → `blue` のようなスケールの別名、`accent-solid` のような段の用途名、`panel` のようなテーマで変わる名前）→ 色。部品は役割のみを参照する。
//!    役割の組のコントラスト比は build.rs が検証するため、スケールを変更しても可読性を確保できない組み合わせは作成できない。
//!
//! アニメーション（[`animation`]）: 出現・消滅・移動の 3 種の [`animation::Motion`] と、値を時間で遷移させる [`animation::Animation`]。実体は `animation` クレート。
//! 3. **Components**（[`components`]）: ボタン・スイッチ・スライダー。
//!
//! アイコンは Phosphor を [`icon`] モジュールで path として描画する（使用するもののみをビルドに含める）。
//!
//! 値は `tokens.json` が唯一の情報源（色は Radix Colors を OKLCH に変換したもの）で、build.rs がこのクレートのコードに変換する。
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
pub mod link;
#[cfg(feature = "gpui")]
pub mod logo;
#[cfg(feature = "gpui")]
pub mod lottie;
#[cfg(feature = "gpui")]
pub mod spot;

#[cfg(feature = "gpui")]
pub use emoji::{emoji, emoji_text};

#[cfg(feature = "gpui")]
pub use icon::{Icon, Weight, icon};

#[cfg(feature = "gpui")]
pub use logo::{Logo, logo};
#[cfg(feature = "gpui")]
pub use lottie::intro_logo;

#[cfg(feature = "gpui")]
include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// パネルの影のぼかし（ガウスの σ）。オフセットなしで四方へ広がる影により、重なったパネル間の境界を示す。
pub const PANEL_SHADOW_BLUR: f32 = 8.0;
/// パネル背景をウィンドウ端から離す幅。影はパネルのテクスチャ内に描画するため、この範囲に収める。
/// GPUI は影を σ の 3 倍まで描画するため、同値にして影が途切れないようにする。
/// オーバーレイ層もナビゲーションバーの配置に使用するため、GPUI を持たない値の側に定義する。
pub const PANEL_MARGIN: f32 = 3.0 * PANEL_SHADOW_BLUR;
/// 角枠（beam の `bracket`。はまる所の見本などが使う、角丸矩形の隅だけを囲む線）の太さ（px）。
pub const BRACKET_LINE_PX: f32 = 6.0;
/// パネル面から角枠の外側までの幅（px）。面と線の間に、線の太さの 2 倍の隙間を空ける。
/// オーバーレイ層が見本の大きさの決定に使用するため、GPUI を持たない値の側に定義する。
pub const BRACKET_PAD_PX: f32 = 3.0 * BRACKET_LINE_PX;
/// 角を囲む線（パネルの角を掴んで大きさを変えるときの目印）の窓の一辺（px）。オーバーレイ層は窓の中心をパネル面の角に置き、
/// 窓 1px がパネル 1px になる大きさにする。Windows の非表示ウィンドウの最小サイズ制約があるため、これより小さくしない。
pub const CORNER_HANDLE_PX: u32 = 240;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
