//! パーソナルカラー: アクセントの塗り（ボタンなどの `accent-solid`、そのホバー・押下、上に載る `on-accent`）をユーザーの色にする。
//! スケールのほかの段（色付きの背景・枠・文字）は変えない。塗りは選んだ色のまま使い、上に載る色とホバー・押下の色が
//! それに従う（決め方は AGENT.md の「パーソナルカラー」）。

use palette::{Clamp, FromColor, LinSrgb, Oklch, Srgb};

use crate::{Mode, Theme, scale};

/// アクセントの塗りの色（OKLCH。明るさ 0〜1、鮮やかさ 0〜[`Accent::MAX_CHROMA`]、色相は度）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accent {
    pub lightness: f32,
    pub chroma: f32,
    pub hue: f32,
}

impl Accent {
    /// tokens.json の `accent-solid`（青）。
    pub const DEFAULT: Accent = Accent::new(0.6493, 0.193, 251.78);
    /// 鮮やかさの上限（sRGB で出せる最も鮮やかな色のあたり）。
    pub const MAX_CHROMA: f32 = 0.3;

    /// 用意した色（名前は i18n の `color.<名前>`）。先頭が既定。選ぶ画面に並ぶ履歴（[`Accent::history`]）の最初の中身。
    pub const PRESETS: [(&'static str, Accent); 8] = [
        ("blue", Accent::DEFAULT),
        ("purple", Accent::new(0.6, 0.2, 300.0)),
        ("pink", Accent::new(0.66, 0.21, 350.0)),
        ("red", Accent::new(0.63, 0.21, 25.0)),
        ("orange", Accent::new(0.72, 0.18, 55.0)),
        ("yellow", Accent::new(0.87, 0.18, 95.0)),
        ("green", Accent::new(0.68, 0.17, 150.0)),
        ("teal", Accent::new(0.7, 0.12, 195.0)),
    ];

    pub const fn new(lightness: f32, chroma: f32, hue: f32) -> Accent {
        Accent {
            lightness,
            chroma,
            hue,
        }
    }

    /// 選ぶ画面に並べる履歴（[`Accent::PRESETS`] と同じ数）: 自分で作った色 `custom`（新しい順）の後ろを、用意した色で埋める。
    /// 同じ色は 1 つだけ。作っていなければ用意した色そのもの。今の色 `current` が並びに無ければ（履歴が無いころに作った色）先頭に置く。
    pub fn history(current: Accent, custom: impl IntoIterator<Item = Accent>) -> Vec<Accent> {
        let len = Accent::PRESETS.len();
        let presets = Accent::PRESETS.map(|(_, a)| a);
        let mut row: Vec<Accent> = Vec::with_capacity(len + 1);
        for a in custom.into_iter().chain(presets) {
            if row.len() < len && !row.contains(&a) {
                row.push(a);
            }
        }
        if !row.contains(&current) {
            row.insert(0, current);
            row.truncate(len);
        }
        row
    }

    /// 用意した色ならその名前（[`Accent::PRESETS`]）。
    pub fn name(self) -> Option<&'static str> {
        let found = Accent::PRESETS.iter().find(|(_, a)| *a == self);
        found.map(|(name, _)| *name)
    }

    /// 色の表記（`#RRGGBB`）。名前の無い色を、文字でも伝えるのに使う。
    pub fn hex(self) -> String {
        format!("#{:06X}", self.color() >> 8)
    }

    /// この色そのもの（0xRRGGBBAA。画面に出せない鮮やかさは落とす）。塗りにはこれをそのまま使う。
    pub fn color(self) -> u32 {
        let a = self.clamped();
        pack(in_gamut(a.lightness, a.chroma, a.hue))
    }

    /// 範囲に収める（色相は 0〜360 に巻く。数でない値は既定の値）。
    pub fn clamped(self) -> Accent {
        let or = |v: f32, default| if v.is_finite() { v } else { default };
        let d = Accent::DEFAULT;
        Accent {
            lightness: or(self.lightness, d.lightness).clamp(0.0, 1.0),
            chroma: or(self.chroma, d.chroma).clamp(0.0, Accent::MAX_CHROMA),
            hue: or(self.hue, d.hue).rem_euclid(360.0),
        }
    }
}

impl Default for Accent {
    fn default() -> Self {
        Accent::DEFAULT
    }
}

/// アクセントの塗りの 3 つの状態と、その上に載る色（0xRRGGBBAA）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub solid: u32,
    pub hover: u32,
    pub active: u32,
    pub on: u32,
}

/// 塗りの上の文字に要るコントラスト比（3）と、状態どうしを見分ける比（1.05）。
/// 8 ビットに丸めても割らないよう、どちらも少し余裕を持たせる。
const CLEAR: f32 = 3.1;
const DISTINCT: f32 = 1.06;

