//! Layer 5: 画面。セットアップウィザード（初回起動と、デバッグからの再試行）。
//! 見た目と遷移は `design::components` のウィザード（`wizard_*` + `navigation`）。

use design::components::{self, Wizard};
use design::{Logo, Theme, icons, radius, spacing};
use gpui::{App, ClickEvent, Context, IntoElement, Render, Window, div, prelude::*, px, rgba};
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Setting};
use ui::{PanelControl, PanelSpec};

/// 1:1 のパネル。
const SIZE: Vec2 = Vec2::new(0.8, 0.8);
const PX: (u32, u32) = (800, 800);
/// ようこそ → 設定（仮置き）→ 完了。
pub const STEPS: usize = 3;
const FULL_LOGO_HEIGHT: f32 = 72.0;
/// 目の前（頭から 1m 先）。
const IN_FRONT: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

/// 初回起動で、目の前に出すウィザードの起動仕様。
pub fn spec() -> PanelSpec {
    // 開いた元の無いパネルなので、頭の前に出す。
    PanelSpec::root(IN_FRONT, |id, _, _| SetupWizard::new(id))
        .in_view(IN_FRONT)
        .sized(SIZE, PX)
        .titled("setup.title")
}

/// デバッグから強制的にやり直す: パネル `from` の隣に開く。
pub fn spawn_spec(from: PanelId) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, _| SetupWizard::new(id))
        .sized(SIZE, PX)
        .titled("setup.title")
}

pub struct SetupWizard {
    panel: PanelId,
    pub wizard: Wizard,
}

impl SetupWizard {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            wizard: Wizard::new(STEPS),
        }
    }
}

/// 終えたことを設定に残し、このパネルを閉じる。
fn finish(panel: PanelId, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(Setting::SetupDone(true));
    control
        .panels
        .entry(panel)
        .or_default()
        .commands
        .push(PanelCommand::Close);
}

impl Render for SetupWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let panel = self.panel;
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
                        components::wizard_next(&theme, "setup-next", i18n::t(cx, "setup.start"))
                            .on_click(go(true));
                    return components::wizard_welcome(
                        &theme,
                        "setup",
                        design::logo(Logo::Full).height(px(FULL_LOGO_HEIGHT)),
                        i18n::t(cx, "setup.welcome_title"),
                        i18n::t(cx, "setup.welcome_description"),
                        [next],
                    )
                    .into_any_element();
                }
                if step == STEPS - 1 {
                    let done = components::wizard_next(
                        &theme,
                        "setup-finish",
                        i18n::t(cx, "setup.finish"),
                    )
                    .on_click(move |_, _, cx| finish(panel, cx));
                    return components::wizard_welcome(
                        &theme,
                        "setup",
                        components::wizard_icon(&theme, &icons::CHECK_CIRCLE),
                        i18n::t(cx, "setup.done_title"),
                        i18n::t(cx, "setup.done_description"),
                        [done],
                    )
                    .into_any_element();
                }
                // 仮置き: セットアップ用の画面ができたらここに入れる。
                let placeholder = div()
                    .id("setup-placeholder")
                    .debug_selector(|| "setup-placeholder".into())
                    .p(px(spacing::XL))
                    .rounded(px(radius::LG))
                    .bg(rgba(theme.control))
                    .child(i18n::t(cx, "setup.placeholder"));
                components::wizard_step(
                    &theme,
                    "setup",
                    components::wizard_icon(&theme, &icons::WRENCH),
                    i18n::t(cx, "setup.step_title"),
                    i18n::t(cx, "setup.step_description"),
                    placeholder,
                    Some(
                        components::wizard_back(&theme, "setup-back", i18n::t(cx, "setup.back"))
                            .on_click(go(false)),
                    ),
                    components::wizard_next(&theme, "setup-next", i18n::t(cx, "setup.next"))
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
            components::navigation(&theme, "setup-nav", &stack, page, swipe_back)
                .background(theme.card),
        )
    }
}
