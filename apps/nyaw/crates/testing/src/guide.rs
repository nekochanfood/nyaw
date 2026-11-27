//! コントローラガイドの検証画面（[`GuideTest`]）: 使用中のコントローラの判定と、画像の上の円・テール・説明を確かめる。

use super::*;
use ::guide::{Callout, Control, Controller, Model};

/// 画面のサイズ（m）と解像度（px）。左に機種の一覧、右にガイドと割り当ての一覧を置く。
pub const GUIDE_SIZE: Vec2 = Vec2::new(1.4, 0.9);
pub const GUIDE_PX: (u32, u32) = (1400, 900);
/// ガイドの画像の一辺（px）。説明の列は左右にこの半分ずつ。
const IMAGE_PX: f32 = 470.0;

/// 機種の一覧の `debug_selector`（[`Model::ALL`] の順）。
const MODEL_IDS: [&str; Model::ALL.len()] = [
    "guide-model-quest-plus",
    "guide-model-quest-2",
    "guide-model-quest-pro",
    "guide-model-rift-s",
    "guide-model-rift-cv1",
    "guide-model-index",
    "guide-model-vive",
    "guide-model-pico-4",
    "guide-model-pico-4-ultra",
    "guide-model-focus-3",
    "guide-model-frame",
];

/// 表示する機種。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    /// その手で使用中のコントローラ（無ければ Quest Touch Plus）。
    InUse,
    Model(Model),
}

/// コントローラガイドの検証画面（`guide-test`）。
///
/// - `guide-detected-left` / `-right`: 手ごとに判定した機種（ランタイムが報告したレンダーモデル名から）。
/// - `guide-in-use` と `guide-model-*`: 表示する機種。`guide-hand-left` / `-right`: 表示する手。
/// - `guide-bind-<操作>`: 割り当ての一覧。カーソルを合わせると、その操作の円と札（`guide-<操作>`）を強調する。
/// - `guide-all-parts`: 割り当てではなく、画像のすべてのボタン（`guide-part-<id>`）を表示する。
///
/// 使用中の機種が変わったら映すため、毎フレーム再描画する（[`ui::PanelState::live`]）。
pub struct GuideTest {
    pub hand: Hand,
    pub shown: Shown,
    pub all_parts: bool,
    /// カーソルを合わせている説明（`callouts` の添字）。
    pub hovered: Option<usize>,
}

impl GuideTest {
    pub fn spec(from: PanelId) -> PanelSpec {
        PanelSpec::spawn(from, |id, _, cx| GuideTest::new(id, cx))
            .sized(GUIDE_SIZE, GUIDE_PX)
            .titled("testing.open.guide")
    }

    pub fn new(panel: PanelId, cx: &mut App) -> Self {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .live = true;
        Self {
            hand: Hand::Right,
            shown: Shown::InUse,
            all_parts: false,
            hovered: None,
        }
    }

    /// 表示中のコントローラ。
    pub fn controller(&self, cx: &App) -> Controller {
        match self.shown {
            Shown::InUse => detected(cx, self.hand).unwrap_or(Controller::of(Model::QuestPlus)),
            Shown::Model(m) => Controller::of(m),
        }
    }
}

/// `hand` で判定したコントローラ。
fn detected(cx: &App, hand: Hand) -> Option<Controller> {
    cx.try_global::<PanelControl>()?
        .controllers
        .iter()
        .find(|c| c.hand == hand)?
        .controller
}

fn hand_name(cx: &App, hand: Hand) -> SharedString {
    match hand {
        Hand::Left => i18n::t(cx, "testing.attach.left_hand"),
        Hand::Right => i18n::t(cx, "testing.attach.right_hand"),
    }
}

/// 割り当ての一覧の 1 行: `debug_selector`、表示、強調する説明の添字（割り当てが無ければ `None`）。
type Row = (SharedString, SharedString, Option<usize>);

