//! カタログ: design のすべて（色の段・役割・定数・文字・アイコン・部品・動き）を 1 画面で確認するデバッグ用のビュー。
//! `cargo run -p design --example catalog` で単独のウィンドウに出る。画面側（VR のパネル）に載せてもよい。
//! 右上のスイッチでライト / ダークを切り替える。

use std::time::Instant;

use gpui::{
    Context, Div, FontWeight, IntoElement, ParentElement, Render, ScrollHandle, SharedString,
    Styled, Window, div, prelude::*, px, rgba,
};

use crate::{
    Mode, Theme, Weight,
    animation::{Animation, Motion},
    components::{self, Variant},
    duration_ms, font_size, icon, icons, letter_spacing, radius, scale, spacing,
};

pub struct Catalog {
    mode: Mode,
    page: ScrollHandle,
    scroll: ScrollHandle,
    slider: f32,
    toggle: bool,
    clicks: usize,
    /// 動きの見本（再生した後のもの）。
    motions: Vec<(Motion, Animation)>,
    last: Option<Instant>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            mode: Mode::Dark,
            page: ScrollHandle::new(),
            scroll: ScrollHandle::new(),
            slider: 0.5,
            toggle: false,
            clicks: 0,
            motions: Vec::new(),
            last: None,
        }
    }
}

const MOTIONS: [Motion; 4] = [Motion::Enter, Motion::Exit, Motion::Return, Motion::Move];

fn hex(c: u32) -> String {
    format!("#{c:08x}")
}

/// 見出し + 中身。
/// 見出しと中身を 1 枚のカードに入れる。
fn section(t: &Theme, title: &str, body: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::SM))
        .p(px(spacing::XL))
        .rounded(px(radius::LG))
        .bg(rgba(t.card))
        .child(
            div()
                .text_size(px(font_size::LG))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(t.accent_text))
                .child(title.to_string()),
        )
        .child(body)
}

fn caption(t: &Theme, s: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(font_size::SM))
        .text_color(rgba(t.neutral_text_low))
        .child(s.into())
}

/// 色見本 1 枚（名前と 16 進つき）。
fn swatch(t: &Theme, name: &str, c: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .w(px(88.0))
        .child(
            div()
                .h(px(40.0))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(rgba(t.neutral_border))
                .bg(rgba(c)),
        )
        .child(caption(t, name.to_string()))
        .child(caption(t, hex(c)))
}

fn wrap() -> Div {
    div().flex().flex_wrap().gap(px(spacing::MD))
}

/// `scale::<mode>` の 3 スケール × 12 段。
macro_rules! scales {
    ($m:ident) => {{
        use scale::$m::*;
        [
            (
                "slate",
                [
                    SLATE_1, SLATE_2, SLATE_3, SLATE_4, SLATE_5, SLATE_6, SLATE_7, SLATE_8,
                    SLATE_9, SLATE_10, SLATE_11, SLATE_12,
                ],
            ),
            (
                "blue",
                [
                    BLUE_1, BLUE_2, BLUE_3, BLUE_4, BLUE_5, BLUE_6, BLUE_7, BLUE_8, BLUE_9,
                    BLUE_10, BLUE_11, BLUE_12,
                ],
            ),
            (
                "red",
                [
                    RED_1, RED_2, RED_3, RED_4, RED_5, RED_6, RED_7, RED_8, RED_9, RED_10, RED_11,
                    RED_12,
                ],
            ),
        ]
    }};
}

/// `Theme` の役割を名前つきで並べる。
macro_rules! roles {
    ($t:expr, $($f:ident),+ $(,)?) => { [$((stringify!($f), $t.$f)),+] };
}

