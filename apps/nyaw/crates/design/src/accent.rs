//! パーソナルカラー: アクセントの塗り（ボタン等の `accent-solid`、そのホバー・押下、上に載せる `on-accent`）をユーザー指定の色に差し替える。
//! スケールの他の段（色付きの背景・枠・文字）は変更しない。塗りは選択色をそのまま使用し、上に載せる色とホバー・押下の色は
//! それに追従する（算出方法は AGENT.md の「パーソナルカラー」を参照）。

use palette::{Clamp, FromColor, LinSrgb, Oklch, Srgb};

use crate::{Mode, Theme, scale};

/// アクセントの塗り色（OKLCH。明度 0〜1、彩度 0〜[`Accent::MAX_CHROMA`]、色相は度）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accent {
    pub lightness: f32,
    pub chroma: f32,
    pub hue: f32,
}

impl Accent {
    /// tokens.json における `accent-solid` の既定値（青）。
    pub const DEFAULT: Accent = Accent::new(0.6493, 0.193, 251.78);
    /// 彩度の上限（sRGB 色域で表現可能な最大彩度付近）。
    pub const MAX_CHROMA: f32 = 0.3;

    /// プリセット色（名前は i18n の `color.<名前>` に対応）。先頭が既定色。選択画面に表示される履歴（[`Accent::history`]）の初期内容。
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

    /// 選択画面に並べる履歴（[`Accent::PRESETS`] と同数）: ユーザー作成色 `custom`（新しい順）の後ろをプリセット色で埋める。
    /// 重複は排除する。作成色がなければプリセット色そのもの。現在の色 `current` が並びに含まれない場合（履歴導入前に作った色）は先頭に配置する。
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

    /// プリセット色に該当する場合はその名前を返す（[`Accent::PRESETS`]）。
    pub fn name(self) -> Option<&'static str> {
        let found = Accent::PRESETS.iter().find(|(_, a)| *a == self);
        found.map(|(name, _)| *name)
    }

    /// 色の 16 進表記（`#RRGGBB`）。名前のない色をテキストで伝える用途。
    pub fn hex(self) -> String {
        format!("#{:06X}", self.color() >> 8)
    }

    /// この色の sRGB 値（0xRRGGBBAA。色域外の彩度はクランプする）。塗りにはこの値をそのまま使用する。
    pub fn color(self) -> u32 {
        let a = self.clamped();
        pack(in_gamut(a.lightness, a.chroma, a.hue))
    }

    /// 有効範囲にクランプする（色相は 0〜360 に周回させる。NaN 等の非数値は既定値で置換する）。
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

/// アクセントの塗りの 3 状態（通常・ホバー・押下）と、その上に載せる色（0xRRGGBBAA）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub solid: u32,
    pub hover: u32,
    pub active: u32,
    pub on: u32,
}

/// 塗りの上の文字に必要なコントラスト比（3:1）と、状態間の弁別に必要な比（1.05:1）。
/// 8 ビットへの量子化後も下回らないよう、どちらも若干の余裕を持たせる。
const CLEAR: f32 = 3.1;
const DISTINCT: f32 = 1.06;

