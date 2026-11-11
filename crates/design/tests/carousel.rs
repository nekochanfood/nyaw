//! カルーセル（`components::carousel`。並べ方は弧 `arc`）: 押したまま左右に動かして選ぶ（マウス・指のスライド）、端はつながる、
//! 隣を押すと中央に来る、画面の積み重ねのスワイプバックと取り合わない。
//! テストでは `Motion` を置かないので、動きは即座に終わる。

use design::Theme;
use design::components::{arc, carousel, navigation};
use gpui::{
    Context, Entity, IntoElement, Modifiers, MouseButton, Pixels, Point, Render, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, Window, div, point,
    prelude::*, px,
};

const LEN: usize = 5;
/// 中央の項目の一辺。項目 1 つ分の動きは、その 0.96 倍（96px）。
const SIDE: f32 = 100.0;
const W: f32 = 600.0;

struct View {
    selected: usize,
    /// `on_select` で知らされた項目。
    picks: Vec<usize>,
    /// 外側の画面の積み重ね（スワイプバックで 1 枚に戻る）。
    stack: Vec<u8>,
    /// 外側へ届いたスクロール（縦の動きは渡す）。
    scrolled: usize,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (this, back) = (cx.entity(), cx.entity());
        let selected = self.selected;
        let page = move |_: &u8, _: &mut Window, _: &mut gpui::App| {
            let this = this.clone();
            let item = |_, side: f32, _: &mut Window, _: &mut gpui::App| {
                div().size(px(side)).into_any_element()
            };
            div()
                .size_full()
                .child(carousel(
                    "c",
                    LEN,
                    selected,
                    arc(SIDE),
                    item,
                    move |i, _, cx| {
                        this.update(cx, |v, cx| {
                            v.selected = i;
                            v.picks.push(i);
                            cx.notify()
                        })
                    },
                ))
                .into_any_element()
        };
        div()
            .size_full()
            .on_scroll_wheel(cx.listener(|v, _, _, _| v.scrolled += 1))
            .child(navigation(
                &Theme::DARK,
                "nav",
                &self.stack,
                page,
                move |_, cx| {
                    back.update(cx, |v, cx| {
                        v.stack.pop();
                        cx.notify()
                    })
                },
            ))
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<View>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, _| View {
        selected: 0,
        picks: Vec::new(),
        stack: vec![0, 1],
        scrolled: 0,
    });
    cx.simulate_resize(gpui::size(px(W), px(300.0)));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    (view, cx)
}

fn item(cx: &mut VisualTestContext, i: usize) -> gpui::Bounds<Pixels> {
    let selector: &'static str = ["c-item-0", "c-item-1", "c-item-2", "c-item-3", "c-item-4"][i];
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not shown"))
}

/// 中央の項目を押したまま、横へ `steps` 回 `step` px ずつ動かして離す（右が正）。
fn drag(cx: &mut VisualTestContext, steps: usize, step: f32) {
    let from = cx.debug_bounds("c").unwrap().center();
    let from = point(from.x, px(SIDE / 2.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let mut at = from;
    for _ in 0..steps {
        at.x += px(step);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
}

fn slide(at: Point<Pixels>, dx: f32, dy: f32, phase: TouchPhase) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
    }
}

fn state(view: &Entity<View>, cx: &mut VisualTestContext) -> (usize, Vec<usize>) {
    view.read_with(cx, |v, _| (v.selected, v.picks.clone()))
}

#[gpui::test]
fn the_selected_item_sits_at_the_top_of_the_arc(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx);
    let (center, right, left) = (item(cx, 0), item(cx, 1), item(cx, LEN - 1));
    assert_eq!(
        center.center().x,
        px(W / 2.0),
        "centered whatever the width"
    );
    assert_eq!(center.size.width, px(SIDE));
    // 隣は左右対称に、弧に沿って下がり、小さくなる。端はつながっているので、先頭の左には最後の項目。
    assert!(right.center().x > center.right() && left.center().x < center.left());
    assert_eq!(right.center().y, left.center().y);
    assert!(right.center().y > center.center().y);
    assert!(right.size.width < center.size.width);
    // 隣より先は見えない。
    assert!(cx.debug_bounds("c-item-2").is_none());
}

#[gpui::test]
fn dragging_past_half_an_item_selects_the_next_and_a_short_drag_snaps_back(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(cx);
    // 左へ 60px（項目 1 つ分は 96px）: 次の項目に収まる。
    drag(cx, 12, -5.0);
    assert_eq!(state(&view, cx), (1, vec![1]));
    assert_eq!(item(cx, 1).center().x, px(W / 2.0));
    // 30px では戻る（知らせも来ない）。
    drag(cx, 6, -5.0);
    assert_eq!(state(&view, cx), (1, vec![1]));
    assert_eq!(item(cx, 1).center().x, px(W / 2.0));
    // 右へ動かすと前の項目。
    drag(cx, 12, 5.0);
    assert_eq!(state(&view, cx), (0, vec![1, 0]));
}