/// `mode` のテーマで、塗りを `accent` にしたときの色。塗りは選んだ色そのまま（色域に収めるだけ）で、ほかがそれに合わせる。
pub fn fill(mode: Mode, accent: Accent) -> Fill {
    let base = Theme::of(mode);
    let a = accent.clamped();
    // 輝度 + 0.05 どうしの比が WCAG のコントラスト比なので、足した値のまま扱う。
    let lit = |c: Srgb| luminance(c.into_linear()) + 0.05;
    let token = |c: u32| lit(Srgb::<u8>::from(c >> 8).into_format());
    let picked = in_gamut(a.lightness, a.chroma, a.hue);
    let solid = lit(picked);
    // ホバー・押下は、既定の塗りと同じ比だけ暗くする。黒より暗くはできないので、暗い塗りでは同じ比だけ明るくする。
    let hover_ratio = token(base.accent_solid) / token(base.accent_solid_hover);
    let active_ratio = token(base.accent_solid) / token(base.accent_solid_active);
    let darker = solid / active_ratio >= 0.05 * DISTINCT;
    let step = |ratio: f32| if darker { solid / ratio } else { solid * ratio };
    let (mut hover, mut active) = (step(hover_ratio), step(active_ratio));
    // 上に載る色は、3 つの状態のどれでも読める方。白で読めれば白（既定と同じ）、読めなければ黒。
    let white = 1.05 / solid.max(active) >= CLEAR;
    if !white {
        // 黒が載る（塗りは明るく、ホバー・押下はそれより暗い）。黒が読める明るさより暗くはしない。
        active = active.max(0.05 * CLEAR);
        hover = hover.max(active * DISTINCT);
    }
    // 押下の色がスクロールバーのつまみ（`track`）と同じ明るさになるなら、塗りから離れる側へ避ける。
    let track = token(base.track);
    if active.max(track) / active.min(track) < DISTINCT {
        active = if darker {
            track / DISTINCT
        } else {
            track * DISTINCT
        };
    }
    Fill {
        solid: pack(picked),
        hover: paint(hover - 0.05, a.chroma, a.hue),
        active: paint(active - 0.05, a.chroma, a.hue),
        on: if white { scale::WHITE } else { scale::BLACK },
    }
}

fn pack(c: Srgb) -> u32 {
    let c: Srgb<u8> = c.clamp().into_format();
    u32::from(c.red) << 24 | u32::from(c.green) << 16 | u32::from(c.blue) << 8 | 0xff
}

/// WCAG の相対輝度。
fn luminance(c: LinSrgb) -> f32 {
    0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
}

/// 8 ビットに丸めて変わらない範囲なら、色域の内とみなす。
fn fits(c: Srgb) -> bool {
    [c.red, c.green, c.blue]
        .iter()
        .all(|v| (-0.002..=1.002).contains(v))
}

/// 明るさ `l`・色相 `h` のまま、色域に収まるまで鮮やかさ `c` を落とした色。
fn in_gamut(l: f32, mut c: f32, h: f32) -> Srgb {
    loop {
        let rgb = Srgb::from_color(Oklch::new(l, c, h));
        if fits(rgb) || c < 0.001 {
            return rgb.clamp();
        }
        c *= 0.9;
    }
}