fn theme_roles(t: &Theme) -> [(&'static str, u32); 51] {
    roles!(
        t,
        accent_base,
        accent_bg_subtle,
        accent_bg,
        accent_bg_hover,
        accent_bg_active,
        accent_border_subtle,
        accent_border,
        accent_border_hover,
        accent_solid,
        accent_solid_hover,
        accent_text_low,
        accent_text,
        danger_base,
        danger_bg_subtle,
        danger_bg,
        danger_bg_hover,
        danger_bg_active,
        danger_border_subtle,
        danger_border,
        danger_border_hover,
        danger_solid,
        danger_solid_hover,
        danger_text_low,
        danger_text,
        neutral_base,
        neutral_bg_subtle,
        neutral_bg,
        neutral_bg_hover,
        neutral_bg_active,
        neutral_border_subtle,
        neutral_border,
        neutral_border_hover,
        neutral_solid,
        neutral_solid_hover,
        neutral_text_low,
        neutral_text,
        accent_solid_active,
        danger_solid_active,
        focus_ring,
        on_accent,
        on_danger,
        background,
        card,
        control,
        control_hover,
        control_active,
        control_on_background,
        control_on_background_hover,
        control_on_background_active,
        groove,
        track,
    )
}

fn scale_section(t: &Theme) -> Div {
    let mut body = div().flex().flex_col().gap(px(spacing::MD));
    for (mode, rows) in [("light", scales!(light)), ("dark", scales!(dark))] {
        for (name, steps) in rows {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::XS))
                    .child(caption(t, format!("{mode}::{name} 1..12")))
                    .child(
                        wrap().gap(px(spacing::XS)).children(
                            steps
                                .iter()
                                .enumerate()
                                .map(|(i, c)| swatch(t, &format!("{}", i + 1), *c)),
                        ),
                    ),
            );
        }
    }
    body = body.child(wrap().child(swatch(t, "WHITE", scale::WHITE)).child(swatch(
        t,
        "BLACK",
        scale::BLACK,
    )));
    section(t, "Constants / scale", body)
}

fn value_row(t: &Theme, name: &str, shown: Div) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(spacing::MD))
        .child(div().w(px(220.0)).child(caption(t, name.to_string())))
        .child(shown)
}

fn constants_section(t: &Theme) -> Div {
    let bar = |w: f32, h: f32, r: f32| {
        div()
            .w(px(w))
            .h(px(h))
            .rounded(px(r))
            .bg(rgba(t.accent_solid))
    };
    let mut body = div().flex().flex_col().gap(px(spacing::SM));
    for (n, v) in [
        ("NONE", radius::NONE),
        ("SM", radius::SM),
        ("MD", radius::MD),
        ("LG", radius::LG),
        ("FULL", radius::FULL),
    ] {
        body = body.child(value_row(
            t,
            &format!("radius::{n} = {v}"),
            bar(64.0, 32.0, v),
        ));
    }
    for (n, v) in [
        ("XS", spacing::XS),
        ("SM", spacing::SM),
        ("MD", spacing::MD),
        ("LG", spacing::LG),
        ("XL", spacing::XL),
    ] {
        body = body.child(value_row(
            t,
            &format!("spacing::{n} = {v}"),
            bar(v, 16.0, 0.0),
        ));
    }
    for (n, v) in [
        ("FAST", duration_ms::FAST),
        ("QUICK", duration_ms::QUICK),
        ("NORMAL", duration_ms::NORMAL),
        ("SLOW", duration_ms::SLOW),
    ] {
        body = body.child(value_row(
            t,
            &format!("duration_ms::{n} = {v}"),
            bar(v as f32, 8.0, 0.0),
        ));
    }
    section(t, "Constants / radius, spacing, duration", body)
}

fn text_section(t: &Theme) -> Div {
    let mut body = div().flex().flex_col().gap(px(spacing::SM));
    for (n, v) in [
        ("SM", font_size::SM),
        ("MD", font_size::MD),
        ("LG", font_size::LG),
    ] {
        body = body.child(value_row(
            t,
            &format!("font_size::{n} = {v}"),
            div()
                .text_size(px(v))
                .child("The quick brown fox / 吾輩は猫である 0123456789"),
        ));
    }
    for (n, v) in [
        ("TIGHT", letter_spacing::TIGHT),
        ("NORMAL", letter_spacing::NORMAL),
        ("WIDE", letter_spacing::WIDE),
    ] {
        body = body.child(value_row(
            t,
            &format!("letter_spacing::{n} = {v}"),
            div().child("Spacing sample / 字間の見本"),
        ));
    }
    for (n, w) in [
        ("NORMAL", FontWeight::NORMAL),
        ("MEDIUM", FontWeight::MEDIUM),
        ("BOLD", FontWeight::BOLD),
    ] {
        body = body.child(value_row(
            t,
            &format!("weight {n}"),
            div().font_weight(w).child("Weight sample / 太さの見本"),
        ));
    }
    section(t, "Typography", body)
}

