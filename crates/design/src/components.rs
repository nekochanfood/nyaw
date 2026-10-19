//! Components: 役割（[`Theme`]）と Constants だけで組む部品。生の色は書かない。
//! 部品は `on_click` などをそのまま足せる（`Styled` / `InteractiveElement` を中の `Stateful<Div>` に委ねている）。
//! ホバー・押下の色は [`duration_ms::NORMAL`] かけて OKLab で補間する（[`Motion`] が有効なときだけ）。

use std::time::Instant;

use gpui::{
    App, Bounds, ContentMask, Div, FontWeight, InteractiveElement, Interactivity, IntoElement,
    MouseButton, Pixels, Point, RenderOnce, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, canvas, div, fill, point, prelude::*, px, rgba, size,
};
use palette::{FromColor, Mix, Oklab, Srgb};

mod navigation;
mod scrollbar;
mod shaped_button;
mod slider;
mod tooltip;
pub use navigation::{Navigation, navigation};
pub use scrollbar::{Elastic, Scrollbars, claim_scroll, elastic, free_scroll, scrollbars};
pub use shaped_button::{ShapedButton, rounded_contains, rounded_corners, shaped_button};
pub use slider::{Slider, slider};
pub use tooltip::*;

use crate::{Motion, Theme, duration_ms, font_size, radius, spacing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    /// 既定。カードに載せる前提で、カードよりわずかに暗い無彩色。
    Default,
    Primary,
    Danger,
    /// カードではなく背景（[`panel`] の地）に直接置くとき。背景から浮くよう、背景との差を取った無彩色。
    OnBackground,
    /// 背景の無いボタン（画像の上に置く操作など）。ふだんは透明で、ホバー・押下だけ背景用の色が付く。
    Ghost,
}

/// 色の移り変わり: `from` から `to` へ `start` から動いている。`start` が `None` なら落ち着いている。
#[derive(Clone, Copy)]
struct Fade {
    from: u32,
    to: u32,
    start: Option<Instant>,
}

impl Fade {
    /// 今の色と、まだ動いているか。
    fn at(&self, now: Instant) -> (u32, bool) {
        let Some(start) = self.start else {
            return (self.to, false);
        };
        let t = now.saturating_duration_since(start).as_secs_f32() * 1000.0
            / duration_ms::NORMAL as f32;
        if t >= 1.0 {
            return (self.to, false);
        }
        // ease-out（立ち上がりが速く、着地がやわらかい）
        let e = 1.0 - (1.0 - t).powi(3);
        (mix(self.from, self.to, e), true)
    }
}

/// 0xRRGGBBAA を OKLab で混ぜる（アルファは混ぜない）。
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let lab = |c: u32| -> Oklab {
        let s: Srgb<f32> = Srgb::<u8>::from(c >> 8).into_format();
        Oklab::from_color(s)
    };
    let m = lab(a).mix(lab(b), t);
    let s: Srgb<u8> = Srgb::from_color(m).into_format();
    u32::from(s.red) << 24 | u32::from(s.green) << 16 | u32::from(s.blue) << 8 | (b & 0xff)
}

#[derive(Clone, Copy)]
struct State {
    pointer: Pointer,
    fade: Fade,
    /// 押した位置（ウィンドウ座標）と時刻。波紋の起点。
    ripple: Option<(Point<Pixels>, Instant)>,
}

/// 波紋の `elapsed_ms` 時点の（半径, 不透明度 0..=255）。`reach` は起点から最も遠い角までの距離。終わっていれば `None`。
/// 半径は ease-out で広がり、不透明度は線形に薄れる。
fn ripple_frame(elapsed_ms: f32, reach: f32) -> Option<(f32, u8)> {
    let t = elapsed_ms / duration_ms::SLOW as f32;
    if !(0.0..1.0).contains(&t) {
        return None;
    }
    Some((
        reach * (1.0 - (1.0 - t).powi(3)),
        (RIPPLE_ALPHA * (1.0 - t)) as u8,
    ))
}

/// 角丸を半分の幅・高さに収める（CSS と同じく、大きすぎる角丸は丸めた長さに縮む）。[左上, 右上, 右下, 左下]。
pub(super) fn clamp_corners(c: [f32; 4], width: f32, height: f32) -> [f32; 4] {
    let max = (width.min(height) / 2.0).max(0.0);
    c.map(|r| r.clamp(0.0, max))
}

/// 半径 `r` の角で、辺から `d` の高さの行が内側へ削られる長さ。`d >= r` なら 0。
pub(super) fn corner_inset(r: f32, d: f32) -> f32 {
    if d >= r {
        return 0.0;
    }
    r - (r * r - (r - d) * (r - d)).sqrt()
}

/// 波紋の最初の不透明度（0..=255）。
const RIPPLE_ALPHA: f32 = 64.0;

#[derive(Clone, Copy, Default)]
struct Pointer {
    hovered: bool,
    pressed: bool,
}

