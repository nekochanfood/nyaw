//! Components: 役割（[`Theme`]）と Constants のみで構成する UI 部品。色の直接指定は行わない。
//! 部品は `on_click` 等をそのまま付与できる（`Styled` / `InteractiveElement` を内部の `Stateful<Div>` に委譲）。
//! ホバー・押下の色は [`duration_ms::NORMAL`] かけて OKLab 空間で補間する（[`Motion`] が有効な場合のみ）。

use std::time::Instant;

use gpui::{
    App, Bounds, ContentMask, Div, FontWeight, InteractiveElement, Interactivity, IntoElement,
    MouseButton, Pixels, Point, RenderOnce, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, anchored, canvas, deferred, div, fill, point, prelude::*, px,
    rgba, size,
};
use palette::{FromColor, Mix, Oklab, Srgb};

mod carousel;
mod color;
mod markdown;
mod navigation;
mod scrollbar;
mod shaped_button;
pub mod slide;
mod slider;
mod tooltip;
mod wizard;
pub use carousel::{Carousel, Place, Shape, arc, carousel};
pub use color::{accent_row, accent_sliders, swatch};
pub use markdown::markdown;
pub use navigation::{Navigation, navigation};
pub use scrollbar::{Elastic, Scroll, Scrollbars, claim_scroll, elastic, free_scroll, scrollbars};
pub use shaped_button::{ShapedButton, rounded_contains, rounded_corners, shaped_button};
pub use slider::{Slider, slider};
pub use tooltip::*;
pub use wizard::{
    WIZARD_CONTENT, WIZARD_MARGIN, Wizard, wizard_back, wizard_icon, wizard_next, wizard_panel,
    wizard_step, wizard_welcome,
};

use crate::{Motion, Theme, duration_ms, font_size, radius, spacing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    /// 既定。カード上に配置する前提で、カードよりわずかに暗い無彩色。
    Default,
    Primary,
    Danger,
    /// カードではなく背景（[`panel`] の地）に直接配置する場合。背景から浮くよう、背景との差分を取った無彩色。
    OnBackground,
    /// 背景のないボタン（画像上に配置する操作等）。通常は透明で、ホバー・押下時のみ背景色が付く。
    Ghost,
}

/// 色の遷移: `from` から `to` へ `start` 時点から遷移中。`start` が `None` なら静止状態。
#[derive(Clone, Copy)]
struct Fade {
    from: u32,
    to: u32,
    start: Option<Instant>,
}

impl Fade {
    /// 現在の色と、遷移が継続中かどうか。
    fn at(&self, now: Instant) -> (u32, bool) {
        let Some(start) = self.start else {
            return (self.to, false);
        };
        let t = now.saturating_duration_since(start).as_secs_f32() * 1000.0
            / duration_ms::NORMAL as f32;
        if t >= 1.0 {
            return (self.to, false);
        }
        (
            mix(self.from, self.to, crate::animation::Motion::Enter.ease(t)),
            true,
        )
    }
}

/// 0xRRGGBBAA のアルファ値に `opacity`（0..=1）を乗算する。
fn scale_alpha(color: u32, opacity: f32) -> u32 {
    let alpha = ((color & 0xff) as f32 * opacity).round() as u32;
    color & !0xff | alpha
}

/// 0xRRGGBBAA を OKLab 空間で混色する（アルファは線形補間）。
pub(crate) fn mix(a: u32, b: u32, t: f32) -> u32 {
    let lab = |c: u32| -> Oklab {
        let s: Srgb<f32> = Srgb::<u8>::from(c >> 8).into_format();
        Oklab::from_color(s)
    };
    let m = lab(a).mix(lab(b), t);
    let s: Srgb<u8> = Srgb::from_color(m).into_format();
    let alpha = ((a & 0xff) as f32 + ((b & 0xff) as f32 - (a & 0xff) as f32) * t).round() as u32;
    u32::from(s.red) << 24 | u32::from(s.green) << 16 | u32::from(s.blue) << 8 | alpha
}

#[derive(Clone, Copy)]
struct State {
    pointer: Pointer,
    fade: Fade,
    /// 押下位置（ウィンドウ座標）と時刻。波紋エフェクトの起点。
    ripple: Option<(Point<Pixels>, Instant)>,
}

/// 波紋の `elapsed_ms` 時点の（半径, 不透明度 0..=255）。`reach` は起点から最遠の角までの距離。完了していれば `None`。
/// 半径は ease-out で拡大し、不透明度は線形に減衰する。
fn ripple_frame(elapsed_ms: f32, reach: f32) -> Option<(f32, u8)> {
    let t = elapsed_ms / duration_ms::SLOW as f32;
    if !(0.0..1.0).contains(&t) {
        return None;
    }
    Some((
        reach * crate::animation::Motion::Enter.ease(t),
        (RIPPLE_ALPHA * (1.0 - t)) as u8,
    ))
}

/// 角丸を幅・高さの半分以内にクランプする（CSS と同様、過大な角丸は縮小される）。[左上, 右上, 右下, 左下]。
fn clamp_corners(c: [f32; 4], width: f32, height: f32) -> [f32; 4] {
    let max = (width.min(height) / 2.0).max(0.0);
    c.map(|r| r.clamp(0.0, max))
}

/// 半径 `r` の角丸において、辺から `d` の高さの水平線が角丸により内側へ食い込む長さ。`d >= r` なら 0。
fn corner_inset(r: f32, d: f32) -> f32 {
    if d >= r {
        return 0.0;
    }
    r - (r * r - (r - d) * (r - d)).sqrt()
}

/// 波紋の初期不透明度（0..=255）。
const RIPPLE_ALPHA: f32 = 64.0;

#[derive(Clone, Copy, Default)]
struct Pointer {
    hovered: bool,
    pressed: bool,
}

/// ボタンの高さと左右パディング（文字 `font_size::MD` に対して高さ ≒ 2.8 倍、左右 ≒ 1.7 倍）。最小タッチ領域（48px）を下回らない。
pub const BUTTON_HEIGHT: f32 = 54.0;
const BUTTON_PADDING: f32 = 32.0;

/// 押せるボタン。`id` はそのまま `debug_selector` にもなる。
#[derive(IntoElement)]
pub struct Button {
    id: SharedString,
    base: Stateful<Div>,
    rest: u32,
    hover: u32,
    active: u32,
    /// 波紋の色（文字色と同一）。
    ink: u32,
    ripple: bool,
    /// ホバー効果音を抑制する（行のように、内部の要素がホバー演出を代行する場合）。
    quiet: bool,
    /// [`Variant::Ghost`] であるか（[`Button::background_opacity`] と [`Button::ghost_background`] は Ghost にのみ作用する）。
    ghost: bool,
    /// [`Variant::Ghost`] のホバー・押下の背景の不透明度（0..=1）。
    background_opacity: f32,
    /// ホバー時にカーソル付近に表示するツールチップ（[`Tooltips`]）。
    tooltip: Option<SharedString>,
    /// 無効状態（[`disabled_button`]）。押下時の効果音も抑制する。
    disabled: bool,
}

impl Button {
    /// 波紋エフェクト（押下位置から広がる効果）を無効にする。既定では有効。
    pub fn no_ripple(mut self) -> Self {
        self.ripple = false;
        self
    }

    /// ホバー時にカーソル付近にツールチップを表示する。アイコンのみのボタン等、外観だけでは意味が伝わりにくいものに付与する。
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// [`Variant::Ghost`] のホバー・押下時の背景不透明度（0..=1。範囲外はクランプ）。未指定時は 100%。
    /// 通常時は透明のため影響しない。Ghost 以外の種類では効果なし。要素全体の `opacity`（文字や中身も影響を受ける）とは別。
    pub fn background_opacity(mut self, opacity: f32) -> Self {
        self.background_opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self
    }