/// `mode` のテーマにおいて、塗りを `accent` にした場合の色セットを算出する。塗りは選択色をそのまま使用し（色域クランプのみ）、他の状態色がそれに追従する。
pub fn fill(mode: Mode, accent: Accent) -> Fill {
    let base = Theme::of(mode);
    let a = accent.clamped();
    // (輝度 + 0.05) の比が WCAG コントラスト比となるため、加算済みの値で計算する。
    let lit = |c: Srgb| luminance(c.into_linear()) + 0.05;
    let token = |c: u32| lit(Srgb::<u8>::from(c >> 8).into_format());
    let picked = in_gamut(a.lightness, a.chroma, a.hue);
    let solid = lit(picked);
    // ホバー・押下は既定の塗りと同じ比率で暗くする。黒より暗い色は表現できないため、暗い塗りでは同じ比率で明るくする。
    let hover_ratio = token(base.accent_solid) / token(base.accent_solid_hover);
    let active_ratio = token(base.accent_solid) / token(base.accent_solid_active);
    let darker = solid / active_ratio >= 0.05 * DISTINCT;
    let step = |ratio: f32| if darker { solid / ratio } else { solid * ratio };
    let (mut hover, mut active) = (step(hover_ratio), step(active_ratio));
    // 上に載せる色は、3 状態すべてで可読性を確保できる方を選ぶ。白で十分なら白（既定と同じ）、不十分なら黒。
    let white = 1.05 / solid.max(active) >= CLEAR;
    if !white {
        // 黒を載せる場合（塗りが明るく、ホバー・押下はそれより暗い）。黒が可読となる明るさの下限を維持する。
        active = active.max(0.05 * CLEAR);
        hover = hover.max(active * DISTINCT);
    }
    // 押下色がスクロールバーのつまみ（`track`）と同じ明るさになる場合、塗りから離れる方向へずらして弁別性を確保する。
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

/// 8 ビットへの量子化で値が変わらない範囲であれば、色域内とみなす。
fn fits(c: Srgb) -> bool {
    [c.red, c.green, c.blue]
        .iter()
        .all(|v| (-0.002..=1.002).contains(v))
}

/// 明度 `l`・色相 `h` を保ったまま、色域に収まるまで彩度 `c` を段階的に減衰させた色。
fn in_gamut(l: f32, mut c: f32, h: f32) -> Srgb {
    loop {
        let rgb = Srgb::from_color(Oklch::new(l, c, h));
        if fits(rgb) || c < 0.001 {
            return rgb.clamp();
        }
        c *= 0.9;
    }
}

/// 輝度 `y`・彩度 `c`・色相 `h` の色（0xRRGGBBAA）。色域に収まるまで彩度を段階的に減衰させる。
fn paint(y: f32, mut c: f32, h: f32) -> u32 {
    loop {
        // 輝度は明度に対して単調増加するため、目標輝度に対応する明度を二分探索で求める。
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

    /// 任意の色を選択しても、上に載せる色は全状態で可読性を保ち、状態間の弁別も確保される。
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

    /// 既定色は tokens.json の `accent-solid` と一致する（定数を転記しているため、不一致があればここで検出される）。
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
            // ホバー・押下は既定と同等の明るさ（色味は塗りに追従するため、色値自体はわずかに異なる）。
            for (made, token) in [
                (f.hover, t.accent_solid_hover),
                (f.active, t.accent_solid_active),
            ] {
                assert!((contrast(made, token) - 1.0).abs() < 0.03);
            }
        }
    }

    /// 選択色はテーマを問わずそのまま塗りとなる。明るい色には黒、暗い色には白が載る。
    #[test]
    fn picked_color_is_kept_and_the_text_on_it_flips() {
        let preset = |name: &str| Accent::PRESETS.iter().find(|(n, _)| *n == name).unwrap().1;
        let yellow = fill(Mode::Dark, preset("yellow"));
        let [r, g, b, _] = yellow.solid.to_be_bytes();
        assert!(
            r > 200 && g > 180 && b < 120,
            "明るい黄色が維持されていること: {r} {g} {b}"
        );
        assert_eq!(yellow.on, scale::BLACK);
        let purple = fill(Mode::Dark, preset("purple"));
        assert_eq!(purple.on, scale::WHITE);
        // ライトテーマでも塗りは変更しない（白いカード上でも黄色はそのまま維持され、黒が載る）。
        let light = fill(Mode::Light, preset("yellow"));
        assert_eq!((light.solid, light.on), (yellow.solid, scale::BLACK));
        // 黒に近い塗りでは、ホバー・押下が明方向へ変化する（黒より暗い色は表現できないため）。
        let ink = fill(Mode::Dark, Accent::new(0.15, 0.0, 0.0));
        assert_eq!(ink.on, scale::WHITE);
        assert!(contrast(ink.active, scale::BLACK) > contrast(ink.solid, scale::BLACK));
    }

    /// 履歴の初期内容はプリセット色。作成色は先頭に挿入され、プリセット色が末尾から押し出される。
    #[test]
    fn history_starts_as_the_presets_and_takes_custom_colors_first() {
        let presets = Accent::PRESETS.map(|(_, a)| a);
        assert_eq!(Accent::history(Accent::DEFAULT, []), presets);
        let (mine, older) = (Accent::new(0.5, 0.1, 10.0), Accent::new(0.4, 0.1, 20.0));
        // プリセット色と同一の色を作成しても、重複して並ばない。
        let row = Accent::history(mine, [mine, older, presets[2]]);
        assert_eq!(row.len(), presets.len());
        assert_eq!(row[..5], [mine, older, presets[2], presets[0], presets[1]]);
        assert!(!row.contains(&presets[7]), "末尾から押し出される");
        // 並びに含まれない現在の色（履歴導入前に作成した色）は先頭に表示される。
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
