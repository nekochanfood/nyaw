//! 親子関係を試す画面（[`DialogTest`]）と、子として開くダイアログ（[`Dialog`]）。

use super::*;

/// 文言のキーが `title` / `description` のダイアログを、ダイアログの幅（`components::DIALOG_WIDTH`）と
/// 文に合った高さ（`components::dialog_height`）で `spec` に与える。
fn dialog_sized(
    spec: PanelSpec,
    title: &str,
    description: &str,
    window: &Window,
    cx: &App,
) -> PanelSpec {
    let w = components::DIALOG_WIDTH;
    let h = components::dialog_height(window, &i18n::t(cx, title), &i18n::t(cx, description));
    spec.sized(Vec2::new(w as f32 / 1000.0, h as f32 / 1000.0), (w, h))
}

/// 親子関係を試す画面（`dialog-test`）。ダイアログ（[`Dialog`]）を子として開く 2 種類のボタンがある。
/// - `open-dialog`: 外側（この画面など）を押すと閉じる（既定）。
/// - `open-modal-dialog`: 外側を押しても閉じない（`PanelSpec::without_dismiss`）。`dialog-ok` / `dialog-cancel` で答えるまで残る。
///
/// 開いている間はこの画面が暗くなって押せず（掴んで動かすと子も付いてくる）、ダイアログを掴むと親も付いてくる（独立しない）。
#[derive(Default)]
pub struct DialogTest {
    panel: PanelId,
    /// ダイアログが答えた回数と、最後の答え。
    pub answered: usize,
    pub answer: Option<&'static str>,
}

impl DialogTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }
}

impl Render for DialogTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        let me = cx.entity().downgrade();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                row()
                    .child({
                        let me = me.clone();
                        button(&theme, "open-dialog", i18n::t(cx, "dialog.open"), false).on_click(
                            move |_, window, cx| {
                                let me = me.clone();
                                let spec = dialog_sized(
                                    PanelSpec::child(panel, true, move |id, _, _| {
                                        Dialog::new(
                                            id,
                                            me,
                                            &design::icons::INFO,
                                            "dialog.dismiss_title",
                                            "dialog.dismiss_body",
                                        )
                                    })
                                    // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
                                    .without_nav_bar(),
                                    "dialog.dismiss_title",
                                    "dialog.dismiss_body",
                                    window,
                                    cx,
                                );
                                cx.default_global::<PanelControl>().spawns.push(spec)
                            },
                        )
                    })
                    .child(
                        button(
                            &theme,
                            "open-modal-dialog",
                            i18n::t(cx, "dialog.open_modal"),
                            false,
                        )
                        .on_click(move |_, window, cx| {
                            let me = me.clone();
                            let spec = dialog_sized(
                                PanelSpec::child(panel, true, move |id, _, _| {
                                    Dialog::new(
                                        id,
                                        me,
                                        &design::icons::WARNING,
                                        "dialog.required_title",
                                        // わざと長い文。3 行を超える分は文の側だけがスクロールし、ボタンは残る。
                                        "dialog.required_body",
                                    )
                                })
                                .without_dismiss()
                                .without_nav_bar(),
                                "dialog.required_title",
                                "dialog.required_body",
                                window,
                                cx,
                            );
                            cx.default_global::<PanelControl>().spawns.push(spec)
                        }),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "dialog-answer".into())
                    .child(i18n::tf(
                        cx,
                        "dialog.answered",
                        &[
                            ("count", &self.answered),
                            (
                                "answer",
                                &self
                                    .answer
                                    .map_or("-".into(), |a| i18n::t(cx, answer_key(a))),
                            ),
                        ],
                    )),
            )
    }
}

/// ダイアログの答え（`OK` / `Cancel`）を出すときの文言のキー。
fn answer_key(answer: &str) -> &'static str {
    match answer {
        "OK" => "common.ok",
        _ => "common.cancel",
    }
}

/// [`DialogTest`] が開く、OK / Cancel だけのダイアログ（`components::dialog`）。押した答えを親へ渡して自分を閉じる。
/// `title` は種類の見分け用。`title` / `description` は文言のキー（描くときに今の言語で引く）。
pub struct Dialog {
    panel: PanelId,
    parent: WeakEntity<DialogTest>,
    glyph: &'static design::Icon,
    title: &'static str,
    description: &'static str,
}

impl Dialog {
    pub fn new(
        panel: PanelId,
        parent: WeakEntity<DialogTest>,
        glyph: &'static design::Icon,
        title: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            panel,
            parent,
            glyph,
            title,
            description,
        }
    }
}

impl Render for Dialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        // 答えの値（親に渡す）は言語によらず、見出しだけを訳す。
        let answer = |id: &'static str, value: &'static str, label: SharedString, variant| {
            let parent = self.parent.clone();
            let close = command(self.panel, PanelCommand::Close);
            components::button(&theme, id, label, variant).on_click(move |e, w, cx| {
                parent
                    .update(cx, |t, cx| {
                        t.answered += 1;
                        t.answer = Some(value);
                        cx.notify();
                    })
                    .ok();
                close(e, w, cx);
            })
        };
        components::dialog(
            &theme,
            "dialog",
            self.glyph,
            i18n::t(cx, self.title),
            i18n::t(cx, self.description),
            [
                answer(
                    "dialog-cancel",
                    "Cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                ),
                answer(
                    "dialog-ok",
                    "OK",
                    i18n::t(cx, "common.ok"),
                    Variant::Primary,
                ),
            ],
        )
    }
}