    /// [`Variant::Ghost`] のホバー・押下の背景色を `theme` のものに差し替える。背景が現在のテーマと同じ明るさで
    /// ボタンが背景に埋もれる場合に、反対のテーマ（ライト上ならダーク、ダーク上ならライト）の色を渡す。Ghost 以外では効果なし。
    pub fn ghost_background(mut self, theme: &Theme) -> Self {
        if self.ghost {
            self.hover = theme.control_on_background_hover;
            self.active = theme.control_on_background_active;
        }
        self
    }

    /// 通常・ホバー・押下の背景色（[`Button::background_opacity`] 適用済み）。
    fn colors(&self) -> (u32, u32, u32) {
        let scale = |c| {
            if self.ghost {
                scale_alpha(c, self.background_opacity)
            } else {
                c
            }
        };
        (scale(self.rest), scale(self.hover), scale(self.active))
    }

    /// ホバー・押下で背景色を変えず、波紋も抑制する（内部にボタンを含むカード用。背景色の変化により内部ボタンが埋もれるのを防ぐ）。
    /// クリック可能であることとホバー効果音はそのまま維持する。
    pub fn still(mut self) -> Self {
        (self.hover, self.active, self.ripple) = (self.rest, self.rest, false);
        self
    }

    /// トグルボタンが有効状態のときの外観（通常時の色を押下色に変更する）。
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

/// 無効化ボタン: 現在の状態（「設定中」等）をボタンの位置に表示する。塗りのない枠線のみのカプセルに、
/// チェックマークと文字（`neutral-text-low`）を配置する。ホバー・押下で色は変わらず、波紋も効果音も出ない。地・カードのどちらにも配置可能。
pub fn disabled_button(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
) -> Button {
    let mut b = inert(base_button(theme, id.into(), Variant::Ghost));
    b.disabled = true;
    let check = crate::icon(&crate::icons::CHECK)
        .weight(crate::Weight::Bold)
        .size(px(font_size::MD));
    b.base = b
        .base
        .px(px(BUTTON_PADDING))
        .gap(px(spacing::SM))
        .border_1()
        .border_color(rgba(theme.neutral_border))
        .text_color(rgba(theme.neutral_text_low))
        .child(check)
        .child(label.into());
    b
}

/// アイコンのみの丸型ボタン。外観だけでは意味が伝わりにくいため、ツールチップを必須とする。
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

/// 中身のない横長ボタン（文字ボタンと同じ高さ・角丸・状態色。左右パディングは控えめ）。中身（アイコンと文字等）は呼び出し側が追加する。
pub fn content_button(theme: &Theme, id: impl Into<SharedString>, variant: Variant) -> Button {
    let mut b = base_button(theme, id.into(), variant);
    b.base = b.base.px(px(spacing::LG)).gap(px(spacing::SM));
    b
}

/// 中身のない丸型ボタン（アイコンボタンと同じ寸法・状態色・必須ツールチップ）。中身（アプリアイコンの画像等）は呼び出し側が追加する。
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

/// Twemoji の丸型ボタン。アイコンボタンと同じ寸法・状態色・必須ツールチップを持つ。
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

/// 高さ・角丸・色・文字のみを設定したボタンの基底。中身と横幅は呼び出し側が追加する。
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
        // 太字の 18.66px 以上は WCAG の「大きい文字」（コントラスト比 3:1 で可）。ボタン文字色の検証はこの前提に基づく。
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
        quiet: false,
        ghost: matches!(variant, Variant::Ghost),
        background_opacity: 1.0,
        tooltip: None,
        disabled: false,
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
        let (rest, hover, active) = self.colors();
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
            active
        } else if p.hovered {
            hover
        } else {
            rest
        };

        let now = crate::drive::now();
        let color = if f.to == target {
            let (c, moving) = f.at(now);
            if moving {
                keep_drawing(window, cx);
            }
            c
        } else if cx.has_global::<Motion>() {
            // 目標色が変化した。現在の色から新しい目標へ遷移を再開する。
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

        // ポインタの状態を更新し再描画を要求する。
        let press = state.clone();
        // ツールチップの所有者。このボタンが描画されなくなり状態が破棄されると、要求も消滅する。
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
        let (id, tip, quiet) = (self.id, self.tooltip.clone(), self.quiet);
        let disabled = self.disabled;
        let (down, up) = (set(None, Some(true)), set(None, Some(false)));
        let up_out = set(None, Some(false));
        // ボタンごとに異なる角丸（`rounded*` で上書き済みのものを含む）。未指定の角は 0。
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
                        let ms = crate::drive::now()
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
                        // 角丸でクリッピングする。GPUI のマスクは矩形のみのため、角丸部分は 1px 幅の帯に分割し
                        // 帯ごとに角丸内側をマスクとして描画し、中間の矩形部分は一括描画する。
                        let (width, height) = (f32::from(b.size.width), f32::from(b.size.height));
                        let [tl, tr, br, bl] = clamp_corners(corners, width, height);
                        let (top, bottom) = (tl.max(tr), bl.max(br));
                        let at = |x: f32, y: f32, w: f32, h: f32| {
                            Bounds::new(b.origin + point(px(x), px(y)), size(px(w), px(h)))
                        };
                        let mut paint = |mask: Bounds<Pixels>| {
                            w.with_content_mask(Some(ContentMask::from_bounds(mask)), |w| {
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
        // GPUI の on_hover はウィンドウにつき一つのため、マウス移動ごとに要求を更新する。
        let (move_id, move_tip, move_owner) = (id.clone(), tip.clone(), owner.clone());
        self.base
            .bg(rgba(color))
            .children(wave)
            .on_mouse_move(move |_, w, cx| {
                if !quiet {
                    crate::cue::hover(&move_id, w, cx);
                }
                if let Some(tip) = &move_tip {
                    hover_tooltip(
                        move_id.clone(),
                        tip.clone(),
                        move_owner.clone(),
                        true,
                        w,
                        cx,
                    );
                }
            })
            .on_hover(move |h, w, cx| {
                if *h && !quiet {
                    crate::cue::hover(&id, w, cx);
                }
                if let Some(tip) = &tip {
                    hover_tooltip(id.clone(), tip.clone(), owner.clone(), *h, w, cx);
                }
                if *h { over(w, cx) } else { leave(w, cx) }
            })
            .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                if !disabled {
                    crate::cue::emit(crate::cue::Cue::Click, w, cx);
                }
                if enabled {
                    press.update(cx, |s, _| {
                        s.ripple = Some((e.position, crate::drive::now()))
                    });
                }
                down(w, cx)
            })
            .on_mouse_up(MouseButton::Left, move |_, w, cx| up(w, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, w, cx| up_out(w, cx))
    }
}

pub use crate::drive::keep_drawing;

/// プログレスリング: 一辺 `size` px・太さ `stroke` px の円弧。溝（`groove`）の上に、上から時計回りに `progress`（0〜1）
/// 分だけアクセント色で描画する。中身（残り時間等）は呼び出し側が `relative` の親で重ねる。
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
            // 円弧を折れ線で近似する（180° 超の弧での方向誤りを回避）。
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

/// 読み込み中スピナー。一辺 `size` px の溝（`groove`）の上を、1/4 周の丸端弧（`accent_solid`）が 0.5 秒で一周する。
/// ciel / website_V3 のスピナーと同一形状（半径 20・線幅 6・弧は周の 1/4）および速度。時刻は全部品で共通のため、
/// 複数配置しても同じ角度で回転する。
pub fn spinner(theme: &Theme, size: f32) -> impl IntoElement {
    let (track, fill) = (theme.groove, theme.accent_solid);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            keep_drawing(window, cx);
            let side = f32::from(bounds.size.width.min(bounds.size.height));
            let scale = side / SPINNER_VIEW;
            let stroke = px(SPINNER_STROKE * scale);
            let r = px(SPINNER_RADIUS * scale);
            let center = bounds.center();
            let turns = spinner_turns(crate::drive::now());
            // 円弧を折れ線で近似する（progress_ring と同様）。`from` は始点、`len` は長さ（いずれも周の割合）。
            let arc = |from: f32, len: f32| {
                let mut path = gpui::PathBuilder::stroke(stroke);
                let steps = (len * 96.0).ceil().max(1.0) as usize;
                for i in 0..=steps {
                    let a = (from + len * i as f32 / steps as f32) * std::f32::consts::TAU;
                    let p = point(center.x + r * a.cos(), center.y + r * a.sin());
                    if i == 0 {
                        path.move_to(p)
                    } else {
                        path.line_to(p)
                    }
                }
                path.build().ok()
            };
            if let Some(path) = arc(0.0, 1.0) {
                window.paint_path(path, rgba(track));
            }
            // 弧は真上（-90°）から時計回りに描画する。丸端は線の両端に半円を付加して生成する。
            let start = turns - 0.25;
            if let Some(path) = arc(start, SPINNER_ARC) {
                window.paint_path(path, rgba(fill));
            }
            for turn in [start, start + SPINNER_ARC] {
                let a = turn * std::f32::consts::TAU;
                let cap = point(center.x + r * a.cos(), center.y + r * a.sin());
                let rad = stroke / 2.0;
                let mut path = gpui::PathBuilder::fill();
                path.move_to(point(cap.x + rad, cap.y));
                for i in 1..=24 {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    path.line_to(point(cap.x + rad * t.cos(), cap.y + rad * t.sin()));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgba(fill));
                }
            }
        },
    )
    .flex_none()
    .size(px(size))
}

