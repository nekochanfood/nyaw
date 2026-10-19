//! 透過する面を色付きの背景と重ね、照明と個別に切り替えて比較する画面。
use design::{Effects, Theme, components, font_size, radius, spacing};
use gpui::{Context, Render, Window, div, prelude::*, px, rgba};

pub struct EffectsTest;

impl Render for EffectsTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let effects = Effects::current(cx);
        let controls = components::card(&theme)
            .p(px(spacing::LG))
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
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
            );
        let preview = div()
            .id("effects-preview")
            .debug_selector(|| "effects-preview".into())
            .relative()
            .flex_1()
            .min_h(px(160.))
            .rounded(px(radius::LG))
            .bg(rgba(theme.accent_solid))
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w_1_2()
                    .bg(rgba(theme.danger_solid))
                    .rounded_r(px(radius::LG)),
            )
            .child(
                components::card(&theme)
                    .id("effects-sample")
                    .debug_selector(|| "effects-sample".into())
                    .absolute()
                    .inset(px(spacing::LG))
                    .rounded(px(radius::LG))
                    .p(px(spacing::LG))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(i18n::t(cx, "effects.sample"))
                    .child(components::button(
                        &theme,
                        "effects-sample-button",
                        i18n::t(cx, "effects.control"),
                        components::Variant::Default,
                    )),
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
    use gpui::{Modifiers, TestAppContext};

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
    }
}