fn icon_section(t: &Theme) -> Div {
    let mut body = div().flex().flex_col().gap(px(spacing::MD));
    for ic in icons::ALL {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(spacing::XS))
                .child(caption(t, ic.name.to_string()))
                .child(wrap().children(Weight::ALL.iter().map(|w| {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            icon(ic)
                                .weight(*w)
                                .size(px(32.0))
                                .color(rgba(t.neutral_text)),
                        )
                        .child(caption(t, format!("{w:?}")))
                })))
                .child(
                    wrap()
                        .child(
                            icon(ic)
                                .weight(Weight::Fill)
                                .size(px(32.0))
                                .color(rgba(t.accent_solid)),
                        )
                        .child(
                            icon(ic)
                                .weight(Weight::Fill)
                                .size(px(32.0))
                                .color(rgba(t.danger_solid)),
                        )
                        .child(
                            icon(ic)
                                .size(px(32.0))
                                .color(rgba(t.neutral_text))
                                .rotate(90.0),
                        )
                        .child(icon(ic).size(px(32.0)).color(rgba(t.neutral_text)).flip_x())
                        .child(
                            icon(ic)
                                .size(px(32.0))
                                .color(rgba(t.neutral_text))
                                .opacity(0.4),
                        ),
                ),
        );
    }
    section(t, "Icons（ビルドに入っているものだけ）", body)
}

/// 縦横にあふれる領域に `scrollbars` を重ねる。
fn scroll_demo(t: &Theme, handle: &ScrollHandle) -> Div {
    div()
        .relative()
        .w(px(320.0))
        .h(px(120.0))
        .rounded(px(radius::MD))
        .bg(rgba(t.control))
        .child(
            // ページの中の入れ子: ポインタが触れている内側だけが動く。
            components::claim_scroll(
                components::free_scroll(div().id("cat-scroll"))
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(handle),
                handle,
            )
            .child(
                components::elastic("cat-pull-demo", handle)
                    .flex_none()
                    .w(px(900.0))
                    .h(px(500.0))
                    .p(px(spacing::SM))
                    .child("スクロールして確認 / scroll me"),
            ),
        )
        .child(components::scrollbars(t, "cat-bars", handle).pulled_by("cat-pull-demo"))
}