/// ciel の SVG（`viewBox="0 0 50 50"`, `r=20`, `stroke-width=6`, `stroke-dasharray="31.4 94.2"`）由来の寸法。
const SPINNER_VIEW: f32 = 50.0;
const SPINNER_RADIUS: f32 = 20.0;
const SPINNER_STROKE: f32 = 6.0;
/// 弧の長さ（周に対する割合）。31.4 / (31.4 + 94.2) = 1/4。
const SPINNER_ARC: f32 = 0.25;
/// 一周に要する時間（ms）。
const SPINNER_PERIOD_MS: u128 = 500;

/// 時刻 `now` における回転量（周の割合 0..1）。等速（linear）。
fn spinner_turns(now: Instant) -> f32 {
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let origin = *ORIGIN.get_or_init(|| now);
    let ms = now.saturating_duration_since(origin).as_millis() % SPINNER_PERIOD_MS;
    ms as f32 / SPINNER_PERIOD_MS as f32
}

/// ステータス画面: 上下左右中央に「アイコン・タイトル・（任意）理由・（任意）ボタン」を縦に配置する。読み込み中は [`spinner`]、
/// エラーは X アイコンを `icon` に渡す。パネル全体を使用する（[`card_panel`]）。
pub fn status_screen(
    theme: &Theme,
    icon: impl IntoElement,
    title: impl IntoElement,
    reason: Option<SharedString>,
    action: Option<gpui::AnyElement>,
) -> Div {
    card_panel(theme)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(spacing::LG))
        .p(px(spacing::XL))
        .child(icon)
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(spacing::XS))
                .text_center()
                .child(div().font_weight(FontWeight::BOLD).child(title))
                .children(
                    reason
                        .map(|reason| div().text_color(rgba(theme.neutral_text_low)).child(reason)),
                ),
        )
        .children(action)
}

/// [`status_screen`] 上に配置するアイコン。一辺 `size` px の枠内に、[`spinner`] の外径と同じ視覚的サイズで
/// グリフを描画する。Phosphor のグリフは枠の 208/256 しか使わないため、その分拡大して枠サイズは維持する
/// （スピナーと入れ替わってもレイアウトがずれない）。
pub fn status_icon(glyph: &'static crate::Icon, size: f32) -> impl IntoElement {
    let ring = (2.0 * SPINNER_RADIUS + SPINNER_STROKE) / SPINNER_VIEW;
    let drawn = size * ring / (208.0 / 256.0);
    div()
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(div().flex_none().child(crate::icon(glyph).size(px(drawn))))
}

/// オン / オフのスイッチ。押下時の処理は呼び出し側が `on_click` で付与する。
pub fn switch(theme: &Theme, id: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let (w, h, pad) = (44.0, 24.0, 2.0);
    let knob = h - pad * 2.0;
    let id: SharedString = id.into();
    let knob = SwitchKnob {
        id: format!("{id}-knob").into(),
        on,
        size: knob,
        // ノブの移動距離。
        travel: w - pad * 2.0 - knob,
        pad,
        track: (theme.groove, theme.accent_solid),
        knob_color: (theme.card, theme.on_accent),
    };
    div()
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .flex()
        .items_center()
        .w(px(w))
        .h(px(h))
        .rounded(px(radius::FULL))
        .relative()
        .child(knob)
}

/// [`switch`] のノブ。オンオフの切り替え時に `Motion::Toggle` でスライドする（オーバーシュートなし）。
#[derive(IntoElement)]
struct SwitchKnob {
    id: SharedString,
    on: bool,
    size: f32,
    travel: f32,
    /// ノブ外側のパディング。
    pad: f32,
    /// 背景色（オフ, オン）。
    track: (u32, u32),
    /// ノブ色（オフ, オン）。
    knob_color: (u32, u32),
}

impl RenderOnce for SwitchKnob {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::animation::{Motion as Kind, Transition};
        let now = crate::drive::now();
        let animated = cx.has_global::<Motion>();
        let to = if self.on { 1.0 } else { 0.0 };
        let state = window.use_keyed_state(self.id, cx, move |_, _| Transition::rest(to));
        state.update(cx, |t, _| t.go(to, Kind::Toggle, now, animated));
        let (at, moving) = state.read(cx).at(now);
        if moving {
            keep_drawing(window, cx);
        }
        let at = at.clamp(0.0, 1.0);
        let c = |(off, on): (u32, u32)| rgba(mix(off, on, at));
        // 背景はスイッチ全体に敷き、ノブと同じ進行度で色が遷移する。
        div()
            .flex()
            .items_center()
            .absolute()
            .inset_0()
            .px(px(self.pad))
            .rounded(px(radius::FULL))
            .bg(c(self.track))
            .child(
                div()
                    .flex_none()
                    .size(px(self.size))
                    .rounded(px(radius::FULL))
                    .bg(c(self.knob_color))
                    .ml(px(self.travel * at)),
            )
    }
}

pub use crate::PANEL_MARGIN;
use crate::{PANEL_SHADOW_ALPHA, PANEL_SHADOW_BLUR};

/// パネル基底の配置: ウィンドウの縁から [`PANEL_MARGIN`] 内側を埋め、その余白に影を描画する。
/// サイズは `absolute` と `inset` で決定されるため、呼び出し側は `.relative()` や `.size_full()` で上書きしない
/// （`absolute` のままで、内部の `absolute` 子要素の基準点となる）。
fn panel_base(div: Div) -> Div {
    div.absolute()
        .inset(px(PANEL_MARGIN))
        .shadow(panel_shadow())
}

/// パネルの影（オフセットなしで四方へ拡散）。
fn panel_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0.0, 0.0, 0.0, PANEL_SHADOW_ALPHA),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(PANEL_SHADOW_BLUR),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// パネル全体の背景。角丸 `XL`、内部との間隔 `MD`（12）のため、内部カードの角丸は同心の `XL − MD = LG`。
/// カードを複数配置する画面でのみ使用し、中身は [`card`] に入れる。カードが 1 枚なら [`card_panel`]。
pub fn panel(theme: &Theme) -> Div {
    panel_base(div())
        .p(px(spacing::MD))
        .rounded(px(radius::XL))
        .bg(rgba(theme.background))
        .text_color(rgba(theme.neutral_text))
}