/// ボタンの高さと左右の余白（文字 `font_size::MD` に対して、高さ ≒ 2.8 倍、左右 ≒ 1.7 倍）。最小ヒット領域（48）を下回らない。
const BUTTON_HEIGHT: f32 = 54.0;
const BUTTON_PADDING: f32 = 32.0;

/// 押せるボタン。`id` はそのまま `debug_selector` にもなる。
#[derive(IntoElement)]
pub struct Button {
    id: SharedString,
    base: Stateful<Div>,
    rest: u32,
    hover: u32,
    active: u32,
    /// 波紋の色（文字色）。
    ink: u32,
    ripple: bool,
    /// ホバー中にカーソルへ出す説明（[`Tooltips`]）。
    tooltip: Option<SharedString>,
}

impl Button {
    /// 波紋（押した位置から広がる効果）を出さない。既定は出す。
    pub fn no_ripple(mut self) -> Self {
        self.ripple = false;
        self
    }

    /// ホバー中にカーソルへ説明を出す。アイコンだけのボタンなど、見た目だけでは意味が伝わりにくいものに付ける。
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// 切り替えボタンが有効なときの見た目（平常時の色が押下の色になる）。
    pub fn selected(mut self, selected: bool) -> Self {
        if selected {
            self.rest = self.active;
        }
        self
    }
}

pub fn button(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    let mut b = base_button(theme, id.into(), variant);
    b.base = b.base.px(px(BUTTON_PADDING)).child(label.into());
    b
}

/// アイコンだけの丸いボタン。見た目だけでは意味が伝わりにくいので、説明（ツールチップ）を必ず持たせる。
pub fn icon_button(
    theme: &Theme,
    id: impl Into<SharedString>,
    glyph: &'static crate::Icon,
    tooltip: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    round_button(theme, id, tooltip, variant).child(
        crate::icon(glyph)
            .weight(crate::Weight::Bold)
            .size(px(font_size::LG)),
    )
}

/// 中身の無い、横長のボタン（文字のボタンと同じ高さ・角丸・状態色。左右の余白は小さめ）。中身（アイコンと文字など）は呼び出し側が足す。
pub fn content_button(theme: &Theme, id: impl Into<SharedString>, variant: Variant) -> Button {
    let mut b = base_button(theme, id.into(), variant);
    b.base = b.base.px(px(spacing::LG)).gap(px(spacing::SM));
    b
}

/// 中身の無い丸いボタン（アイコンボタンと同じ寸法・状態色・必須の説明）。中身（アプリのアイコンの画像など）は呼び出し側が足す。
pub fn round_button(
    theme: &Theme,
    id: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    let mut b = base_button(theme, id.into(), variant).tooltip(tooltip);
    b.base = b.base.w(px(BUTTON_HEIGHT));
    b
}

/// Twemoji の丸いボタン。アイコンボタンと同じ寸法・状態色・必須の説明を持つ。
pub fn emoji_button(
    theme: &Theme,
    id: &'static str,
    glyph: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    let mut b = base_button(theme, id.into(), variant).tooltip(tooltip);
    b.base = b
        .base
        .w(px(BUTTON_HEIGHT))
        .child(crate::emoji(glyph).size(px(font_size::LG)));
    b
}

/// 高さ・角丸・色・文字だけを決めたボタン。中身と横の大きさは呼び出し側が足す。
fn base_button(theme: &Theme, id: SharedString, variant: Variant) -> Button {
    let (bg, hover, active, fg) = match variant {
        Variant::Default => (
            theme.control,
            theme.control_hover,
            theme.control_active,
            theme.neutral_text,
        ),
        Variant::Primary => (
            theme.accent_solid,
            theme.accent_solid_hover,
            theme.accent_solid_active,
            theme.on_accent,
        ),
        Variant::Danger => (
            theme.danger_solid,
            theme.danger_solid_hover,
            theme.danger_solid_active,
            theme.on_danger,
        ),
        Variant::OnBackground => (
            theme.control_on_background,
            theme.control_on_background_hover,
            theme.control_on_background_active,
            theme.neutral_text,
        ),
        Variant::Ghost => (
            theme.control_on_background_hover & !0xff,
            theme.control_on_background_hover,
            theme.control_on_background_active,
            theme.neutral_text,
        ),
    };
    let selector = id.clone();
    let base = div()
        .id(id.clone())
        .debug_selector(move || selector.to_string())
        .h(px(BUTTON_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::FULL))
        .text_size(px(font_size::MD))
        // 太字の 18.66px 以上は大きい文字（WCAG の 3:1）。ボタンの文字色の検査はこれを前提にしている。
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(fg));
    Button {
        id,
        base,
        rest: bg,
        hover,
        active,
        ink: fg,
        ripple: true,
        tooltip: None,
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.base.extend(elements)
    }
}