/// 輝度 `y`・鮮やかさ `c`・色相 `h` の色（0xRRGGBBAA）。色域に収まるまで鮮やかさを落とす。
fn paint(y: f32, mut c: f32, h: f32) -> u32 {
    loop {
        // 輝度は明るさについて単調に増えるので、目当ての輝度になる明るさを二分探索する。
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            if luminance(LinSrgb::from_color(Oklch::new(mid, c, h))) < y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let rgb = Srgb::from_color(Oklch::new(hi, c, h));
        if fits(rgb) || c < 0.001 {
            return pack(rgb);
        }
        c *= 0.9;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use palette::color_difference::Wcag21RelativeContrast;

    fn contrast(a: u32, b: u32) -> f32 {
        let c = |c: u32| -> Srgb { Srgb::<u8>::from(c >> 8).into_format() };
        c(a).relative_contrast(c(b))
    }

    fn near(a: u32, b: u32) -> bool {
        let (a, b) = (a.to_be_bytes(), b.to_be_bytes());
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    /// どんな色を選んでも、上に載る色は 3 つの状態のどれでも読め、状態どうしも見分けられる。
    #[test]
    fn every_color_keeps_the_contrast_rules() {
        let presets = Accent::PRESETS.map(|(_, a)| a);
        let sweep = (0..24).flat_map(|h| {
            [0.0, 0.1, 0.2, 0.3].into_iter().flat_map(move |c| {
                [0.0, 0.2, 0.4, 0.55, 0.65, 0.75, 0.85, 0.95, 1.0]
                    .map(|l| Accent::new(l, c, h as f32 * 15.0))
            })
        });
        for accent in presets.into_iter().chain(sweep) {
            for mode in [Mode::Dark, Mode::Light] {
                let (t, f) = (Theme::of(mode), fill(mode, accent));
                for (name, fg, bg, min) in [
                    ("on / solid", f.on, f.solid, 3.0),
                    ("on / hover", f.on, f.hover, 3.0),
                    ("on / active", f.on, f.active, 3.0),
                    ("solid / hover", f.solid, f.hover, 1.05),
                    ("hover / active", f.hover, f.active, 1.05),
                    ("solid / active", f.solid, f.active, 1.05),
                    ("active / track", f.active, t.track, 1.05),
                ] {
                    let got = contrast(fg, bg);
                    assert!(got >= min, "{accent:?} {mode:?} {name}: {got} < {min}");
                }
            }
        }
    }

    /// 既定の色は tokens.json の `accent-solid` そのもの（定数を写しているので、ずれたらここで落ちる）。
    #[test]
    fn default_is_the_token_accent() {
        for mode in [Mode::Dark, Mode::Light] {
            let (t, f) = (Theme::of(mode), fill(mode, Accent::DEFAULT));
            assert!(
                near(f.solid, t.accent_solid),
                "{:08x} {:08x}",
                f.solid,
                t.accent_solid
            );
            assert_eq!(f.on, t.on_accent);
            // ホバー・押下は既定と同じ明るさ（色味は塗りに合わせるので、色そのものは少し違う）。
            for (made, token) in [
                (f.hover, t.accent_solid_hover),
                (f.active, t.accent_solid_active),
            ] {
                assert!((contrast(made, token) - 1.0).abs() < 0.03);
            }
        }
    }

    /// 選んだ色はどのテーマでもそのまま塗りになる。明るい色には黒、暗い色には白が載る。
    #[test]
    fn picked_color_is_kept_and_the_text_on_it_flips() {
        let preset = |name: &str| Accent::PRESETS.iter().find(|(n, _)| *n == name).unwrap().1;
        let yellow = fill(Mode::Dark, preset("yellow"));
        let [r, g, b, _] = yellow.solid.to_be_bytes();
        assert!(
            r > 200 && g > 180 && b < 120,
            "明るい黄色のまま: {r} {g} {b}"
        );
        assert_eq!(yellow.on, scale::BLACK);
        let purple = fill(Mode::Dark, preset("purple"));
        assert_eq!(purple.on, scale::WHITE);
        // ライトでも塗りは変えない（白いカードの上でも黄色は黄色のまま、黒が載る）。
        let light = fill(Mode::Light, preset("yellow"));
        assert_eq!((light.solid, light.on), (yellow.solid, scale::BLACK));
        // 黒に近い塗りは、ホバー・押下で明るくなる（黒より暗くはできない）。
        let ink = fill(Mode::Dark, Accent::new(0.15, 0.0, 0.0));
        assert_eq!(ink.on, scale::WHITE);
        assert!(contrast(ink.active, scale::BLACK) > contrast(ink.solid, scale::BLACK));
    }

    /// 履歴の最初の中身は用意した色。作った色は先頭に入り、用意した色が後ろから落ちる。
    #[test]
    fn history_starts_as_the_presets_and_takes_custom_colors_first() {
        let presets = Accent::PRESETS.map(|(_, a)| a);
        assert_eq!(Accent::history(Accent::DEFAULT, []), presets);
        let (mine, older) = (Accent::new(0.5, 0.1, 10.0), Accent::new(0.4, 0.1, 20.0));
        // 用意した色と同じ色を作っても、2 つは並ばない。
        let row = Accent::history(mine, [mine, older, presets[2]]);
        assert_eq!(row.len(), presets.len());
        assert_eq!(row[..5], [mine, older, presets[2], presets[0], presets[1]]);
        assert!(!row.contains(&presets[7]), "後ろから落ちる");
        // 並びに無い今の色（履歴が無いころに作った色）は先頭に見せる。
        let row = Accent::history(mine, []);
        assert_eq!(
            (row[0], row[1], row.len()),
            (mine, presets[0], presets.len())
        );
        assert_eq!(presets[2].name(), Some("pink"));
        assert_eq!(mine.name(), None);
        let hex = Accent::new(1.0, 0.0, 0.0).hex();
        assert_eq!(hex, "#FFFFFF");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let a = Accent::new(2.0, 9.0, -30.0).clamped();
        assert_eq!(a, Accent::new(1.0, Accent::MAX_CHROMA, 330.0));
        let nan = Accent::new(f32::NAN, f32::NAN, f32::NAN);
        assert_eq!(nan.clamped(), Accent::DEFAULT);
    }
}