/// 背景（[`panel`]）の上に配置するカード。要素は原則としてカード内に配置する。
pub fn card(theme: &Theme) -> Div {
    div()
        .rounded(px(radius::LG))
        .bg(rgba(theme.card))
        .text_color(rgba(theme.neutral_text))
}

/// 中身の幅に収まるカードパネル（[`card_panel`] の、ウィンドウ全体に広がらない版）。配置は呼び出し側が決定する。
pub fn card_panel_fit(theme: &Theme) -> Div {
    card(theme).rounded(px(radius::XL)).shadow(panel_shadow())
}

/// カード 1 枚のみの画面: 背景（[`panel`]）を敷かず、カードがパネル全体を占める。角丸はパネルの `XL`。
pub fn card_panel(theme: &Theme) -> Div {
    panel_base(card(theme)).rounded(px(radius::XL))
}

/// ナビゲーションバーの高さ（ボタン + 上下 `SM`）。カプセル形のため、ボタン（`FULL`）の角丸はバーの角丸と同心。
pub const NAV_BAR_HEIGHT: f32 = BUTTON_HEIGHT + 2.0 * spacing::SM;

/// 非ホバー時のナビゲーションバー（丸い線）の幅（バーに対する割合）と高さ（px）。
const NAV_LINE_WIDTH: f32 = 0.4;
const NAV_LINE_HEIGHT: f32 = spacing::MD;

/// ナビゲーションバー: パネル下部に配置し、パネルを操作する帯。左にタイトル（左寄せ）、右端に閉じるボタン（アイコンのみ、
/// id は `nav-close`）。`minimize` 指定時は最小化ボタン（マイナス、id は `nav-minimize`）に差し替わる。
/// カード 1 枚の帯で角丸 `FULL`。押下時の処理（閉じる・最小化）は `on_close` で受け取る。
/// ボタンはすべて右寄せ: `trailing`（パネル固有の操作）は閉じるボタンの左に、指定順で左から配置する。
/// バーはウィンドウ縁から [`PANEL_MARGIN`] 内側に配置し（影の余白）、高さは [`NAV_BAR_HEIGHT`]。
///
/// `open`（0〜1）は展開度。0（非ホバー時）はボタンもタイトルも表示しない丸い線で、バー上端に寄せる。
/// 展開時はまず線がバーの上下中央へ降り、そこを中心に上下左右へ広がる（縮小時は逆に、縮んでから上がる）。
/// 配置アンカー（バー上端の左右中央。所有者に付与・先頭からの距離で拡大に使用）とは別に、
/// 展開アンカー（バーの中心）を持つ。中身はバー内に配置してバーと連動し、展開後半で不透明度のみで出現する。
pub fn nav_bar(
    theme: &Theme,
    open: f32,
    title: impl Into<SharedString>,
    minimize: bool,
    close_tooltip: impl Into<SharedString>,
    on_close: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    trailing: Vec<Button>,
) -> Div {
    let (close_id, close_glyph) = if minimize {
        ("nav-minimize", &crate::icons::MINUS)
    } else {
        ("nav-close", &crate::icons::X)
    };
    // 前半で移動し、後半で展開する。各区間内も ease-in-out を適用し、つなぎ目で急変しないようにする。
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
    // 展開の中心（バーの上下中央）。移動前は、線の上端がバーの上端に位置するまで上方へオフセットする。
    let center_offset = lerp(-(NAV_BAR_HEIGHT - NAV_LINE_HEIGHT) / 2.0, 0.0, moved);
    // タイトルはバー左寄せ。長い場合もボタンに重ならないよう、右にボタン分の余白を確保して省略する。
    let buttons = (1 + trailing.len()) as f32 * (BUTTON_HEIGHT + spacing::SM) + spacing::SM;
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
                    // 縮小時の線はダークテーマでも暗い背景に埋もれないようライトの色で描画し、展開に伴い現テーマの色へ遷移する。
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
                            .pl(px(spacing::XL))
                            .pr(px(buttons))
                            .text_color(rgba(theme.neutral_text))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
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
                                    .children(trailing)
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
                            ),
                    ),
            ),
    )
}

/// リスト行: 左にアイコンと文字、右に `>`。[`list`] のカード内に並べる前提で、静止時はカードと同色、
/// ホバー・押下で `control-hover` / `control-active` に変化する（区切り線とカードの角丸は [`list`] が付与する）。
/// 選択中の行は `.selected(true)`（押下色を維持）。高さはボタンと同一。
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

/// メニュー行: リスト行（[`list_item`]）と同一だが、右の `>` がない（その場で実行する項目。遷移を伴うリストではない）。
pub fn menu_item(
    theme: &Theme,
    id: impl Into<SharedString>,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
) -> Button {
    menu_item_with(theme, id, crate::icon(glyph).size(px(font_size::LG)), label)
}

/// [`menu_item`] の先頭に任意の要素（アプリアイコンの画像など）を置く版。大きさは呼び出し側が `font_size::LG` に合わせる。
pub fn menu_item_with(
    theme: &Theme,
    id: impl Into<SharedString>,
    glyph: impl IntoElement,
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
        // 行は文字主体のボタンではないため太字にしない（文字サイズは本文と同じ `MD`）。
        .font_weight(FontWeight::NORMAL)
        .child(glyph)
        .child(div().flex_1().min_w_0().truncate().child(label.into()));
    b
}

/// タイル: 押下可能なカード。グリッドに配置する（ダッシュボードのソース・クイック設定）。背景（[`panel`]）に直接配置し、
/// 静止時はカードと同色、ホバー・押下で `control-hover` / `control-active`。有効トグルは `.selected(true)`。
/// 中身は縦に中央揃えで積む。サイズと中身（アイコン・文字）は呼び出し側が追加する。
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

/// 選択カード（[`choice`]）の選択時の枠線太さと、右上チェックバッジの直径（px）。
const CHOICE_RING: f32 = 3.0;
const CHOICE_BADGE: f32 = 24.0;

/// 選択カード: 複数の中から 1 つ選ぶ、見本や画像を載せたカード（背景上に配置。[`tile`] と同様カード色で、ホバー・押下で変化）。
/// 選択中のカードはアクセント色の枠で囲み、右上にチェックバッジを表示する（色の差異のみに依存しない）。
/// 中身 `content` は中央に配置する（バッジは中身の上に重ねるため、中身はここで受け取る）。高さは中身に合わせ、幅は呼び出し側が追加する。
pub fn choice(
    theme: &Theme,
    id: impl Into<SharedString>,
    selected: bool,
    content: impl IntoElement,
) -> Button {
    let mut b = base_button(theme, id.into(), Variant::Default);
    (b.rest, b.hover, b.active) = (theme.card, theme.control_hover, theme.control_active);
    let ring = if selected { theme.accent_solid } else { 0 };
    b.base = b
        .base
        .relative()
        .h_auto()
        .flex_col()
        .gap(px(spacing::SM))
        .p(px(spacing::MD))
        .rounded(px(radius::LG))
        .border(px(CHOICE_RING))
        .border_color(rgba(ring))
        .font_weight(FontWeight::NORMAL)
        .child(content);
    if selected {
        let check = crate::icon(&crate::icons::CHECK)
            .weight(crate::Weight::Bold)
            .size(px(font_size::SM))
            .color(rgba(theme.on_accent));
        b.base = b.base.child(
            div()
                .absolute()
                .top(px(spacing::SM))
                .right(px(spacing::SM))
                .size(px(CHOICE_BADGE))
                .rounded(px(radius::FULL))
                .bg(rgba(theme.accent_solid))
                .flex()
                .items_center()
                .justify_center()
                .child(check),
        );
    }
    b
}