impl RenderOnce for Button {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let rest = self.rest;
        let state = window.use_keyed_state(self.id.clone(), cx, move |_, _| State {
            pointer: Pointer::default(),
            fade: Fade {
                from: rest,
                to: rest,
                start: None,
            },
            ripple: None,
        });
        let State {
            pointer: p,
            fade: f,
            ripple,
        } = *state.read(cx);
        let target = if p.pressed {
            self.active
        } else if p.hovered {
            self.hover
        } else {
            rest
        };

        let now = Instant::now();
        let color = if f.to == target {
            let (c, moving) = f.at(now);
            if moving {
                keep_drawing(window, cx);
            }
            c
        } else if cx.has_global::<Motion>() {
            // 目標が変わった。今の色から新しい目標へ向けて動き直す。
            let (from, _) = f.at(now);
            state.update(cx, |s, _| {
                s.fade = Fade {
                    from,
                    to: target,
                    start: Some(now),
                }
            });
            keep_drawing(window, cx);
            from
        } else {
            state.update(cx, |s, _| {
                s.fade = Fade {
                    from: target,
                    to: target,
                    start: None,
                }
            });
            target
        };

        // ポインタの状態を書き換えて描き直す。
        let press = state.clone();
        // ツールチップの持ち主。このボタンが描かれなくなる（状態が捨てられる）と要求も消える。
        let owner = state.clone().into_any().downgrade();
        let set = move |hovered: Option<bool>, pressed: Option<bool>| {
            let state = state.clone();
            move |window: &mut Window, cx: &mut App| {
                state.update(cx, |s, _| {
                    let p = &mut s.pointer;
                    if let Some(h) = hovered {
                        p.hovered = h;
                        if !h {
                            p.pressed = false;
                        }
                    }
                    if let Some(d) = pressed {
                        p.pressed = d;
                    }
                });
                window.refresh();
            }
        };
        let animated = cx.has_global::<Motion>();
        let (ink, enabled) = (self.ink, self.ripple && animated);
        let (over, leave) = (set(Some(true), None), set(Some(false), None));
        let (id, tip) = (self.id, self.tooltip.clone());
        let (down, up) = (set(None, Some(true)), set(None, Some(false)));
        let up_out = set(None, Some(false));
        // ボタンごとに違う角丸（`rounded*` で上書きされたものも含む）。指定が無い角は 0。
        let rem = window.rem_size();
        let corners = {
            let c = &self.base.style().corner_radii;
            let px_of =
                |l: Option<gpui::AbsoluteLength>| l.map_or(0.0, |l| f32::from(l.to_pixels(rem)));
            [
                px_of(c.top_left),
                px_of(c.top_right),
                px_of(c.bottom_right),
                px_of(c.bottom_left),
            ]
        };
        let wave = ripple.filter(|_| enabled).and_then(|(origin, start)| {
            let ms = now.saturating_duration_since(start).as_secs_f32() * 1000.0;
            ripple_frame(ms, 0.0)?;
            keep_drawing(window, cx);
            Some(
                canvas(
                    |_, _, _| (),
                    move |b, _, w, _| {
                        let (dx, dy) = (
                            (origin.x - b.left())
                                .abs()
                                .max((origin.x - b.right()).abs()),
                            (origin.y - b.top())
                                .abs()
                                .max((origin.y - b.bottom()).abs()),
                        );
                        let reach = f32::from(dx).hypot(f32::from(dy));
                        let ms = Instant::now()
                            .saturating_duration_since(start)
                            .as_secs_f32()
                            * 1000.0;
                        let Some((r, a)) = ripple_frame(ms, reach) else {
                            return;
                        };
                        let circle = Bounds::new(
                            point(origin.x - px(r), origin.y - px(r)),
                            size(px(r * 2.0), px(r * 2.0)),
                        );
                        let quad =
                            fill(circle, rgba(ink & !0xff | u32::from(a))).corner_radii(px(r));
                        // 角丸で切る: gpui のマスクは長方形だけなので、角の部分は 1px の帯に分けて
                        // 帯ごとに角丸の内側だけをマスクにして描き、間の長方形部分は一度で描く。
                        let (width, height) = (f32::from(b.size.width), f32::from(b.size.height));
                        let [tl, tr, br, bl] = clamp_corners(corners, width, height);
                        let (top, bottom) = (tl.max(tr), bl.max(br));
                        let at = |x: f32, y: f32, w: f32, h: f32| {
                            Bounds::new(b.origin + point(px(x), px(y)), size(px(w), px(h)))
                        };
                        let mut paint = |mask: Bounds<Pixels>| {
                            w.with_content_mask(Some(ContentMask { bounds: mask }), |w| {
                                w.paint_quad(quad.clone())
                            })
                        };
                        paint(at(0.0, top, width, height - top - bottom));
                        for i in 0..top.ceil() as u32 {
                            let (l, r) = (
                                corner_inset(tl, i as f32 + 0.5),
                                corner_inset(tr, i as f32 + 0.5),
                            );
                            paint(at(l, i as f32, width - l - r, 1.0));
                        }
                        for i in 0..bottom.ceil() as u32 {
                            let (l, r) = (
                                corner_inset(bl, i as f32 + 0.5),
                                corner_inset(br, i as f32 + 0.5),
                            );
                            paint(at(l, height - 1.0 - i as f32, width - l - r, 1.0));
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
        });
        self.base
            .bg(rgba(color))
            .children(wave)
            .on_hover(move |h, w, cx| {
                if let Some(tip) = &tip {
                    hover_tooltip(id.clone(), tip.clone(), owner.clone(), *h, w, cx);
                }
                if *h { over(w, cx) } else { leave(w, cx) }
            })
            .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                if enabled {
                    press.update(cx, |s, _| s.ripple = Some((e.position, Instant::now())));
                }
                down(w, cx)
            })
            .on_mouse_up(MouseButton::Left, move |_, w, cx| up(w, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, w, cx| up_out(w, cx))
    }
}

/// 色が動いている間は次のフレームも描く。VR のフレームループにも伝える（[`Motion`]）。
fn keep_drawing(window: &mut Window, cx: &mut App) {
    if cx.has_global::<Motion>() {
        let id = window.window_handle().window_id();
        cx.global_mut::<Motion>().animating.insert(id);
    }
    window.request_animation_frame();
}

/// 円の進み具合: 一辺 `size` px・太さ `stroke` px の線の輪。溝（`groove`）の上に、上から時計回りに `progress`（0〜1）
/// だけアクセントの色で満ちる。中身（残り時間など）は呼び出し側が `relative` の親で重ねる。
pub fn progress_ring(theme: &Theme, size: f32, stroke: f32, progress: f32) -> impl IntoElement {
    let (track, fill) = (theme.groove, theme.accent_solid);
    let progress = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let r = (bounds.size.width.min(bounds.size.height) - px(stroke)) / 2.0;
            // 円弧は細かい折れ線で描く（180° を超える弧の向きの取り違えが無い）。
            let arc = |to: f32| {
                let mut path = gpui::PathBuilder::stroke(px(stroke));
                let steps = (to * 96.0).ceil().max(1.0) as usize;
                for i in 0..=steps {
                    let a = to * i as f32 / steps as f32 * std::f32::consts::TAU
                        - std::f32::consts::FRAC_PI_2;
                    let p = point(center.x + r * a.cos(), center.y + r * a.sin());
                    if i == 0 {
                        path.move_to(p)
                    } else {
                        path.line_to(p)
                    }
                }
                path.build().ok()
            };
            if let Some(path) = arc(1.0) {
                window.paint_path(path, rgba(track));
            }
            if progress > 0.0
                && let Some(path) = arc(progress)
            {
                window.paint_path(path, rgba(fill));
            }
        },
    )
    .flex_none()
    .size(px(size))
}

