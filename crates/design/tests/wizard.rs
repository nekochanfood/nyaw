//! ウィザードの画面（`components::wizard_welcome` / `wizard_step`）の配置と、「次へ」「戻る」のボタンの幅。

use design::components::{self, Variant, WIZARD_MARGIN};
use design::{Theme, spacing};
use gpui::{
    Context, Entity, IntoElement, Pixels, Render, TestAppContext, VisualTestContext, Window, div,
    prelude::*, px,
};

const SIZE: f32 = 600.0;

struct View {
    welcome: bool,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let hero = div().w(px(200.0)).h(px(100.0));
        let next = components::wizard_next(&theme, "next", "Next");
        let page = if self.welcome {
            let second = components::button(&theme, "second", "Second", Variant::Default);
            components::wizard_welcome(&theme, "w", hero, "Title", "", [next, second])
        } else {
            let back = components::wizard_back(&theme, "back", "Back");
            let body = div().h(px(100.0));
            components::wizard_step(&theme, "w", hero, "Title", "About", body, Some(back), next)
        };
        div().size_full().child(page)
    }
}

fn setup(cx: &mut TestAppContext, welcome: bool) -> (Entity<View>, &mut VisualTestContext) {
    setup_sized(cx, welcome, SIZE)
}

/// 幅 `width`・高さ [`SIZE`] の窓で開く。
fn setup_sized(
    cx: &mut TestAppContext,
    welcome: bool,
    width: f32,
) -> (Entity<View>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, _| View { welcome });
    cx.simulate_resize(gpui::size(px(width), px(SIZE)));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    (view, cx)
}

fn near(a: Pixels, b: f32) -> bool {
    (a - px(b)).abs() < px(1.5)
}

/// welcome: 上からアイコン（やロゴ）・タイトル・ボタンが 1 つのまとまりで、画面の上下左右中央。ボタンは縦に積む（主の操作が上）。
#[gpui::test]
fn welcome_centers_the_content_with_its_actions_stacked_under_it(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx, true);
    let [hero, title, next, second] =
        ["w-hero", "w-title", "next", "second"].map(|s| cx.debug_bounds(s).unwrap());
    for part in [hero, title, next, second] {
        assert!(near(part.center().x, SIZE / 2.0), "{part:?}");
    }
    let middle = (hero.top() + second.bottom()) / 2.0;
    assert!(near(middle, SIZE / 2.0), "{middle:?}");
    // ボタンは内容の下に、少し広めに空けて置く（読むものと押すものを分ける）。主の操作が上、その下にもう 1 つ。
    assert!(near(
        next.top() - title.bottom(),
        components::WIZARD_ACTIONS_GAP
    ));
    assert!(near(second.top() - next.bottom(), spacing::SM));
}

/// step: 左下に「戻る」、右下に「次へ」。文字が短くても、押しやすい幅（高さの 2.5 倍）を保つ。
#[gpui::test]
fn step_keeps_roomy_buttons_in_the_footer_corners(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx, false);
    let [back, next] = ["back", "next"].map(|s| cx.debug_bounds(s).unwrap());
    assert!(near(back.left(), WIZARD_MARGIN));
    assert!(near(next.right(), SIZE - WIZARD_MARGIN));
    assert!(near(next.bottom(), SIZE - WIZARD_MARGIN) && near(back.bottom(), next.bottom().into()));
    for b in [back, next] {
        assert_eq!(b.size.height, px(components::BUTTON_HEIGHT));
        assert!(b.size.width >= px(components::BUTTON_HEIGHT * 2.5), "{b:?}");
    }
    // 内容（アイコン・タイトル・説明・中身）は、フッターの上の領域の上下中央。
    let hero = cx.debug_bounds("w-hero").unwrap();
    assert!(hero.top() > px(WIZARD_MARGIN) && hero.bottom() < next.top());
}

/// 横長のパネルでは、内容は決まった幅（`WIZARD_CONTENT`）の列になって左右中央に来る。ボタンは下の両端のまま。
#[gpui::test]
fn step_centers_a_column_of_content_on_a_wide_panel(cx: &mut TestAppContext) {
    const WIDE: f32 = 1200.0;
    let (_, cx) = setup_sized(cx, false, WIDE);
    let [hero, back, next] = ["w-hero", "back", "next"].map(|s| cx.debug_bounds(s).unwrap());
    let column = (WIDE - components::WIZARD_CONTENT) / 2.0;
    assert!(near(hero.left(), column), "{hero:?}");
    assert!(near(hero.right(), WIDE - column), "{hero:?}");
    assert!(near(back.left(), WIZARD_MARGIN) && near(next.right(), WIDE - WIZARD_MARGIN));
}