/// 背景上に配置するタイル（アプリ一覧のアプリ等）: [`tile`] と同様に縦配置だが、カード背景を敷かず、ホバー・押下時のみ
/// 色が付く。通常時は背景（`background`）と同色（透明にするとホバー色からのフェードアウトが途切れる。[`sidebar_item`] 参照）。
pub fn ghost_tile(theme: &Theme, id: impl Into<SharedString>) -> Button {
    let mut b = tile(theme, id);
    (b.rest, b.hover, b.active) = (
        theme.background,
        theme.control_on_background_hover,
        theme.control_on_background_active,
    );
    b
}

/// サイドバー行の左端インジケーター（選択行のみ）の高さと幅（px）。
pub const SIDEBAR_INDICATOR: (f32, f32) = (24.0, 4.0);
/// インジケーターと行左端の間隔。行の背景（丸い端）内に収め、アイコンとも `MD` 離す。
pub const SIDEBAR_INDICATOR_INSET: f32 = spacing::SM;

/// サイドバー行: 左にアイコンと文字。サイドバーの地（`sidebar`）上に配置する前提で、通常時は地と同色（背景が無いように
/// 見せる。透明にするとホバー色からのフェードアウトが途切れるため地色を使用）、ホバー・押下で `control-on-background-*`。
/// 選択行（`selected`）は文字が `neutral-text`（ダークでは白。左端インジケーターは [`sidebar`] が行ごとに描画・アニメーション）。非選択行は
/// `neutral-text-low`（選択行より薄いが、地とのコントラスト比 4.5 を維持）。高さはボタンと同一。
pub fn sidebar_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
    selected: bool,
) -> Button {
    let mut b = base_button(theme, id.into(), Variant::OnBackground);
    (b.rest, b.hover, b.active) = (
        theme.sidebar,
        theme.control_on_background_hover,
        theme.control_on_background_active,
    );
    let fg = if selected {
        theme.neutral_text
    } else {
        theme.neutral_text_low
    };
    b.ink = fg;
    b.base = b
        .base
        .w_full()
        .justify_start()
        .rounded(px(radius::LG))
        .gap(px(spacing::MD))
        .pl(px(SIDEBAR_INDICATOR_INSET
            + SIDEBAR_INDICATOR.1
            + spacing::MD))
        .pr(px(spacing::LG))
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgba(fg))
        .child(crate::icon(glyph).size(px(font_size::LG)))
        .child(div().flex_1().min_w_0().truncate().child(label.into()));
    b
}

/// サイドバー行（[`sidebar_item`]）を `XS` 間隔で縦に並べ、`selected` 番目の行の左端にアクセント（`accent-solid`）のインジケーターを描画する。
/// インジケーターは行ごとに保持し、選択行の変更時に旧行のインジケーターは中央へ縮んで消え、同時に新行のインジケーターが中央から伸びて出現する
/// （消滅は `Motion::Hop`（ease-out）、出現は `Motion::HopIn`（ease-in-out））。縮小時は上下に幅と同じ高さ（1:1）まで縮んでから全体が縮小し、出現時はその逆
/// （幅より低く潰れることはない）。インジケーターの出現状態は `id` ごとに保持する。
pub fn sidebar(
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    id: &'static str,
    selected: usize,
    items: impl IntoIterator<Item = Button>,
) -> Div {
    use crate::animation::{Motion as Kind, Transition};
    let (h, w) = SIDEBAR_INDICATOR;
    let items: Vec<Button> = items.into_iter().collect();
    let shown = |i: usize| if i == selected { 1.0 } else { 0.0 };
    let now = crate::drive::now();
    let animated = cx.has_global::<Motion>();
    // 行ごとのインジケーター出現度（0 = 非表示、1 = 完全表示）。
    let state = window.use_keyed_state(id, cx, |_, _| Vec::<Transition>::new());
    let shapes: Vec<(f32, bool)> = state.update(cx, |ts, _| {
        // 初期表示時点で選択行のインジケーターは表示済み。
        while ts.len() < items.len() {
            ts.push(Transition::rest(shown(ts.len())));
        }
        ts.iter_mut()
            .enumerate()
            .map(|(i, t)| {
                let kind = if i == selected {
                    Kind::HopIn
                } else {
                    Kind::Hop
                };
                t.go(shown(i), kind, now, animated);
                t.at(now)
            })
            .collect()
    });
    if shapes.iter().any(|&(_, moving)| moving) {
        keep_drawing(window, cx);
    }
    // 出現度 `p` のインジケーターサイズ: 高さは `p` に比例し、幅は高さに達するまで固定（1:1 の点から縦に伸びる）。
    let marks = shapes
        .into_iter()
        .enumerate()
        .filter_map(move |(i, (p, _))| {
            let height = p.max(0.0) * h;
            let width = height.min(w);
            let center = i as f32 * (BUTTON_HEIGHT + spacing::XS) + BUTTON_HEIGHT / 2.0;
            (height > 0.0).then(|| {
                div()
                    .absolute()
                    .left(px(SIDEBAR_INDICATOR_INSET + (w - width) / 2.0))
                    .top(px(center - height / 2.0))
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(radius::FULL))
                    .bg(rgba(theme.accent_solid))
            })
        });
    div()
        .relative()
        .flex()
        .flex_col()
        .gap(px(spacing::XS))
        .children(items)
        // 行の後に配置し、行の背景より上に描画する。
        .children(marks)
}

/// 行（[`list_item`]）をカード 1 枚に縦に並べ、行間に 1px の区切り線を描画する。
/// 先頭・末尾の行はカードの角丸（`LG`）に合わせて丸める（行の塗りがカードの角からはみ出さない）。
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

/// メニューの外側パディング（カード縁と行の間）。行のホバー色がカード縁からわずかに離れる幅。
pub const MENU_PADDING: f32 = spacing::XS;
/// メニューカードの角丸。行の角丸（`LG`）にパディングを加えた同心の値。
pub const MENU_RADIUS: f32 = radius::LG + MENU_PADDING;
/// メニュー行の高さ。リスト行（ボタンの高さ）より詰め、最小タッチ領域（48px）ちょうどにする。
pub const MENU_ITEM_HEIGHT: f32 = 48.0;
/// メニュー行の左右パディング。カードのパディングと合計して、文字がカード縁から `LG` 離れる。
const MENU_ITEM_PADDING: f32 = spacing::MD;

/// 選択行: メニュー行（[`menu_item`]）の右に現在の値と開閉キャレット（`caret-up-down`）を配置する。押下時に呼び出し側が
/// [`select_popover`] を `.child` で付与して選択肢を開く（行の右下に右端を揃えて浮遊）。
pub fn select(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Button {
    // 右の要素が独自のパディングを持つため、行の右パディングは詰める。
    let mut b = inert(menu_item(theme, id, glyph, label)).pr(px(spacing::SM));
    let pick_h = BUTTON_HEIGHT - spacing::SM * 2.0;
    let mut pick = base_button(theme, format!("{id}-pick").into(), Variant::Default);
    (pick.rest, pick.hover, pick.active) = (theme.card, theme.control_hover, theme.control_active);
    pick.base = pick
        .base
        .relative()
        .gap(px(spacing::SM))
        // パディングは縦横 1:1: 行の縁との間は上下右とも `SM`、文字の周りも上下左右同一。
        .px(px((pick_h - font_size::MD) / 2.0))
        .h(px(pick_h))
        .rounded(px(radius::MD))
        .font_weight(FontWeight::NORMAL)
        .text_color(rgba(theme.neutral_text_low))
        .child(div().min_w_0().truncate().child(value.into()))
        .child(
            crate::icon(&crate::icons::CARET_UP_DOWN)
                .size(px(font_size::MD))
                .color(rgba(theme.neutral_text_low)),
        );
    // クリック処理は行が受ける（行の `on_click`）。ホバーと押下の演出はこの要素のみ。
    b.base = b.base.relative().child(pick);
    b
}

/// 行はボタンのままだが、ホバー・押下の色変化と波紋を抑制する（演出は内部要素に委譲する）。
fn inert(mut b: Button) -> Button {
    (b.hover, b.active, b.ripple, b.quiet) = (b.rest, b.rest, false, true);
    b
}

/// オン / オフ行: メニュー行（[`menu_item`]）の右に [`switch`]（`{id}-switch`）を配置する。行のどこを押しても切り替わる（行のホバー色に
/// スイッチが埋もれないよう、行はホバー色を出さない）。押下時の処理は呼び出し側が `on_click` で付与する。
pub fn switch_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
    on: bool,
) -> Button {
    let mut b = inert(menu_item(theme, id, glyph, label));
    b.base = b.base.child(switch(theme, format!("{id}-switch"), on));
    b
}