/// オン / オフのスイッチ。押したときの処理は呼び出し側が `on_click` で足す。
pub fn switch(theme: &Theme, id: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let (w, h, pad) = (44.0, 24.0, 2.0);
    let knob = h - pad * 2.0;
    let id: SharedString = id.into();
    let mut track = div()
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .flex()
        .items_center()
        .w(px(w))
        .h(px(h))
        .px(px(pad))
        .rounded(px(radius::FULL))
        .bg(rgba(if on { theme.accent_solid } else { theme.groove }));
    if on {
        track = track.justify_end();
    }
    track.child(
        div()
            .size(px(knob))
            .rounded(px(radius::FULL))
            .bg(rgba(if on { theme.on_accent } else { theme.card })),
    )
}

pub use crate::PANEL_MARGIN;
use crate::PANEL_SHADOW_BLUR;

/// パネルの地の置き方: ウィンドウの縁から [`PANEL_MARGIN`] 内側を埋め、その余白に影を落とす。
/// 大きさは `absolute` と `inset` で決まるので、呼び出し側は `.relative()` や `.size_full()` で上書きしない
/// （`absolute` のままで、中の `absolute` の子の基準になる）。
fn panel_base(div: crate::Surface) -> crate::Surface {
    div.absolute()
        .inset(px(PANEL_MARGIN))
        .shadow(panel_shadow())
}

/// パネルの影（ずらさず四方へ広がる）。
fn panel_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0.0, 0.0, 0.0, 0.4),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(PANEL_SHADOW_BLUR),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// パネル全体の地。角丸は `XL`、中身との間隔は `MD`（12）なので、中のカードの角丸は同心の `XL − MD = LG`。
/// カードを複数並べる画面だけが使い、中身は [`card`] に入れる。カードが 1 枚なら [`card_panel`]。
pub fn panel(theme: &Theme) -> crate::Surface {
    panel_base(crate::Surface::new(div()))
        .p(px(spacing::MD))
        .rounded(px(radius::XL))
        .bg(rgba(theme.background))
        .text_color(rgba(theme.neutral_text))
}

