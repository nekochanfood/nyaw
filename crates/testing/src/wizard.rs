use super::*;
use design::icons;

/// ウィザードの試験画面: ようこそ → 手順 1 → 手順 2 → 完了。`wizard-next` / `wizard-back` / `wizard-restart`。
pub struct WizardTest {
    pub wizard: components::Wizard,
}

impl WizardTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            wizard: components::Wizard::new(4),
        }
    }
}

impl Render for WizardTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let w = self.wizard;
        let go = |forward: bool| {
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                if forward {
                    this.wizard.next()
                } else {
                    this.wizard.back()
                };
                cx.notify();
            })
        };
        if w.is_first() {
            let next = components::wizard_next(&theme, "wizard-next", i18n::t(cx, "wizard.start"))
                .on_click(go(true));
            return components::wizard_welcome(
                &theme,
                "wizard",
                &icons::STAR,
                i18n::t(cx, "wizard.welcome_title"),
                i18n::t(cx, "wizard.welcome_description"),
                [next],
            )
            .into_any_element();
        }
        if w.is_last() {
            let again = cx.listener(|this, _: &ClickEvent, _, cx| {
                this.wizard = components::Wizard::new(4);
                cx.notify();
            });
            let b =
                components::wizard_next(&theme, "wizard-restart", i18n::t(cx, "wizard.restart"))
                    .on_click(again);
            return components::wizard_welcome(
                &theme,
                "wizard",
                &icons::CHECK_CIRCLE,
                i18n::t(cx, "wizard.done_title"),
                i18n::t(cx, "wizard.done_description"),
                [b],
            )
            .into_any_element();
        }
        let form = div()
            .id("wizard-form")
            .debug_selector(|| "wizard-form".into())
            .p(px(spacing::XL))
            .rounded(px(radius::LG))
            .bg(rgba(theme.control))
            .child(i18n::t(cx, "wizard.form"));
        components::wizard_step(
            &theme,
            "wizard",
            &icons::STAR,
            i18n::tf(cx, "wizard.step_title", &[("n", &w.step().to_string())]),
            i18n::t(cx, "wizard.step_description"),
            form,
            Some(
                components::wizard_back(&theme, "wizard-back", i18n::t(cx, "wizard.back"))
                    .on_click(go(false)),
            ),
            components::wizard_next(&theme, "wizard-next", i18n::t(cx, "wizard.next"))
                .on_click(go(true)),
        )
        .into_any_element()
    }
}
