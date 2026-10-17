//! 画面の積み重ね（`components::navigation`）: 積む・押したまま右から左へ動かして戻る（マウス・指のスライド）。
//! テストでは `Motion` を置かないので、積み下ろしの動きは即座に終わる。

use design::{
    Theme,
    components::{navigation, slider},
};
use gpui::{
    App, Context, Entity, IntoElement, Modifiers, MouseButton, MouseExitEvent, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, Window,
    div, point, prelude::*, px,
};

#[derive(Default)]
struct View {
    stack: Vec<u8>,
    taps: usize,
    backs: usize,
    value: f32,
}

/// 画面 1 枚: 左上に `push`（次の画面を積む）、その下に幅いっぱいの `tap`（押した回数）、`slider`。
fn page(this: &Entity<View>, n: u8, cx: &mut App) -> gpui::AnyElement {
    let (push, tap, set) = (this.clone(), this.clone(), this.clone());
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .id("push")
                .debug_selector(|| "push".into())
                .w(px(100.0))
                .h(px(40.0))
                .on_click(move |_, _, cx| {
                    push.update(cx, |v, cx| {
                        v.stack.push(n + 1);
                        cx.notify()
                    })
                }),
        )
        .child(
            div()
                .id("tap")
                .debug_selector(|| "tap".into())
                .w_full()
                .h(px(40.0))
                .on_click(move |_, _, cx| {
                    tap.update(cx, |v, cx| {
                        v.taps += 1;
                        cx.notify()
                    })
                }),
        )
        .child(slider(
            &Theme::DARK,
            "slider",
            this.read(cx).value,
            move |v, _, cx| {
                set.update(cx, |s, cx| {
                    s.value = v;
                    cx.notify()
                })
            },
        ))
        .into_any_element()
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (this, back) = (cx.entity(), cx.entity());
        div().size_full().child(navigation(
            &Theme::DARK,
            "nav",
            &self.stack,
            move |&n, _, cx| page(&this, n, cx),
            move |_, cx| {
                back.update(cx, |v, cx| {
                    v.stack.pop();
                    v.backs += 1;
                    cx.notify()
                })
            },
        ))
    }
}

const W: f32 = 400.0;

fn setup(cx: &mut TestAppContext, stack: Vec<u8>) -> (Entity<View>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, _| View {
        stack,
        value: 0.5,
        ..View::default()
    });
    cx.simulate_resize(gpui::size(px(W), px(300.0)));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    (view, cx)
}

fn depth(view: &Entity<View>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |v, _| v.stack.len() - 1)
}

/// `selector` の行の右端近く。
fn right_of(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
    point(px(W - 20.0), cx.debug_bounds(selector).unwrap().center().y)
}

/// 押したまま `from` から左へ `steps` 回 `step` px ずつ動かす（離さない）。
fn drag_left(
    cx: &mut VisualTestContext,
    from: Point<Pixels>,
    steps: usize,
    step: f32,
) -> Point<Pixels> {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let mut at = from;
    for _ in 0..steps {
        at.x -= px(step);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    at
}

fn slide(at: Point<Pixels>, dx: f32, dy: f32, phase: TouchPhase) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
    }
}

fn top_x(cx: &mut VisualTestContext) -> Pixels {
    cx.debug_bounds("nav-top").unwrap().origin.x
}

#[gpui::test]
fn pushing_shows_the_new_page_on_top(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0]);
    let at = cx.debug_bounds("push").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    assert_eq!(depth(&view, cx), 1);
    let top = cx.debug_bounds("nav-top").unwrap();
    assert_eq!(top.origin.x, px(0.0), "the pushed page is in place");
    assert_eq!(top.size.width, px(W));
}