/// 地（[`panel`]）の上に置くカード。要素は基本的にカードの中に置く。
pub fn card(theme: &Theme) -> crate::Surface {
    crate::Surface::new(div())
        .rounded(px(radius::LG))
        .bg(rgba(theme.card))
        .text_color(rgba(theme.neutral_text))
}

/// カードが 1 枚だけの画面: 地（[`panel`]）を敷かず、カードがパネル全体になる。角丸はパネルの `XL`。
pub fn card_panel(theme: &Theme) -> crate::Surface {
    panel_base(card(theme)).rounded(px(radius::XL))
}

/// ナビゲーションバーの地の高さ（ボタン + 上下 `SM`）。カプセル形なので、ボタン（`FULL`）の角は帯の角と同心になる。
pub const NAV_BAR_HEIGHT: f32 = BUTTON_HEIGHT + 2.0 * spacing::SM;

/// 触れていないときのナビゲーションバー（丸い線）の幅（帯に対する割合）と高さ（px）。
const NAV_LINE_WIDTH: f32 = 0.4;
const NAV_LINE_HEIGHT: f32 = spacing::MD;

/// ナビゲーションバー: パネルの下に付き、パネルを操作する帯。中央にパネルのタイトル、右端に閉じるボタン（アイコンだけ。
/// id は `nav-close`）。`minimize` なら閉じる代わりにしまうボタン（マイナス。id は `nav-minimize`）。
/// カード 1 枚の帯で、角丸は `FULL`。押したときの処理（閉じる・しまう）は `on_close` で受け取る。
/// `leading` は左端に並べる（パネルごとの操作。閉じるボタンと左右対称の位置から右へ）。
/// 帯は窓の縁から [`PANEL_MARGIN`] 内側に置き（影の余白）、高さは [`NAV_BAR_HEIGHT`]。
///
/// `open`（0〜1）は広がり具合。0（触れていない）はボタンもタイトルも出さない丸い線で、帯の上端に寄せて置く。
/// 広がるときは、まず線が帯の上下中央へ下りてから、そこを中心に上下左右へ広がる（縮むときは逆に、縮んでから上がる）。
/// 置き場所のアンカー（帯の上端の左右中央。持ち主に付ける・頭からの距離で大きくするのに使う）とは別に、
/// 広がりのアンカー（帯の中心）を持つ。中身は帯の中に置いて帯と一緒に動き、広がりの後半で不透明度だけで現れる。
// ponytail: 呼び出しが 34 か所あるので引数は並べたまま。増えるようなら構造体にまとめる。
#[allow(clippy::too_many_arguments)]
pub fn nav_bar(
    theme: &Theme,
    open: f32,
    title: impl Into<SharedString>,
    minimize: bool,
    close_tooltip: impl Into<SharedString>,
    on_close: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    leading: Vec<Button>,
    beside_close: Option<Button>,
) -> Div {
    let (close_id, close_glyph) = if minimize {
        ("nav-minimize", &crate::icons::MINUS)
    } else {
        ("nav-close", &crate::icons::X)
    };
    // 前半で動き、後半で広がる。それぞれの区間の中でも ease-in-out にして、つなぎ目で急に変わらないようにする。
    const MOVE_END: f32 = 0.35;
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let open = open.clamp(0.0, 1.0);
    let moved = smooth(open / MOVE_END);
    let grown = smooth((open - MOVE_END) / (1.0 - MOVE_END));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let height = lerp(NAV_LINE_HEIGHT, NAV_BAR_HEIGHT, grown);
    // 広がりの中心（帯の上下中央）。動く前は、線の上端が帯の上端に来るところまで上げておく。
    let center_offset = lerp(-(NAV_BAR_HEIGHT - NAV_LINE_HEIGHT) / 2.0, 0.0, moved);
    // タイトルは帯の中央。長くても閉じるボタンにかからないよう、左右にボタンの分を空けて省略する。
    let side = leading.len().max(1 + usize::from(beside_close.is_some())) as f32
        * (BUTTON_HEIGHT + spacing::SM)
        + spacing::SM;
    div().absolute().inset(px(PANEL_MARGIN)).child(
        div()
            .absolute()
            .top(px(center_offset))
            .left_0()
            .w_full()
            .h(px(NAV_BAR_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .child(
                card(theme)
                    // 縮んだ線はダークでも暗い背景に埋もれないよう、ライトの色で描き、広がるにつれて今のテーマの色へ移る。
                    .bg(rgba(mix(Theme::LIGHT.card, theme.card, grown)))
                    .relative()
                    .shadow(panel_shadow())
                    .rounded(px(radius::FULL))
                    .w(gpui::relative(lerp(NAV_LINE_WIDTH, 1.0, grown)))
                    .h(px(height))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .opacity((grown * 2.0 - 1.0).clamp(0.0, 1.0))
                            .flex()
                            .items_center()
                            .px(px(side))
                            .text_color(rgba(theme.neutral_text))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_center()
                                    .truncate()
                                    .text_size(px(font_size::MD))
                                    .font_weight(FontWeight::BOLD)
                                    .child(title.into()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px((height - BUTTON_HEIGHT) / 2.0))
                                    .right(px(spacing::SM))
                                    .flex()
                                    .gap(px(spacing::SM))
                                    .when_some(beside_close, |d, button| d.child(button))
                                    .child(
                                        icon_button(
                                            theme,
                                            close_id,
                                            close_glyph,
                                            close_tooltip,
                                            Variant::Default,
                                        )
                                        .on_click(on_close),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px((height - BUTTON_HEIGHT) / 2.0))
                                    .left(px(spacing::SM))
                                    .flex()
                                    .gap(px(spacing::SM))
                                    .children(leading),
                            ),
                    ),
            ),
    )
}

