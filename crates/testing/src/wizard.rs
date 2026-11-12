use super::*;
use design::icons;

const STEPS: usize = 4;

/// ウィザードの試験画面: ようこそ → 手順 1 → 手順 2 → 完了。`wizard-next` / `wizard-back` / `wizard-restart`。
/// 画面は `navigation` の積み重ね（右から入る。押したまま右から左へ動かすとスワイプバック）。
pub struct WizardTest {
    pub wizard: components::Wizard,
}

impl WizardTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            wizard: components::Wizard::new(STEPS),
        }
    }
}

/// `from` の隣にウィザードを開く（大きさはオーバーレイの既定。4:3）。
pub fn spawn(from: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(from, |id, _, _| WizardTest::new(id)).titled("testing.open.wizard"),
        )
    }
}

impl Render for WizardTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let stack: Vec<usize> = (0..=self.wizard.step()).collect();
        let page = {
            let this = this.clone();
            move |&step: &usize, _: &mut Window, cx: &mut App| {
                let go = |forward: bool| {
                    let this = this.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        this.update(cx, |v, cx| {
                            if forward {
                                v.wizard.next()
                            } else {
                                v.wizard.back()
                            };
                            cx.notify();
                        })
                    }
                };
                if step == 0 {
                    let next =
                        components::wizard_next(&theme, "wizard-next", i18n::t(cx, "wizard.start"))
                            .on_click(go(true));
                    return components::wizard_welcome(
                        &theme,
                        "wizard",
                        components::wizard_icon(&theme, &icons::STAR),
                        i18n::t(cx, "wizard.welcome_title"),
                        i18n::t(cx, "wizard.welcome_description"),
                        [next],
                    )
                    .into_any_element();
                }
                if step == STEPS - 1 {
                    let this = this.clone();
                    let again = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        this.update(cx, |v, cx| {
                            v.wizard = components::Wizard::new(STEPS);
                            cx.notify();
                        })
                    };
                    let b = components::wizard_next(
                        &theme,
                        "wizard-restart",
                        i18n::t(cx, "wizard.restart"),
                    )
                    .on_click(again);
                    return components::wizard_welcome(
                        &theme,
                        "wizard",
                        components::wizard_icon(&theme, &icons::CHECK_CIRCLE),
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
                    components::wizard_icon(&theme, &icons::STAR),
                    i18n::tf(cx, "wizard.step_title", &[("n", &step.to_string())]),
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
        };
        let swipe_back = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |v, cx| {
                v.wizard.back();
                cx.notify();
            })
        };
        components::wizard_panel(&theme).child(
            components::navigation(&theme, "wizard-nav", &stack, page, swipe_back)
                .background(theme.card),
        )
    }
}