#[gpui::test]
fn mouse_swipe_past_half_goes_back_without_clicking(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    // `tap` を押して左へ。ページは指に付いてくるので、離す所にも `tap` がある。
    let from = right_of(cx, "tap");
    let at = drag_left(cx, from, 12, 20.0);
    assert!(
        top_x(cx) < px(-200.0),
        "follows the pointer: {:?}",
        top_x(cx)
    );
    // 下の画面は上の画面の右隣に隙間なく並び、引っ張られて入ってくる。
    let below = cx.debug_bounds("nav-below").unwrap().origin.x;
    assert_eq!(below, top_x(cx) + px(W));
    assert_eq!(depth(&view, cx), 1, "not until released");
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 0);
    assert_eq!(view.read_with(cx, |v, _| (v.backs, v.taps)), (1, 0));
}

#[gpui::test]
fn short_slow_swipe_snaps_back(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    let from = right_of(cx, "tap");
    let at = drag_left(cx, from, 15, 4.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 1);
    assert_eq!(view.read_with(cx, |v, _| (v.backs, v.taps)), (0, 0));
    assert_eq!(top_x(cx), px(0.0));
    // 戻った後は普通に押せる。
    let tap = cx.debug_bounds("tap").unwrap().center();
    cx.simulate_click(tap, Modifiers::none());
    assert_eq!(view.read_with(cx, |v, _| v.taps), 1);
}

#[gpui::test]
fn a_quick_flick_goes_back_even_if_short(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    let from = right_of(cx, "tap");
    let at = drag_left(cx, from, 4, 25.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 0);
}