/// スライダー行: メニュー行（[`menu_item`]）の右に現在値（`{id}-value`）と [`slider`] を配置する。行自体は操作不可（色変化・波紋・効果音なし）。
/// 値の文字はラベル側に配置し、スライダーは右端に寄せる（文字の長さ・サイズが変わっても、操作中のスライダーが移動しない）。
pub fn slider_item(
    theme: &Theme,
    id: &'static str,
    glyph: &'static crate::Icon,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    slider: Slider,
) -> Button {
    let mut b = inert(menu_item(theme, id, glyph, label));
    b.disabled = true;
    let value_id = format!("{id}-value");
    b.base = b
        .base
        .child(
            div()
                .debug_selector(move || value_id.clone())
                .flex_none()
                .text_color(rgba(theme.neutral_text_low))
                .child(value.into()),
        )
        .child(slider);
    b
}

/// 選択肢ポップオーバーが開く際の降下距離（px）。
pub const SELECT_POPOVER_RISE: f32 = spacing::SM;

/// 選択肢ポップオーバーの最小幅（px）。幅は中身（最長の選択肢）に合わせ、短い場合のみこの値まで拡張する。
pub const SELECT_POPOVER_MIN_WIDTH: f32 = 200.0;

/// 選択肢の行（[`select_popover`] 内に配置）: 文字のみで、選択中の行は右端にチェックマークを表示する。
pub fn select_option(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Button {
    let mut b = base_button(theme, id.into(), Variant::Default);
    (b.rest, b.hover, b.active) = (theme.card, theme.control_hover, theme.control_active);
    b.base = b
        .base
        .w_full()
        .justify_start()
        .gap(px(spacing::LG))
        .font_weight(FontWeight::NORMAL)
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
        .children(selected.then(|| crate::icon(&crate::icons::CHECK).size(px(font_size::MD))));
    b
}

/// ドロップダウンボタン: 現在の値と開閉キャレット（[`select`] と同じ `caret-up-down`）を表示する。行内ではなく単独で配置する、
/// 複数から 1 つ選ぶ操作（行内なら [`select`]）。`glyph` 指定時は値の左に表示する。押下時に呼び出し側が [`dropdown_popover`] を
/// `.child` で付与して選択肢を開く。
pub fn dropdown(
    theme: &Theme,
    id: impl Into<SharedString>,
    variant: Variant,
    glyph: Option<&'static crate::Icon>,
    value: impl Into<SharedString>,
) -> Button {
    let mut b = content_button(theme, id, variant);
    let caret = crate::icon(&crate::icons::CARET_UP_DOWN).size(px(font_size::MD));
    b.base = b
        .base
        .relative()
        // 文字側は広め、キャレット側は狭め（キャレットは独自のパディングを持つ）。
        .pl(px(spacing::XL))
        .pr(px(spacing::LG))
        .font_weight(FontWeight::NORMAL)
        .children(glyph.map(|g| crate::icon(g).size(px(font_size::LG))))
        .child(div().min_w_0().truncate().child(value.into()))
        .child(caret);
    b
}

/// 選択肢ポップオーバーの表示位置（開いた要素を基準とする）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Popover {
    /// 右下に右端揃えで配置（[`select`] の行）。
    BelowEnd,
    /// 下に左右中央揃えで配置（[`dropdown`]）。
    Below,
    /// 上に左端揃えで配置（画面下隅に配置した [`dropdown`]）。
    AboveStart,
}

/// [`dropdown`] の選択肢ポップオーバー: [`select_popover`] と同一で、ボタンの下に左右中央揃えで表示する。`up` の場合は
/// （画面下隅に配置したボタン用に）上に左端揃えで表示する。
#[allow(clippy::too_many_arguments)]
pub fn dropdown_popover(
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    id: &'static str,
    open: bool,
    up: bool,
    items: impl IntoIterator<Item = Button>,
    on_dismiss: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let place = if up {
        Popover::AboveStart
    } else {
        Popover::Below
    };
    popover(theme, window, cx, id, open, place, items, on_dismiss)
}

/// 選択肢ポップオーバー（[`select`] の子に配置）: 行の右下に右端揃えで、メニューカード（[`menu`]）に影と縁取りを付けて浮遊させる。
/// 背面にウィンドウ全体（`viewport`）を覆う透明な板を敷き、外部クリック時にそれが受けて `on_dismiss` を呼ぶ（下の要素は押されない。
/// 開いている行の再クリックでも、閉じてすぐ再度開くことはない）。
/// 行（`id`）ごとに毎回呼び、`open` で開閉する。開くと [`SELECT_POPOVER_RISE`] 上方から降りながら出現し（`Motion::Enter`）、
/// 閉じるとその逆で消滅する（`Motion::Exit`）。消滅完了まで描画を継続し、閉じ始めた時点で背面の板は除去する。
pub fn select_popover(
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    id: &'static str,
    open: bool,
    items: impl IntoIterator<Item = Button>,
    on_dismiss: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    popover(
        theme,
        window,
        cx,
        id,
        open,
        Popover::BelowEnd,
        items,
        on_dismiss,
    )
}

#[allow(clippy::too_many_arguments)]
fn popover(
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    id: &'static str,
    open: bool,
    place: Popover,
    items: impl IntoIterator<Item = Button>,
    on_dismiss: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    use crate::animation::{Motion as Kind, Transition};
    let now = crate::drive::now();
    let animated = cx.has_global::<Motion>();
    let state = window.use_keyed_state(SharedString::from(format!("{id}-popover")), cx, |_, _| {
        Transition::rest(0.0)
    });
    let to = if open { 1.0 } else { 0.0 };
    state.update(cx, |t, _| {
        t.go(
            to,
            if open { Kind::Enter } else { Kind::Exit },
            now,
            animated,
        )
    });
    let (shown, moving) = state.read(cx).at(now);
    if moving {
        keep_drawing(window, cx);
    }
    if !open && !moving {
        // 行の並びに影響しないよう、空でも浮遊させる。
        return div().absolute();
    }
    let viewport = window.viewport_size();
    // 開いた要素の縁上の、サイズゼロの点。ここにカードの角（または辺の中央）を合わせる。
    let (at, anchor) = match place {
        Popover::BelowEnd => (div().right_0().bottom_0(), gpui::Anchor::TopRight),
        Popover::Below => (
            div().left(gpui::relative(0.5)).bottom_0(),
            gpui::Anchor::TopCenter,
        ),
        Popover::AboveStart => (div().left_0().top_0(), gpui::Anchor::BottomLeft),
    };
    // 開いた要素との間隔は `XS`。出現時は、その向こう側から [`SELECT_POPOVER_RISE`] 分だけ接近する。
    let gap = px(spacing::XS - SELECT_POPOVER_RISE * (1.0 - shown));
    let card = menu(theme, items)
        .id("select-popover")
        .occlude()
        .min_w(px(SELECT_POPOVER_MIN_WIDTH))
        .opacity(shown)
        // ダークテーマではカード上に重なると同色で境目が消えるため、影に加えて薄い線で縁取る。
        .border_1()
        .border_color(rgba(theme.neutral_border_subtle))
        .shadow_md();
    let card = if place == Popover::AboveStart {
        card.mb(gap)
    } else {
        card.mt(gap)
    };
    at.absolute()
        .children(open.then(|| {
            deferred(
                anchored()
                    .position_mode(gpui::AnchoredPositionMode::Window)
                    .position(point(px(0.0), px(0.0)))
                    .child(
                        div()
                            .id("select-backdrop")
                            .w(viewport.width)
                            .h(viewport.height)
                            .occlude()
                            .on_mouse_down(MouseButton::Left, on_dismiss),
                    ),
            )
        }))
        .child(deferred(anchored().anchor(anchor).snap_to_window().child(card)).priority(1))
}