/// 一覧の行: 左にアイコンと文字、右に `>`。行は [`list`] のカードに並べる前提で、安静時はカードと同じ色、
/// ホバー・押下で `control-hover` / `control-active` に沈む（区切り線とカードの角は [`list`] が付ける）。
/// 選んでいる行は `.selected(true)`（押下の色のまま）。高さはボタンと同じ。
pub fn list_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
) -> Button {
    menu_item(theme, id, glyph, label).child(
        crate::icon(&crate::icons::CARET_RIGHT)
            .size(px(font_size::MD))
            .color(rgba(theme.neutral_text_low)),
    )
}

/// メニューの行: 一覧の行（[`list_item`]）と同じで、右の `>` が無い（その場で何かをする項目。先へ進む一覧ではない）。
pub fn menu_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
) -> Button {
    let mut b = base_button(theme, id.into(), Variant::Default);
    (b.rest, b.hover, b.active) = (theme.card, theme.control_hover, theme.control_active);
    b.base = b
        .base
        .w_full()
        .justify_start()
        .gap(px(spacing::MD))
        .px(px(spacing::LG))
        .rounded(px(0.0))
        // 行は文字が主役のボタンではないので太字にしない（文字は本文と同じ `MD`）。
        .font_weight(FontWeight::NORMAL)
        .child(crate::icon(glyph).size(px(font_size::LG)))
        .child(div().flex_1().child(label.into()));
    b
}

/// タイル: 押せるカード。グリッドに並べる（ダッシュボードの取得元・クイック設定）。地（[`panel`]）に直接置き、
/// 安静時はカードと同じ色、ホバー・押下で `control-hover` / `control-active`。有効な切り替えは `.selected(true)`。
/// 中身は縦に中央揃えで積む。大きさと中身（アイコン・文字）は呼び出し側が足す。
pub fn tile(theme: &Theme, id: impl Into<SharedString>) -> Button {
    let mut b = base_button(theme, id.into(), Variant::Default);
    (b.rest, b.hover, b.active) = (theme.card, theme.control_hover, theme.control_active);
    b.base = b
        .base
        .flex_col()
        .gap(px(spacing::SM))
        .p(px(spacing::MD))
        .rounded(px(radius::LG))
        .font_weight(FontWeight::NORMAL);
    b
}

/// 地の上に置くタイル（アプリ一覧のアプリなど）: [`tile`] と同じく縦に並べるが、カードの地を敷かず、ホバー・押下のときだけ
/// 色が付く。ふだんは地（`background`）と同じ色（透明にするとホバーの色から戻るフェードが途中で切れる。[`sidebar_item`]）。
pub fn ghost_tile(theme: &Theme, id: impl Into<SharedString>) -> Button {
    let mut b = tile(theme, id);
    (b.rest, b.hover, b.active) = (
        theme.background,
        theme.control_on_background_hover,
        theme.control_on_background_active,
    );
    b
}

/// サイドバーの行: 左にアイコンと文字。どれを選んでいるかを見せるための行で、ふだんは地（`background`）と同じ色
/// （背景が無いように見える。透明にするとホバーの色から戻るフェードが途中で切れるので地の色を使う）、ホバー・押下で
/// `control-on-background-*`、選んでいる行（`selected`）はアクセント（`accent-bg-active` に `accent-text`）。高さはボタンと同じ。
pub fn sidebar_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
    selected: bool,
) -> Button {
    let mut b = base_button(theme, id.into(), Variant::OnBackground);
    (b.rest, b.hover, b.active) = if selected {
        (
            theme.accent_bg_active,
            theme.accent_bg_active,
            theme.accent_bg_active,
        )
    } else {
        (
            theme.background,
            theme.control_on_background_hover,
            theme.control_on_background_active,
        )
    };
    let fg = if selected {
        theme.accent_text
    } else {
        theme.neutral_text
    };
    b.ink = fg;
    b.base = b
        .base
        .w_full()
        .justify_start()
        .gap(px(spacing::MD))
        .px(px(spacing::LG))
        .font_weight(FontWeight::NORMAL)
        .text_color(rgba(fg))
        .child(crate::icon(glyph).size(px(font_size::LG)))
        .child(div().flex_1().child(label.into()));
    b
}