#[gpui::test]
fn root_page_and_rightward_or_vertical_drags_do_not_go_back(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0]);
    let from = point(px(W - 20.0), px(250.0));
    let at = drag_left(cx, from, 12, 20.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(
        view.read_with(cx, |v, _| v.backs),
        0,
        "nothing below the root"
    );

    view.update(cx, |v, cx| {
        v.stack.push(1);
        cx.notify()
    });
    cx.run_until_parked();
    // 左から右へ・縦だけに動かしても戻らない。
    let from = point(px(20.0), px(250.0));
    let at = drag_left(cx, from, 10, -20.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    let from = point(px(W - 20.0), px(20.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(from.x, px(280.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        point(from.x, px(280.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(depth(&view, cx), 1);
}

#[gpui::test]
fn a_drift_before_moving_left_still_swipes(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    // 長押しの間の手ブレや、遅れたレーザーが手に追いつく分で、最初は右下へずれてから左へ動かす。
    let from = point(px(W - 120.0), px(150.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let drift = point(from.x + px(60.0), from.y + px(30.0));
    cx.simulate_mouse_move(drift, MouseButton::Left, Modifiers::none());
    assert_eq!(top_x(cx), px(0.0), "the drift does not move the page");
    let mut at = drift;
    for _ in 0..12 {
        at.x -= px(25.0);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    assert!(
        top_x(cx) < px(-200.0),
        "follows from where it settled: {:?}",
        top_x(cx)
    );
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 0);
}

#[gpui::test]
fn the_slider_handle_keeps_its_drag(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    let handle = cx.debug_bounds("slider-handle").unwrap().center();
    let at = drag_left(cx, handle, 10, 20.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 1);
    assert_eq!(view.read_with(cx, |v, _| v.value), 0.0);
    // 指のスライドも同じ。
    view.update(cx, |v, _| v.value = 1.0);
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    let handle = cx.debug_bounds("slider-handle").unwrap().center();
    cx.simulate_event(slide(handle, -14.0, 0.0, TouchPhase::Started));
    for _ in 0..10 {
        cx.simulate_event(slide(handle, -20.0, 0.0, TouchPhase::Moved));
    }
    cx.simulate_event(slide(handle, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(depth(&view, cx), 1);
    assert!(view.read_with(cx, |v, _| v.value) < 0.5);
}

#[gpui::test]
fn touch_slide_goes_back_and_swallows_the_coasting(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1, 2]);
    // スライドの宛先は押した所に固定（UI 層と同じ）。
    let at = point(px(W / 2.0), px(250.0));
    // 縦向きの始まりは奪わない。
    cx.simulate_event(slide(at, -2.0, 14.0, TouchPhase::Started));
    cx.simulate_event(slide(at, -300.0, 0.0, TouchPhase::Moved));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(depth(&view, cx), 2);

    cx.simulate_event(slide(at, -14.0, 0.0, TouchPhase::Started));
    for _ in 0..10 {
        cx.simulate_event(slide(at, -20.0, 0.0, TouchPhase::Moved));
    }
    assert!(top_x(cx) < px(-200.0));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(depth(&view, cx), 1);
    // 離した後の慣性（終わりの段階のスクロール）では、下の画面までは戻らない。
    for _ in 0..5 {
        cx.simulate_event(slide(at, -20.0, 0.0, TouchPhase::Ended));
    }
    assert_eq!(depth(&view, cx), 1);
    // 次のスライドからはまた戻れる。
    cx.simulate_event(slide(at, -14.0, 0.0, TouchPhase::Started));
    cx.simulate_event(slide(at, -300.0, 0.0, TouchPhase::Moved));
    cx.simulate_event(slide(at, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(depth(&view, cx), 0);
}

#[gpui::test]
fn another_hands_pointer_does_not_disturb_the_swipe(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    let from = right_of(cx, "tap");
    let mut at = drag_left(cx, from, 6, 20.0);
    // もう一方の手（ボタンを押していない）が動き、パネルを掴んで離れる（掴んだ手はもう何も送ってこない）。
    cx.simulate_mouse_move(point(px(10.0), px(280.0)), None, Modifiers::none());
    cx.simulate_event(MouseExitEvent {
        position: point(px(10.0), px(280.0)),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    for _ in 0..6 {
        at.x -= px(20.0);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    assert!(top_x(cx) < px(-200.0));
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 0);
}

#[gpui::test]
fn grabbing_the_panel_with_the_swiping_hand_ends_the_swipe(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, vec![0, 1]);
    let from = right_of(cx, "tap");
    let at = drag_left(cx, from, 4, 4.0);
    // 押したまま外れた（その手でパネルを掴んだ）。そこで離したことにし、足りないので戻る。
    cx.simulate_event(MouseExitEvent {
        position: at,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
    assert_eq!(top_x(cx), px(0.0));
    // 掴んでいる間に届かなかった離しの後の動きは何もしない。
    cx.simulate_mouse_move(point(px(10.0), at.y), None, Modifiers::none());
    assert_eq!(depth(&view, cx), 1);
    assert_eq!(view.read_with(cx, |v, _| v.taps), 0);
}

/// 本番と同じく動き（`Motion`）を有効にして、積んだ直後の動きが終わってからスワイプで戻れる。
#[gpui::test]
fn with_motion_push_then_swipe_back(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(design::Motion::default()));
    let (view, cx) = setup(cx, vec![0]);
    let at = cx.debug_bounds("push").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    assert_eq!(depth(&view, cx), 1);
    // 入ってくる動きを待つ（実時間）。
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.update(|w, _| w.refresh());
        cx.run_until_parked();
    }
    assert_eq!(top_x(cx), px(0.0), "settled");
    // 指のスライド
    let p = point(px(W / 2.0), px(250.0));
    cx.simulate_event(slide(p, -14.0, 0.0, TouchPhase::Started));
    for _ in 0..12 {
        cx.simulate_event(slide(p, -20.0, 0.0, TouchPhase::Moved));
    }
    assert!(top_x(cx) < px(-200.0), "follows: {:?}", top_x(cx));
    cx.simulate_event(slide(p, 0.0, 0.0, TouchPhase::Ended));
    assert_eq!(depth(&view, cx), 0);
    // マウス（長押しでマウスの押下になったタッチも同じ）
    let at = cx.debug_bounds("push").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.update(|w, _| w.refresh());
        cx.run_until_parked();
    }
    let from = right_of(cx, "tap");
    let at = drag_left(cx, from, 12, 20.0);
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(depth(&view, cx), 0);
}
