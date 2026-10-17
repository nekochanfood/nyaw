use super::*;

pub struct KeyboardTest {
    panel: PanelId,
    pub keyboard: gpui::Entity<ui::keyboard::KeyboardView>,
    pub text: String,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

impl KeyboardTest {
    pub fn spec() -> PanelSpec {
        PanelSpec::new(Pose::IDENTITY, |id, _, cx| Self::new(id, cx))
            .in_view(Pose::new(Vec3::new(0., 0.23, -0.85), Quat::IDENTITY))
            .sized(Vec2::new(0.96, 0.66), (1600, 1100))
            .titled("keyboard.title")
    }

    pub fn new(panel: PanelId, cx: &mut Context<Self>) -> Self {
        let keyboard = cx.new(|cx| ui::keyboard::KeyboardView::new(panel, cx));
        Self {
            panel,
            keyboard,
            text: String::new(),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for KeyboardTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let os_visible = cx
            .try_global::<ui::keyboard::InputRouting>()
            .and_then(|routing| routing.active)
            .is_some_and(|s| {
                s.target == self.panel && s.mode == ui::keyboard::KeyboardMode::Persistent
            });
        let focus = self.focus.clone();
        let field = div()
            .id("keyboard-test-field")
            .debug_selector(|| "keyboard-test-field".into())
            .track_focus(&self.focus)
            .min_h(px(108.))
            .flex_shrink_0()
            .p(px(spacing::MD))
            .rounded(px(radius::LG))
            .bg(rgba(theme.control))
            .focus(|s| s.bg(rgba(theme.accent_bg_active)))
            .on_click(cx.listener(move |this, _, window, cx| {
                ui::keyboard::focus_input(this.panel, &focus, window, cx)
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "backspace" => {
                        this.text.pop();
                    }
                    "enter" => this.text.push('\n'),
                    "tab" => this.text.push('\t'),
                    _ => {
                        if let Some(text) = &event.keystroke.key_char {
                            this.text.push_str(text);
                        }
                    }
                }
                cx.notify();
            }))
            .child(if self.text.is_empty() {
                i18n::t(cx, "keyboard.type_hint")
            } else {
                self.text.clone().into()
            });
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(
                button(
                    &theme,
                    "keyboard-os-toggle",
                    i18n::t(cx, "keyboard.os_button"),
                    os_visible,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    ui::keyboard::toggle_os_keyboard(this.panel, cx);
                    cx.notify();
                })),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        components::free_scroll(
                            div()
                                .id("keyboard-test-scroll")
                                .debug_selector(|| "keyboard-test-scroll".into())
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(spacing::LG))
                                .child(field)
                                .child(self.keyboard.clone()),
                        ),
                    )
                    .child(components::scrollbars(
                        &theme,
                        "keyboard-test-bars",
                        &self.scroll,
                    )),
            )
    }
}