/// 行（[`list_item`]）をカード 1 枚に縦に並べ、行の間に 1px の区切り線を引く。
/// 端の行はカードの角（`LG`）に合わせて丸める（行の塗りがカードの角からはみ出さない）。
pub fn list(theme: &Theme, items: impl IntoIterator<Item = Button>) -> Div {
    let items: Vec<Button> = items.into_iter().collect();
    let last = items.len().saturating_sub(1);
    let line = theme.neutral_border_subtle;
    card(theme)
        .flex()
        .flex_col()
        .children(items.into_iter().enumerate().flat_map(move |(i, mut b)| {
            if i == 0 {
                b = b.rounded_t(px(radius::LG));
            }
            if i == last {
                b = b.rounded_b(px(radius::LG));
            }
            let divider = (i > 0).then(|| div().h(px(1.0)).bg(rgba(line)).into_any_element());
            divider.into_iter().chain([b.into_any_element()])
        }))
}

/// ダイアログのアイコンの大きさ。
const DIALOG_ICON: f32 = 64.0;
/// ダイアログの文の行の高さ（文字の大きさに対する比）。
const DIALOG_LEADING: f32 = 1.4;
/// 概要が見える行数の上限。超える分はスクロールする。
const DIALOG_LINES: usize = 3;

/// 幅 `width`（px）のダイアログのパネルの高さ（px。影の余白を含む）。ボタンは `actions` 個。
/// タイトルと概要を実際に折り返して数える（概要は [`DIALOG_LINES`] 行まで）。開くとき（`PanelSpec::sized`）に使えば、
/// 余白が潰れも余りもしない高さで開く。
// ponytail: 開いたあとに言語が変わっても測り直さない（長くなった分は概要がスクロールする）。要るなら描いた配置から測り直す。
pub fn dialog_height(
    window: &Window,
    width: u32,
    title: &str,
    description: &str,
    actions: u32,
) -> u32 {
    // 文の幅 = パネル − 影の余白 − 縁（概要はさらに横の `SM`）。
    let inner = width as f32 - 2.0 * (PANEL_MARGIN + spacing::XL);
    let lines = |text: &str, size: f32, weight: FontWeight, wrap: f32| {
        let font = gpui::Font {
            weight,
            ..crate::font::font()
        };
        let run = gpui::TextRun {
            len: text.len(),
            font,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_text(
                SharedString::from(text.to_owned()),
                px(size),
                &[run],
                Some(px(wrap)),
                None,
            )
            .map_or(1, |ls| {
                ls.iter().map(|l| l.wrap_boundaries().len() + 1).sum()
            })
    };
    let title = lines(title, font_size::LG, FontWeight::BOLD, inner);
    let body = lines(
        description,
        font_size::MD,
        FontWeight::NORMAL,
        inner - 2.0 * spacing::SM,
    )
    .min(DIALOG_LINES);
    let n = actions as f32;
    let h = 2.0 * (PANEL_MARGIN + spacing::XL)
        + DIALOG_ICON
        + spacing::XL
        + title as f32 * font_size::LG * DIALOG_LEADING
        + spacing::SM
        + body as f32 * font_size::MD * DIALOG_LEADING
        + spacing::XL
        + n * BUTTON_HEIGHT
        + (n - 1.0) * spacing::SM;
    h.ceil() as u32
}

/// ダイアログ（子パネル）の中身。[`dialog`] で作る。
#[derive(IntoElement)]
pub struct Dialog {
    id: &'static str,
    theme: Theme,
    glyph: &'static crate::Icon,
    /// アイコンのウェイト（既定は塗り。[`Dialog::weight`]）。
    weight: crate::Weight,
    title: SharedString,
    description: SharedString,
    actions: Vec<Button>,
}

impl Dialog {
    /// アイコンのウェイトを変える（既定は塗り）。塗りだと強すぎる・形が分かりにくいアイコン（電源など）に。
    pub fn weight(mut self, weight: crate::Weight) -> Self {
        self.weight = weight;
        self
    }
}

/// ダイアログ（子パネル）の中身。上からアイコン・タイトル・概要・ボタンを中央揃えで並べる。
/// ボタンは幅いっぱいで縦に積む。間隔は決まった値で、全体を上下の中央に置く（余った高さは上下の縁に分ける）。
/// 概要は [`DIALOG_LINES`] 行まで見せ、超える分やパネルに収まらない分は概要だけがスクロールする（`{id}-body`、棒は `{id}-bars`）。
/// パネルの高さは [`dialog_height`] で決める。
/// 並べる順は呼び出し側が決める（主の操作を上に）。`id` はパネルの中で一意に。
/// 開く・閉じる・答えを返すのは呼び出し側（`ui::PanelSpec::child`）。
pub fn dialog(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    actions: impl IntoIterator<Item = Button>,
) -> Dialog {
    Dialog {
        id,
        theme: *theme,
        glyph,
        weight: crate::Weight::Fill,
        title: title.into(),
        description: description.into(),
        actions: actions.into_iter().collect(),
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Dialog {
            id,
            theme,
            glyph,
            weight,
            title,
            description,
            actions,
        } = self;
        let scroll = window
            .use_keyed_state(id, cx, |_, _| gpui::ScrollHandle::new())
            .read(cx)
            .clone();
        let body_id = format!("{id}-body");
        // 余白は縁 `XL`、アイコン → タイトル `XL`（アイコンの上の縁と揃える）、タイトル → 概要 `SM`（ひとまとまり）、文 → ボタン `XL`。
        // 概要の枠は [`DIALOG_LINES`] 行までで、超える分はスクロールする。縮められる（min_h_0）のも概要の枠だけなので、
        // パネルが低くても概要だけが縮む。
        // 横の `SM` は、スクロールバーが文にかからないための余白。
        let body = div()
            .id(SharedString::from(body_id.clone()))
            .debug_selector(move || body_id.clone())
            .min_h_0()
            .max_h(px(DIALOG_LINES as f32 * font_size::MD * DIALOG_LEADING))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(spacing::SM))
            .text_center()
            .text_size(px(font_size::MD))
            .line_height(px(font_size::MD * DIALOG_LEADING))
            .text_color(rgba(theme.neutral_text_low))
            .child(description);
        card_panel(&theme)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(spacing::XL))
            .child(
                crate::icon(glyph)
                    .weight(weight)
                    .size(px(DIALOG_ICON))
                    .color(rgba(theme.accent_solid)),
            )
            .child(
                div()
                    .flex_none()
                    .mt(px(spacing::XL))
                    .text_center()
                    .text_size(px(font_size::LG))
                    .line_height(px(font_size::LG * DIALOG_LEADING))
                    .font_weight(FontWeight::BOLD)
                    .child(title),
            )
            .child(
                div()
                    .relative()
                    .mt(px(spacing::SM))
                    .w_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(scrollbars(&theme, format!("{id}-bars"), &scroll)),
            )
            .child(
                div()
                    .mt(px(spacing::XL))
                    .flex_none()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .children(actions.into_iter().map(|b| b.w_full())),
            )
    }
}

