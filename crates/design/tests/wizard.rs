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
    let (view, cx) = cx.add_window_view(|_, _| View { welcome });
    cx.simulate_resize(gpui::size(px(SIZE), px(SIZE)));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    (view, cx)
}

fn near(a: Pixels, b: f32) -> bool {
    (a - px(b)).abs() < px(1.5)
}

/// welcome: アイコン（やロゴ）とタイトルのかたまりが画面の上下左右中央。ボタンは下の帯に、左右中央寄せで縦に積む。
#[gpui::test]
fn welcome_centers_the_content_and_stacks_the_actions_at_the_bottom(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx, true);
    let [hero, title, next, second] =
        ["w-hero", "w-title", "next", "second"].map(|s| cx.debug_bounds(s).unwrap());
    assert!(near(hero.center().x, SIZE / 2.0) && near(title.center().x, SIZE / 2.0));
    let middle = (hero.top() + title.bottom()) / 2.0;
    assert!(near(middle, SIZE / 2.0), "{middle:?}");
    // 主の操作が上、その下にもう 1 つ。下の縁からは画面の余白だけ空く。
    assert!(title.bottom() < next.top());
    assert!(near(next.center().x, SIZE / 2.0) && near(second.center().x, SIZE / 2.0));
    assert!(near(second.top() - next.bottom(), spacing::SM));
    assert!(near(second.bottom(), SIZE - WIZARD_MARGIN));
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