impl Catalog {
    fn components_section(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let (a, b, c, d) = (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let on = self.toggle;
        let body = div()
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(
                wrap()
                    .child(components::button(
                        t,
                        "cat-default",
                        "Default",
                        Variant::Default,
                    ))
                    .child(
                        components::button(t, "cat-primary", "Primary", Variant::Primary).on_click(
                            move |_, _, cx| {
                                a.update(cx, |s, cx| {
                                    s.clicks += 1;
                                    cx.notify()
                                })
                            },
                        ),
                    )
                    .child(components::button(
                        t,
                        "cat-danger",
                        "Danger",
                        Variant::Danger,
                    ))
                    .child(
                        components::button(t, "cat-selected", "Selected", Variant::Default)
                            .selected(true),
                    ),
            )
            .child(
                // 背景に直接置くボタンは、背景の上で見る。
                wrap()
                    .p(px(spacing::MD))
                    .rounded(px(radius::LG))
                    .bg(rgba(t.background))
                    .child(components::button(
                        t,
                        "cat-on-background",
                        "On background",
                        Variant::OnBackground,
                    )),
            )
            .child(caption(
                t,
                format!(
                    "Primary を押した回数: {}（ホバー・押下は色が補間される）",
                    self.clicks
                ),
            ))
            .child(
                wrap()
                    .items_center()
                    .child(
                        components::switch(t, "cat-switch", on).on_click(move |_, _, cx| {
                            b.update(cx, |s, cx| {
                                s.toggle = !s.toggle;
                                cx.notify()
                            })
                        }),
                    )
                    .child(caption(
                        t,
                        format!("switch: {}", if on { "on" } else { "off" }),
                    )),
            )
            .child(
                wrap()
                    .items_center()
                    .child(components::slider(
                        t,
                        "cat-slider",
                        self.slider,
                        move |v, _, cx| {
                            c.update(cx, |s, cx| {
                                s.slider = v;
                                cx.notify()
                            })
                        },
                    ))
                    .child(caption(t, format!("slider: {:.2}", self.slider)))
                    .child(
                        components::slider(t, "cat-slider-v", self.slider, move |v, _, cx| {
                            d.update(cx, |s, cx| {
                                s.slider = v;
                                cx.notify()
                            })
                        })
                        .vertical()
                        .length(80.0),
                    ),
            )
            .child(caption(t, "scrollbars（縦・横）"))
            .child(scroll_demo(t, &self.scroll));
        section(t, "Components", body)
    }

    fn motion_section(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 再生中なら進めて、終わるまで描き直す。
        let now = Instant::now();
        let dt = self
            .last
            .map_or(0.0, |l| now.duration_since(l).as_secs_f32());
        self.last = Some(now);
        let mut playing = false;
        for (_, a) in &mut self.motions {
            a.advance(dt);
            playing |= !a.is_done();
        }
        if playing {
            window.request_animation_frame();
        }
        let entity = cx.entity();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .child(wrap().child(
                components::button(t, "cat-play", "Play", Variant::Primary).on_click(
                    move |_, _, cx| {
                        entity.update(cx, |s, cx| {
                            s.motions = MOTIONS
                                .iter()
                                .map(|m| (*m, Animation::new(0.0, 1.0, *m)))
                                .collect();
                            s.last = None;
                            cx.notify()
                        })
                    },
                ),
            ));
        for m in MOTIONS {
            let v = self
                .motions
                .iter()
                .find(|(k, _)| *k == m)
                .map_or(0.0, |(_, a)| a.value());
            body = body.child(value_row(
                t,
                &format!("{m:?}（{} ms）", m.duration_ms()),
                div()
                    .w(px(240.0))
                    .h(px(16.0))
                    .rounded(px(radius::FULL))
                    .bg(rgba(t.track))
                    .child(
                        div()
                            .h_full()
                            .w(px(240.0 * v))
                            .rounded(px(radius::FULL))
                            .bg(rgba(t.accent_solid)),
                    ),
            ));
        }
        section(t, "Animation", body)
    }
}

impl Render for Catalog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::of(self.mode);
        let effects = crate::Effects::current(cx);
        let effect_controls = wrap()
            .items_center()
            .child(caption(&t, "Translucent"))
            .child(
                components::switch(&t, "cat-translucent", effects.translucent).on_click(
                    |_, _, cx| {
                        let mut effects = crate::Effects::current(cx);
                        effects.translucent = !effects.translucent;
                        effects.set(cx);
                    },
                ),
            )
            .child(caption(&t, "Pointer light"))
            .child(
                components::switch(&t, "cat-pointer-light", effects.pointer_light).on_click(
                    |_, _, cx| {
                        let mut effects = crate::Effects::current(cx);
                        effects.pointer_light = !effects.pointer_light;
                        effects.set(cx);
                    },
                ),
            );
        let this = cx.entity();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(font_size::LG))
                    .font_weight(FontWeight::BOLD)
                    .child("design catalog"),
            )
            .child(
                wrap()
                    .items_center()
                    .child(caption(
                        &t,
                        format!("{:?}（スイッチで切り替え）", self.mode),
                    ))
                    .child(
                        components::switch(&t, "cat-mode", self.mode == Mode::Light).on_click(
                            move |_, _, cx| {
                                this.update(cx, |s, cx| {
                                    s.mode = s.mode.toggled();
                                    cx.notify()
                                })
                            },
                        ),
                    ),
            );
        let components = self.components_section(&t, cx);
        let motion = self.motion_section(&t, window, cx);
        let roles = section(
            &t,
            "Utilities / Theme の役割",
            wrap().children(theme_roles(&t).iter().map(|(n, c)| swatch(&t, n, *c))),
        );
        let page = self.page.clone();
        components::surface(&t)
            .id("catalog-surface")
            .size_full()
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(
                        div()
                            .id("catalog")
                            .size_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .track_scroll(&page)
                            .child(
                                components::elastic("cat-pull-page", &page)
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .gap(px(spacing::XL))
                                    .p(px(spacing::XL))
                                    .child(header)
                                    .child(effect_controls)
                                    .child(
                                        components::card(&t)
                                            .p(px(spacing::XL))
                                            .child("Surface / ポインタを動かすと面を照らします"),
                                    )
                                    .child(roles)
                                    .child(scale_section(&t))
                                    .child(constants_section(&t))
                                    .child(text_section(&t))
                                    .child(icon_section(&t))
                                    .child(components)
                                    .child(motion),
                            ),
                    )
                    .child(
                        components::scrollbars(&t, "cat-page-bars", &page)
                            .pulled_by("cat-pull-page")
                            .corner_radius(radius::XL),
                    ),
            )
            .on_mouse_move(|event, window, cx| {
                let source = cx
                    .try_global::<components::TooltipSource>()
                    .and_then(|s| s.0)
                    .unwrap_or(0);
                crate::SurfacePointers::set(window, usize::from(source), Some(event.position), cx);
            })
            .on_hover(|hovered, window, cx| {
                if !hovered {
                    let source = cx
                        .try_global::<components::TooltipSource>()
                        .and_then(|s| s.0)
                        .unwrap_or(0);
                    crate::SurfacePointers::set(window, usize::from(source), None, cx);
                }
            })
    }
}