/// 画面やパネルの地。文字色も役割から決まる。
pub fn surface(theme: &Theme) -> crate::Surface {
    crate::Surface::new(div())
        .font(crate::font::font())
        .bg(rgba(theme.background))
        .rounded(px(radius::XL))
        .text_color(rgba(theme.neutral_text))
        .text_size(px(font_size::MD))
        .line_height(px(font_size::MD * 1.4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fade_runs_from_start_to_target_over_normal() {
        let t0 = Instant::now();
        let f = Fade {
            from: 0x000000ff,
            to: 0xffffffff,
            start: Some(t0),
        };
        assert_eq!(f.at(t0), (0x000000ff, true));
        let (mid, moving) = f.at(t0 + Duration::from_millis(duration_ms::NORMAL / 2));
        assert!(moving && mid != 0x000000ff && mid != 0xffffffff);
        assert_eq!(
            f.at(t0 + Duration::from_millis(duration_ms::NORMAL)),
            (0xffffffff, false)
        );
    }

    #[test]
    fn ripple_grows_fades_and_ends() {
        let slow = duration_ms::SLOW as f32;
        let (r0, a0) = ripple_frame(0.0, 100.0).unwrap();
        let (r1, a1) = ripple_frame(slow / 2.0, 100.0).unwrap();
        assert!(r0 == 0.0 && r1 > 50.0 && r1 < 100.0, "{r1}");
        assert!(a0 as f32 == RIPPLE_ALPHA && a1 < a0 && a1 > 0);
        assert_eq!(ripple_frame(slow, 100.0), None);
    }

    #[test]
    fn corner_cuts_follow_the_radius() {
        // 辺ぎわの行ほど深く削れ、半径に届けば削れない。
        assert!(corner_inset(10.0, 0.5) > corner_inset(10.0, 5.0));
        assert_eq!(corner_inset(10.0, 10.0), 0.0);
        assert_eq!(corner_inset(0.0, 0.5), 0.0);
        // 円の式に乗る（半径 10、辺から 5 の行は 10 - sqrt(75)）。
        assert!((corner_inset(10.0, 5.0) - (10.0 - 75f32.sqrt())).abs() < 1e-5);
        // 巨大な角丸（カプセル）は高さの半分に縮む。
        assert_eq!(
            clamp_corners([9999.0, 4.0, -1.0, 0.0], 100.0, 40.0),
            [20.0, 4.0, 0.0, 0.0]
        );
    }

    #[test]
    fn settled_fade_stays_put() {
        let f = Fade {
            from: 1,
            to: 0x3060c0ff,
            start: None,
        };
        assert_eq!(f.at(Instant::now()), (0x3060c0ff, false));
    }
}
