//! 画面の積み重ね（push 遷移・スワイプバック）を試す画面（[`NavTest`]）。

use super::*;

/// 押すと `view` の積み重ねに `page` を積む・下ろす（`None`）。
fn stack_op<V: 'static, P: 'static + Copy>(
    view: &gpui::Entity<V>,
    stack: fn(&mut V) -> &mut Vec<P>,
    page: Option<P>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| {
        view.update(cx, |v, cx| {
            match page {
                Some(p) => stack(v).push(p),
                None => drop(stack(v).pop()),
            }
            cx.notify()
        })
    }
}

/// 画面の積み重ね（[`components::navigation`]）を試す画面 [`NavTest`] の画面 1 枚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavPage {
    /// 一番下。項目（`nav-item-0..`）を押すと [`NavPage::Detail`] を積む。
    Root,
    /// 項目の詳しい画面。`nav-deeper` で [`NavPage::Deep`] を積む。`nav-tap`（押した回数を数える）と `nav-slider` がある。
    Detail(usize),
    /// 一番上。
    Deep(usize),
}

/// 画面の積み重ね（push 遷移とスワイプバック）を試す画面（`nav-test`）。設定画面の中身と同じ部品。
/// - 項目を押すと右から次の画面が入り、`nav-back` か、押したまま右から左へ動かす（スワイプバック）と戻る。
/// - スワイプして離した所のボタン（`nav-tap`）は押されない。スライダー（`nav-slider`）のハンドルを左へ動かしても戻らない。
/// - パネルを掴んでいる手のポインタは無視する（掴みながらトリガーで動かしても戻らず、もう一方の手ではスワイプできる）。
#[derive(Default)]
pub struct NavTest {
    pub stack: Vec<NavPage>,
    /// `nav-tap` を押した回数。
    pub taps: usize,
    /// `nav-slider` の値。
    pub value: f32,
    /// スワイプバックで戻った回数。
    pub swiped_back: usize,
}

impl NavTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            stack: vec![NavPage::Root],
            taps: 0,
            value: 0.5,
            swiped_back: 0,
        }
    }
}

impl Render for NavTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let stack: fn(&mut NavTest) -> &mut Vec<NavPage> = |v| &mut v.stack;
        let page = move |p: &NavPage, _: &mut Window, cx: &mut App| {
            let theme = Theme::current(cx);
            let body = div().size_full().flex().flex_col().gap(px(spacing::MD));
            match *p {
                NavPage::Root => body.child(components::list(
                    &theme,
                    (0..3).map(|n| {
                        components::list_item(
                            &theme,
                            ["nav-item-0", "nav-item-1", "nav-item-2"][n],
                            &design::icons::GEAR,
                            i18n::tf(cx, "nav.item", &[("n", &(n + 1))]),
                        )
                        .on_click(stack_op(
                            &this,
                            stack,
                            Some(NavPage::Detail(n)),
                        ))
                    }),
                )),
                NavPage::Detail(n) => {
                    let (tap, set) = (this.clone(), this.clone());
                    body.child(components::list(
                        &theme,
                        [
                            components::list_item(
                                &theme,
                                "nav-deeper",
                                &design::icons::INFO,
                                i18n::tf(cx, "nav.deeper", &[("n", &(n + 1))]),
                            )
                            .on_click(stack_op(
                                &this,
                                stack,
                                Some(NavPage::Deep(n)),
                            )),
                            components::list_item(
                                &theme,
                                "nav-tap",
                                &design::icons::BELL,
                                i18n::t(cx, "nav.tap"),
                            )
                            .on_click(move |_, _, cx| {
                                tap.update(cx, |v, cx| {
                                    v.taps += 1;
                                    cx.notify()
                                })
                            }),
                        ],
                    ))
                    .child(
                        components::slider(&theme, "nav-slider", this.read(cx).value, {
                            move |v, _, cx| {
                                set.update(cx, |s, cx| {
                                    s.value = v;
                                    cx.notify()
                                })
                            }
                        })
                        .length(400.0),
                    )
                }
                NavPage::Deep(n) => body.child(i18n::tf(cx, "nav.deep", &[("n", &(n + 1))])),
            }
            .into_any_element()
        };
        let depth = self.stack.len() - 1;
        let title = match self.stack[depth] {
            NavPage::Root => i18n::t(cx, "nav.title"),
            NavPage::Detail(n) => i18n::tf(cx, "nav.item", &[("n", &(n + 1))]),
            NavPage::Deep(n) => i18n::tf(cx, "nav.deeper", &[("n", &(n + 1))]),
        };
        let back_button = (depth > 0).then(|| {
            components::icon_button(
                &theme,
                "nav-back",
                &design::icons::CARET_LEFT,
                i18n::t(cx, "common.back"),
                Variant::OnBackground,
            )
            .on_click(stack_op(&cx.entity(), stack, None))
            .into_any_element()
        });
        let back = cx.entity();
        components::panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(::settings::heading(title, back_button))
            .child(div().flex_1().child(components::navigation(
                &theme,
                "nav",
                &self.stack,
                page,
                move |_, cx| {
                    back.update(cx, |v, cx| {
                        v.stack.pop();
                        v.swiped_back += 1;
                        cx.notify()
                    })
                },
            )))
    }
}