/// メニュー行（[`menu_item`]）を縦に配置する。区切り線は描画せず、行はカード（[`MENU_RADIUS`]）と同心の角丸にし、ホバー・押下時のみ変化する。
/// 行はリストより詰める（高さ [`MENU_ITEM_HEIGHT`]、左右 [`MENU_ITEM_PADDING`]）。
pub fn menu(theme: &Theme, items: impl IntoIterator<Item = Button>) -> Div {
    // 内側のカードも外のパネルと同じ角丸にする（小さいと、外の角が背景色で塗りつぶされ丸みが変化しないように見える）。
    card(theme)
        .rounded(px(MENU_RADIUS))
        .flex()
        .flex_col()
        .p(px(MENU_PADDING))
        .children(items.into_iter().map(|b| {
            b.h(px(MENU_ITEM_HEIGHT))
                .px(px(MENU_ITEM_PADDING))
                .rounded(px(MENU_RADIUS - MENU_PADDING))
        }))
}

/// 見出しアイコンのサイズ。
pub(crate) const HEADING_ICON: f32 = 64.0;
/// 見出しテキストの行の高さ（文字サイズに対する比率）。
pub(crate) const HEADING_LEADING: f32 = 1.4;
/// 見出しの間隔: アイコン → タイトル（タイトル → 概要の倍。アイコンとテキストを分離し、テキスト同士は一体に見せる）。
const HEADING_HERO_GAP: f32 = spacing::LG;
/// 見出しの間隔: タイトル → 説明。
const HEADING_GAP: f32 = spacing::SM;

/// 見出し: 上からアイコン等（`hero`、`{id}-hero`）・タイトル（`{id}-title`）・説明（`{id}-description`）を左揃えで縦配置する。
/// ダイアログとウィザードの画面で共通の見た目: アイコン → タイトルは [`HEADING_HERO_GAP`]、タイトル → 説明は [`HEADING_GAP`]。
/// タイトルは `LG` の太字、説明は `MD` の `text-low`（行の高さはいずれも [`HEADING_LEADING`]）。説明の内容形式は
/// 呼び出し側が決定するため、`description` は文字列でもスクロール可能な枠でもよい。空タイトル・`None` の説明は表示しない。
/// 中央揃えにする場合は、返された列に `items_center().text_center()` を付与する。
pub(crate) fn heading(
    theme: &Theme,
    id: &str,
    hero: impl IntoElement,
    title: SharedString,
    description: Option<impl IntoElement>,
) -> Div {
    let (hero_id, title_id, description_id) = (
        format!("{id}-hero"),
        format!("{id}-title"),
        format!("{id}-description"),
    );
    let has_title = !title.is_empty();
    let has_text = has_title || description.is_some();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .debug_selector(move || hero_id)
                .when(has_text, |d| d.mb(px(HEADING_HERO_GAP)))
                .child(hero),
        )
        .children(has_title.then(|| {
            div()
                .id(SharedString::from(title_id.clone()))
                .debug_selector(move || title_id)
                .flex_none()
                .text_size(px(font_size::LG))
                .line_height(px(font_size::LG * HEADING_LEADING))
                .font_weight(FontWeight::BOLD)
                .child(title)
        }))
        .children(description.map(|description| {
            div()
                .id(SharedString::from(description_id.clone()))
                .debug_selector(move || description_id)
                .when(has_title, |d| d.mt(px(HEADING_GAP)))
                // 中身がスクロール枠の場合、見出し列が低い際に説明部分のみが縮小される。
                .min_h_0()
                .flex()
                .flex_col()
                .text_size(px(font_size::MD))
                .line_height(px(font_size::MD * HEADING_LEADING))
                .text_color(rgba(theme.neutral_text_low))
                .child(description)
        }))
}

/// ダイアログパネルの幅（px。影の余白を含む。1px = 1mm）。ボタンを横に並べ、テキストは 1 行約 24 文字。
pub const DIALOG_WIDTH: u32 = 560;
/// 説明の表示行数上限。超過分はスクロールする。
const DIALOG_LINES: usize = 3;

/// ダイアログパネルの高さ（px。影の余白を含む。幅は [`DIALOG_WIDTH`]）。
/// タイトルと説明を実際に折り返して行数を計算する（説明は [`DIALOG_LINES`] 行まで）。開く際に（`PanelSpec::sized`）使用すれば、
/// パディングが過不足のない高さで開く。
// ponytail: 開いた後に言語が変わっても再計算しない（長くなった分は説明がスクロールする）。必要なら描画済みレイアウトから再計算する。
pub fn dialog_height(window: &Window, title: &str, description: &str) -> u32 {
    // テキスト幅 = パネル − 影の余白 − 縁（説明はさらに右に `SM`）。
    let inner = DIALOG_WIDTH as f32 - 2.0 * (PANEL_MARGIN + spacing::XL);
    // 行の高さとアイコンから算出するため、テキストスケールに合わせる（テキストの折り返しはテキストシステムが同じ倍率で計測する）。
    let scale = gpui::text_scale();
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
        inner - spacing::SM,
    )
    .min(DIALOG_LINES);
    let h = 2.0 * (PANEL_MARGIN + spacing::XL)
        + HEADING_ICON * scale
        + HEADING_HERO_GAP
        + title as f32 * font_size::LG * HEADING_LEADING * scale
        + HEADING_GAP
        + body as f32 * font_size::MD * HEADING_LEADING * scale
        + spacing::XL
        + BUTTON_HEIGHT;
    h.ceil() as u32
}

/// ダイアログ（子パネル）の内容。[`dialog`] で生成する。
#[derive(IntoElement)]
pub struct Dialog {
    id: &'static str,
    theme: Theme,
    glyph: &'static crate::Icon,
    /// アイコンのウェイト（既定は Fill。[`Dialog::weight`]）。
    weight: crate::Weight,
    title: SharedString,
    description: SharedString,
    actions: Vec<Button>,
}

impl Dialog {
    /// アイコンのウェイトを変更する（既定は Fill）。Fill では強すぎる、または形状が判別しにくいアイコン（電源等）に使用する。
    pub fn weight(mut self, weight: crate::Weight) -> Self {
        self.weight = weight;
        self
    }
}

