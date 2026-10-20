//! 透過する面を色付きの背景と重ね、照明と個別に切り替えて比較する画面。
use design::{Effects, Mode, Theme, components, font_size, radius, spacing};
use gpui::{Context, Render, Window, div, prelude::*, px, rgba};

pub struct EffectsTest;

impl Render for EffectsTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let effects = Effects::current(cx);
        let edge_strength = Effects::edge_strength(cx);
        let controls = components::card(&theme)
            .p(px(spacing::LG))
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(i18n::t(cx, "effects.light_theme"))
                    .child(
                        components::switch(
                            &theme,
                            "effects-light-theme",
                            cx.try_global::<Mode>().copied().unwrap_or_default() == Mode::Light,
                        )
                        .on_click(|_, _, cx| {
                            let mode = if cx.try_global::<Mode>().copied().unwrap_or_default()
                                == Mode::Light
                            {
                                Mode::Dark
                            } else {
                                Mode::Light
                            };
                            cx.set_global(mode);
                            cx.refresh_windows();
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(i18n::t(cx, "effects.translucent"))
                    .child(
                        components::switch(&theme, "effects-translucent", effects.translucent)
                            .on_click(|_, _, cx| {
                                let mut effects = Effects::current(cx);
                                effects.translucent = !effects.translucent;
                                effects.set(cx);
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(i18n::t(cx, "effects.pointer_light"))
                    .child(
                        components::switch(&theme, "effects-pointer-light", effects.pointer_light)
                            .on_click(|_, _, cx| {
                                let mut effects = Effects::current(cx);
                                effects.pointer_light = !effects.pointer_light;
                                effects.set(cx);
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(i18n::t(cx, "effects.edge_strength"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(spacing::LG))
                            .child(
                                components::slider(
                                    &theme,
                                    "effects-edge-strength",
                                    edge_strength / Effects::MAX_EDGE_STRENGTH,
                                    |v, _, cx| {
                                        Effects::set_edge_strength(
                                            (v * Effects::MAX_EDGE_STRENGTH * 100.).round() / 100.,
                                            cx,
                                        )
                                    },
                                )
                                .length(300.),
                            )
                            .child(div().w(px(72.)).child(format!("{edge_strength:.2}×"))),
                    ),
            );
        // 背面の模様は、操作色の調整から独立させる。
        let color_backdrop = design::scale::dark::BLUE_9;
        let preview = div()
            .id("effects-preview")
            .debug_selector(|| "effects-preview".into())
            .flex()
            .gap(px(spacing::SM))
            .flex_1()
            .min_h(px(260.))
            .children(
                [
                    ("dark", 0x101820ff),
                    ("light", 0xffffffff),
                    ("color", color_backdrop),
                ]
                .into_iter()
                .map(|(name, color)| {
                    let prefix = format!("effects-{name}");
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .p(px(spacing::XS))
                        .bg(rgba(color))
                        .child(if name == "color" {
                            div().absolute().inset_0().flex().children((0..8).map(|i| {
                                div().h_full().flex_1().bg(rgba(if i % 2 == 0 {
                                    color_backdrop
                                } else {
                                    theme.danger_solid
                                }))
                            }))
                        } else {
                            div()
                        })
                        .child(
                            div()
                                .relative()
                                .size_full()
                                .bg(rgba(theme.background))
                                .rounded(px(radius::MD))
                                .p(px(spacing::SM))
                                .flex()
                                .flex_col()
                                .gap(px(spacing::SM))
                                .child(i18n::t(cx, &format!("effects.{name}_backdrop")))
                                .child(
                                    components::card(&theme)
                                        .rounded(px(radius::SM))
                                        .p(px(spacing::SM))
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .justify_between()
                                        .relative()
                                        .when(name == "color", |card| {
                                            card.child(
                                                div()
                                                    .absolute()
                                                    .left_0()
                                                    .right_0()
                                                    .top(px(32.))
                                                    .bottom(px(64.))
                                                    .rounded(px(radius::SM))
                                                    .overflow_hidden()
                                                    .flex()
                                                    .children((0..8).map(|i| {
                                                        div().h_full().flex_1().bg(rgba(
                                                            if i % 2 == 0 {
                                                                color_backdrop
                                                            } else {
                                                                theme.danger_solid
                                                            },
                                                        ))
                                                    })),
                                            )
                                        })
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(spacing::SM))
                                                .child(
                                                    design::icon(&design::icons::HEART)
                                                        .size(px(font_size::MD)),
                                                )
                                                .child(i18n::t(cx, "effects.card")),
                                        )
                                        .child(
                                            components::button(
                                                &theme,
                                                format!("{prefix}-button"),
                                                i18n::t(cx, "effects.button"),
                                                components::Variant::Default,
                                            )
                                            .px(px(spacing::SM)),
                                        )
                                        .child(
                                            components::button(
                                                &theme,
                                                format!("{prefix}-primary"),
                                                "Primary",
                                                components::Variant::Primary,
                                            )
                                            .px(px(spacing::SM)),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    components::button(
                                                        &theme,
                                                        format!("{prefix}-ghost"),
                                                        "Ghost",
                                                        components::Variant::Ghost,
                                                    )
                                                    .px(px(spacing::SM)),
                                                )
                                                .child(components::switch(
                                                    &theme,
                                                    format!("{prefix}-switch"),
                                                    true,
                                                )),
                                        ),
                                ),
                        )
                }),
            );
        components::panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .child(
                div()
                    .text_size(px(font_size::MD))
                    .child(i18n::t(cx, "effects.instructions")),
            )
            .child(controls)
            .child(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, MouseButton, TestAppContext};

    #[gpui::test]
    fn edge_slider_updates_the_material_without_changing_effect_flags(cx: &mut TestAppContext) {
        cx.update(|cx| Effects::ENABLED.set(cx));
        let (_, cx) = cx.add_window_view(|_, _| EffectsTest);
        cx.simulate_resize(gpui::size(px(800.), px(600.)));
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| Effects::edge_strength(cx)), 2.);
        let handle = cx
            .debug_bounds("effects-edge-strength-handle")
            .unwrap()
            .center();
        let track = cx.debug_bounds("effects-edge-strength").unwrap();
        cx.simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
        for (x, expected) in [(track.right(), 3.), (track.left(), 0.)] {
            cx.simulate_mouse_move(
                gpui::point(x, handle.y),
                MouseButton::Left,
                Modifiers::none(),
            );
            cx.run_until_parked();
            cx.update(|window, cx| {
                Effects::sync(window, cx);
                assert_eq!(Effects::edge_strength(cx), expected);
                assert_eq!(Effects::current(cx), Effects::ENABLED);
            });
        }
        cx.simulate_mouse_up(handle, MouseButton::Left, Modifiers::none());
    }

    #[gpui::test]
    fn switches_change_design_flags_independently(cx: &mut TestAppContext) {
        cx.update(|cx| Effects::ENABLED.set(cx));
        let (_, cx) = cx.add_window_view(|_, _| EffectsTest);
        cx.simulate_resize(gpui::size(px(800.), px(600.)));
        cx.run_until_parked();
        for (selector, expected) in [
            (
                "effects-translucent",
                Effects {
                    translucent: false,
                    pointer_light: true,
                },
            ),
            ("effects-pointer-light", Effects::default()),
            (
                "effects-translucent",
                Effects {
                    translucent: true,
                    pointer_light: false,
                },
            ),
        ] {
            let at = cx.debug_bounds(selector).expect("トグルが見える").center();
            cx.simulate_click(at, Modifiers::none());
            cx.run_until_parked();
            assert_eq!(cx.update(|_, cx| Effects::current(cx)), expected);
        }
        let effects = cx.update(|_, cx| Effects::current(cx));
        let at = cx.debug_bounds("effects-light-theme").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(Theme::current(cx), Theme::LIGHT.translucent());
            assert_eq!(
                Effects::current(cx),
                effects,
                "テーマと効果を独立に比較できる"
            );
        });
    }
}