#[gpui::test]
fn the_items_follow_the_pointer_while_held(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let from = point(px(W / 2.0), px(SIDE / 2.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let mut at = from;
    for _ in 0..8 {
        at.x -= px(6.0);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    // 48px = 項目半分: 中央にいた項目は左へ、次の項目は中央へ、同じだけ寄っている。
    let (leaving, coming) = (item(cx, 0).center().x, item(cx, 1).center().x);
    assert!(leaving < px(W / 2.0) && coming > px(W / 2.0));
    let gap = (px(W) - leaving - coming).abs();
    assert!(gap < px(1.0), "{leaving:?} {coming:?}");
    assert_eq!(state(&view, cx), (0, vec![]), "not until released");
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn a_quick_flick_selects_the_next_even_if_short(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // 45px は項目半分（48px）に届かないが、払っているので次へ進む。
    drag(cx, 3, -15.0);
    assert_eq!(state(&view, cx), (1, vec![1]));
}

#[gpui::test]
fn the_ends_are_joined(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // 先頭から右へ動かすと最後の項目、そこから左へ動かすと先頭に戻る。
    drag(cx, 12, 5.0);
    assert_eq!(state(&view, cx).0, LEN - 1);
    assert_eq!(item(cx, LEN - 1).center().x, px(W / 2.0));
    drag(cx, 12, -5.0);
    assert_eq!(state(&view, cx).0, 0);
}

#[gpui::test]
fn pressing_a_neighbor_brings_it_to_the_middle(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let right = item(cx, 1).center();
    cx.simulate_click(right, Modifiers::none());
    assert_eq!(state(&view, cx), (1, vec![1]));
    // 中央の項目を押しても何も起きない。
    let center = item(cx, 1).center();
    cx.simulate_click(center, Modifiers::none());
    assert_eq!(state(&view, cx), (1, vec![1]));
    // 外から選ぶ項目を替える（左右のボタンなど）と、その項目が中央に来る。
    view.update(cx, |v, cx| {
        v.selected = 4;
        cx.notify()
    });
    cx.run_until_parked();
    assert_eq!(item(cx, 4).center().x, px(W / 2.0));
}

#[gpui::test]
fn touch_slide_selects_and_swallows_the_coasting(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // スライドの宛先は押した所に固定（UI 層と同じ）。
    let at = point(px(W / 2.0), px(SIDE / 2.0));
    cx.simulate_event(slide(at, -14.0, 0.0, TouchPhase::Started));
    for _ in 0..9 {
        cx.simulate_event(slide(at, -5.0, 0.0, TouchPhase::Moved));
    }
    assert_eq!(state(&view, cx), (0, vec![]), "not until released");
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(state(&view, cx), (1, vec![1]));
    // 離した後の慣性（終わりの段階のスクロール）では進まない。
    for _ in 0..5 {
        cx.simulate_event(slide(at, -40.0, 0.0, TouchPhase::Ended));
    }
    assert_eq!(state(&view, cx), (1, vec![1]));
    // 次のスライドからはまた動く。
    cx.simulate_event(slide(at, 14.0, 0.0, TouchPhase::Started));
    cx.simulate_event(slide(at, 60.0, 0.0, TouchPhase::Moved));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(state(&view, cx), (0, vec![1, 0]));
    assert_eq!(
        view.read_with(cx, |v, _| v.scrolled),
        0,
        "横の動きは外へ渡さない"
    );
}

#[gpui::test]
fn vertical_slides_pass_through(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let at = point(px(W / 2.0), px(SIDE / 2.0));
    cx.simulate_event(slide(at, 2.0, -14.0, TouchPhase::Started));
    cx.simulate_event(slide(at, 0.0, -80.0, TouchPhase::Moved));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(state(&view, cx), (0, vec![]));
    assert!(
        view.read_with(cx, |v, _| v.scrolled) > 0,
        "縦の動きは外へ渡す"
    );
}

#[gpui::test]
fn dragging_the_carousel_does_not_swipe_the_page_back(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // 画面を 1 枚積んだ上のカルーセルを、右から左へ大きく動かす（スワイプバックと同じ向き）。
    drag(cx, 10, -20.0);
    assert_eq!(
        view.read_with(cx, |v, _| v.stack.len()),
        2,
        "stays on the page"
    );
    assert_eq!(state(&view, cx).0, 3, "the carousel took the drag");
    let at = point(px(W / 2.0), px(SIDE / 2.0));
    cx.simulate_event(slide(at, -14.0, 0.0, TouchPhase::Started));
    cx.simulate_event(slide(at, -400.0, 0.0, TouchPhase::Moved));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(view.read_with(cx, |v, _| v.stack.len()), 2);
    // カルーセルの外で動かせば、今までどおり戻る。
    let outside = point(px(W - 20.0), px(250.0));
    cx.simulate_mouse_down(outside, MouseButton::Left, Modifiers::none());
    let mut at = outside;
    for _ in 0..20 {
        at.x -= px(20.0);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(view.read_with(cx, |v, _| v.stack.len()), 1);
}