/// ダイアログ（子パネル）の内容。上に見出し（アイコン・タイトル・説明。ウィザードの画面と共通の [`heading`]。左揃え）、
/// 下にボタンを均等幅で横 1 列に配置する。間隔は固定値で、全体を上下中央に配置する（余った高さは上下の縁に分配）。
/// 説明は [`DIALOG_LINES`] 行まで表示し、超過分やパネルに収まらない分は説明のみスクロールする（`{id}-body`、バーは `{id}-bars`）。
/// パネルは幅 [`DIALOG_WIDTH`]、高さ [`dialog_height`]。
/// ボタンは渡した順に左から配置: 肯定（主操作）を右端に、否定（キャンセル・閉じる）を左に。`id` はパネル内で一意とする。
/// 開閉・応答の返却は呼び出し側（`ui::PanelSpec::child`）が行う。
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
        // パディングは縁 `XL`、見出し内は [`heading`] の通り、テキスト → ボタン `XL`。
        // 説明枠は [`DIALOG_LINES`] 行までで、超過分はスクロールする。縮小可能（min_h_0）なのは見出し列と説明枠のみのため、
        // パネルが低くても説明のみが縮小される。
        // 右の `SM` はスクロールバーがテキストに重ならないための余白（左は付けず、テキスト左端をタイトルに揃える）。
        let body = div()
            .id(SharedString::from(body_id.clone()))
            .debug_selector(move || body_id.clone())
            .min_h_0()
            .max_h(crate::font::scaled(
                DIALOG_LINES as f32 * font_size::MD * HEADING_LEADING,
            ))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pr(px(spacing::SM))
            .child(description);
        let description = div()
            .relative()
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .child(scrollbars(&theme, format!("{id}-bars"), &scroll));
        let icon = crate::icon(glyph)
            .weight(weight)
            .size(px(HEADING_ICON))
            .color(rgba(theme.accent_solid));
        card_panel(&theme)
            .flex()
            .flex_col()
            .justify_center()
            .p(px(spacing::XL))
            .child(heading(&theme, id, icon, title, Some(description)).min_h_0())
            .child(
                div()
                    .mt(px(spacing::XL))
                    .flex_none()
                    .w_full()
                    .flex()
                    .gap(px(spacing::SM))
                    .children(actions.into_iter().map(|b| b.flex_1())),
            )
    }
}

/// テーマプレビューの窓 1 枚のサイズと、手前の窓を奥の窓からずらすオフセット（px）。
const PREVIEW_WINDOW: (f32, f32) = (176.0, 120.0);
const PREVIEW_SHIFT: (f32, f32) = (96.0, 44.0);

/// テーマプレビュー（テキストなしの図示、`theme-preview`）: 背景上に重なった 2 枚の窓。手前の窓にテキストを模した線とアクセント色のボタンを配置する。
/// テーマやパーソナルカラーの選択画面で、`theme` 適用結果を表示する。サイズは固定で、配置は呼び出し側が決定する。
pub fn theme_preview(theme: &Theme) -> Div {
    let line = |width: f32, color: u32| {
        div()
            .w(px(width))
            .h(px(6.0))
            .rounded(px(radius::FULL))
            .bg(rgba(color))
    };
    let window = || {
        div()
            .absolute()
            .w(px(PREVIEW_WINDOW.0))
            .h(px(PREVIEW_WINDOW.1))
            .p(px(spacing::LG))
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .rounded(px(radius::LG))
            // 窓は影で背景から浮かせる（枠線は使用しない）。
            .shadow_md()
            .bg(rgba(theme.card))
    };
    // ボタン上に載る色（`on-accent`）も短い線で表現する。
    let button = div()
        .w(px(52.0))
        .h(px(22.0))
        .rounded(px(radius::FULL))
        .circular_corners()
        .bg(rgba(theme.accent_solid))
        .flex()
        .items_center()
        .justify_center()
        .child(line(24.0, theme.on_accent).h(px(4.0)));
    let back = window()
        .top_0()
        .left_0()
        .child(line(56.0, theme.neutral_text_low))
        .child(line(96.0, theme.groove))
        .child(line(72.0, theme.groove));
    let front = window()
        .left(px(PREVIEW_SHIFT.0))
        .top(px(PREVIEW_SHIFT.1))
        .child(line(72.0, theme.neutral_text))
        .child(line(132.0, theme.neutral_text_low))
        .child(line(108.0, theme.neutral_text_low))
        .child(
            div()
                .flex_1()
                .flex()
                .items_end()
                .justify_end()
                .child(button),
        );
    div()
        .debug_selector(|| "theme-preview".into())
        .flex_none()
        .p(px(spacing::LG))
        .rounded(px(radius::LG))
        .bg(rgba(theme.background))
        .child(
            div()
                .relative()
                .w(px(PREVIEW_WINDOW.0 + PREVIEW_SHIFT.0))
                .h(px(PREVIEW_WINDOW.1 + PREVIEW_SHIFT.1))
                .child(back)
                .child(front),
        )
}

/// グループ: 小見出しの下に中身（カードやリスト）を配置する。設定項目のグルーピングに使用する。
pub fn group(theme: &Theme, title: impl Into<SharedString>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::SM))
        .child(
            div()
                .px(px(spacing::LG))
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(title.into()),
        )
        .child(body)
}

/// 画面やパネルの背景面。文字色もロールから決定する。
pub fn surface(theme: &Theme) -> Div {
    div()
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
    fn ghost_background_opacity_and_theme_apply_only_to_ghost() {
        let (dark, light) = (&Theme::DARK, &Theme::LIGHT);
        let ghost = || base_button(light, "g".into(), Variant::Ghost);
        let (rest, hover, active) = ghost().colors();
        assert_eq!(hover, light.control_on_background_hover);
        let (r, h, a) = ghost().background_opacity(0.5).colors();
        assert_eq!(r & 0xff, 0, "通常時は透明のまま");
        assert_eq!((rest & 0xff, r), (0, rest));
        assert_eq!(h, scale_alpha(hover, 0.5));
        assert_eq!(a, scale_alpha(active, 0.5));
        assert_eq!(
            ghost().background_opacity(7.0).colors().1,
            hover,
            "範囲外は 100% にクランプ"
        );
        // 色の差し替えと不透明度は、適用順序に依存しない。
        let swapped = (
            dark.control_on_background_hover,
            dark.control_on_background_active,
        );
        let one = ghost()
            .ghost_background(dark)
            .background_opacity(0.5)
            .colors();
        let two = ghost()
            .background_opacity(0.5)
            .ghost_background(dark)
            .colors();
        assert_eq!(one, two);
        assert_eq!(
            (one.1, one.2),
            (scale_alpha(swapped.0, 0.5), scale_alpha(swapped.1, 0.5))
        );
        // Ghost 以外では効果なし。
        let plain = base_button(light, "d".into(), Variant::Default)
            .ghost_background(dark)
            .background_opacity(0.5);
        assert_eq!(plain.colors().1, light.control_hover);
    }

    #[test]
    fn scale_alpha_scales_only_the_alpha() {
        assert_eq!(scale_alpha(0x11223380, 0.5), 0x11223340);
        assert_eq!(scale_alpha(0x112233ff, 1.0), 0x112233ff);
        assert_eq!(scale_alpha(0x112233ff, 0.0), 0x11223300);
    }

    #[test]
    fn spinner_turns_once_per_half_second() {
        let t0 = Instant::now();
        let a = spinner_turns(t0);
        let quarter = spinner_turns(t0 + Duration::from_millis(125));
        let full = spinner_turns(t0 + Duration::from_millis(500));
        assert!(((quarter - a).rem_euclid(1.0) - 0.25).abs() < 1e-3);
        assert!((full - a).abs() < 1e-3);
    }

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
    fn fade_blends_alpha_for_transparent_rest() {
        // Ghost: 静止色は透明。ホバーへの遷移途中の背景は半透明であるべき。
        let mid = mix(0x80808000, 0x808080ff, 0.5);
        assert_eq!(mid & 0xff, 0x80, "{mid:08x}");
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
        // 辺に近い行ほど深く削れ、半径に達すると削れない。
        assert!(corner_inset(10.0, 0.5) > corner_inset(10.0, 5.0));
        assert_eq!(corner_inset(10.0, 10.0), 0.0);
        assert_eq!(corner_inset(0.0, 0.5), 0.0);
        // 円の方程式に従う（半径 10、辺から 5 の行は 10 - sqrt(75)）。
        assert!((corner_inset(10.0, 5.0) - (10.0 - 75f32.sqrt())).abs() < 1e-5);
        // 過大な角丸（カプセル形）は高さの半分にクランプされる。
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