impl Render for GuideTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let controller = self.controller(cx);
        let model = controller.model;
        let hand = self.hand;

        // 説明: 割り当て（操作ごと）か、画像のすべてのボタン。
        let (callouts, rows): (Vec<Callout>, Vec<Row>) = if self.all_parts {
            let callouts: Vec<Callout> = ::guide::sheet(model, hand)
                .iter()
                .map(|p| Callout {
                    id: format!("guide-part-{}", p.id).into(),
                    parts: vec![p.id.as_str()],
                    title: ::guide::part_name(cx, &p.id),
                    detail: None,
                })
                .collect();
            let rows = callouts
                .iter()
                .enumerate()
                .map(|(i, c)| (c.id.clone(), c.title.clone(), Some(i)))
                .collect();
            (callouts, rows)
        } else {
            let bound = ::guide::bindings(cx, controller, hand);
            let rows = Control::ALL
                .into_iter()
                .map(|control| {
                    let i = bound.iter().position(|(c, _)| *c == control);
                    let parts = i.and_then(|i| bound[i].1.detail.clone());
                    (
                        format!("guide-bind-{}", ::guide::control_id(control)).into(),
                        format!(
                            "{} — {}",
                            ::guide::control_name(cx, control),
                            parts.unwrap_or_else(|| i18n::t(cx, "guide_test.unbound"))
                        )
                        .into(),
                        i,
                    )
                })
                .collect();
            (bound.into_iter().map(|(_, c)| c).collect(), rows)
        };
        let hovered = self.hovered.filter(|i| *i < callouts.len());

        let detected_row = div().flex().flex_col().gap(px(spacing::XS)).children(
            [
                (Hand::Left, "guide-detected-left"),
                (Hand::Right, "guide-detected-right"),
            ]
            .map(|(h, id)| {
                let found = detected(cx, h);
                let label = match found {
                    Some(c) => i18n::tf(
                        cx,
                        "guide_test.hand_model",
                        &[("hand", &hand_name(cx, h)), ("model", &c.model.name())],
                    ),
                    None => i18n::tf(cx, "guide_test.hand_none", &[("hand", &hand_name(cx, h))]),
                };
                indicator(&theme, id, label, found.is_some())
            }),
        );
        let hand_row = row().children(
            [
                (Hand::Left, "guide-hand-left"),
                (Hand::Right, "guide-hand-right"),
            ]
            .map(|(h, id)| {
                themed_button(&theme, id, hand_name(cx, h), hand == h).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.hand = h;
                        this.hovered = None;
                        cx.notify();
                    },
                ))
            }),
        );
        let pick = |shown: Shown| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                this.shown = shown;
                this.hovered = None;
                cx.notify();
            })
        };
        let models = div()
            .id("guide-models")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(spacing::XS))
            .child(
                themed_button(
                    &theme,
                    "guide-in-use",
                    i18n::t(cx, "guide_test.in_use"),
                    self.shown == Shown::InUse,
                )
                .on_click(pick(Shown::InUse)),
            )
            .children(Model::ALL.into_iter().zip(MODEL_IDS).map(|(m, id)| {
                themed_button(&theme, id, m.name(), self.shown == Shown::Model(m))
                    .on_click(pick(Shown::Model(m)))
            }));
        let sidebar = section(&theme, i18n::t(cx, "guide_test.detected"))
            .w(px(300.0))
            .flex_none()
            .child(detected_row)
            .child(labeled(&theme, i18n::t(cx, "guide_test.hand"), hand_row))
            .child(
                labeled(&theme, i18n::t(cx, "guide_test.controllers"), models)
                    .flex_1()
                    .min_h_0(),
            );

        let note = (self.shown == Shown::InUse && detected(cx, hand).is_none()).then(|| {
            div()
                .debug_selector(|| "guide-note".into())
                .text_color(rgba(theme.neutral_text_low))
                .child(i18n::tf(
                    cx,
                    "guide_test.not_connected",
                    &[("hand", &hand_name(cx, hand)), ("model", &model.name())],
                ))
        });
        let list = div()
            .flex()
            .flex_wrap()
            .gap(px(spacing::XS))
            .children(rows.into_iter().map(|(id, label, index)| {
                let selected = index.is_some() && index == hovered;
                div()
                    .id(id.clone())
                    .debug_selector(move || id.to_string())
                    .h(px(40.0))
                    .px(px(spacing::MD))
                    .flex()
                    .items_center()
                    .rounded(px(radius::LG))
                    .bg(rgba(if selected {
                        theme.control_active
                    } else {
                        theme.control
                    }))
                    .when(index.is_none(), |d| {
                        d.text_color(rgba(theme.neutral_text_low))
                    })
                    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                        if *over {
                            this.hovered = index;
                        } else if this.hovered == index {
                            this.hovered = None;
                        }
                        cx.notify();
                    }))
                    .child(label)
            }));
        let main = section(&theme, model.name())
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .debug_selector(|| "guide-view".into())
                    .flex()
                    .justify_center()
                    .child(
                        ::guide::controller(model, hand)
                            .size(px(IMAGE_PX))
                            .callouts(callouts)
                            .focus(hovered),
                    ),
            )
            .children(note)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::t(cx, "guide_test.hint")),
                    )
                    .child(
                        themed_button(
                            &theme,
                            "guide-all-parts",
                            i18n::t(cx, "guide_test.all_parts"),
                            self.all_parts,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.all_parts = !this.all_parts;
                            this.hovered = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(labeled(&theme, i18n::t(cx, "guide_test.bindings"), list));

        components::panel(&theme)
            .flex()
            .gap(px(spacing::MD))
            .child(sidebar)
            .child(main)
    }
}
