use design::{Theme, components::slider};
use gpui::{
    Context, IntoElement, Modifiers, MouseButton, Render, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, Window, div, point, prelude::*, px,
};

struct View(f32);

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .p(px(40.0))
            .child(slider(&Theme::DARK, "s", self.0, move |v, _, cx| {
                this.update(cx, |s, cx| {
                    s.0 = v;
                    cx.notify()
                })
            }))
    }
}

fn setup(cx: &mut TestAppContext) -> (gpui::Entity<View>, &mut gpui::VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, _| View(0.5));
    cx.simulate_resize(gpui::size(px(400.0), px(200.0)));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    (view, cx)
}

#[gpui::test]
fn grab_follows_outside_the_window_until_release(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let at = cx.debug_bounds("s-handle").unwrap().center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    // ウィンドウの外（右の遠く）までドラッグしても掴んだまま、端で止まる。
    cx.simulate_mouse_move(
        point(px(5000.0), px(900.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(view.read_with(cx, |v, _| v.0), 1.0);
    // 外から戻ってきても追従する。
    let s = cx.debug_bounds("s").unwrap();
    cx.simulate_mouse_move(
        point(s.origin.x + px(100.0), px(900.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    let held = view.read_with(cx, |v, _| v.0);
    assert!(held > 0.4 && held < 0.6, "{held}");
    // 外で離したら止まる。
    cx.simulate_mouse_up(
        point(px(5000.0), px(900.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(
        point(s.origin.x + px(10.0), px(900.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(view.read_with(cx, |v, _| v.0), held);
}

#[gpui::test]
fn lost_release_is_noticed_on_the_next_move(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let at = cx.debug_bounds("s-handle").unwrap().center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    // 離した知らせが来ないまま、ボタンを押していない動きが来た。
    cx.simulate_mouse_move(point(px(5000.0), px(0.0)), None, Modifiers::none());
    assert_eq!(view.read_with(cx, |v, _| v.0), 0.5);
}

#[gpui::test]
fn only_the_handle_can_be_grabbed(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let s = cx.debug_bounds("s").unwrap();
    // 溝を押してそのまま動かしても何も起きない。
    let groove = point(s.origin.x + px(180.0), s.center().y);
    cx.simulate_mouse_down(groove, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(s.origin.x + px(20.0), s.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(groove, MouseButton::Left, Modifiers::none());
    assert_eq!(view.read_with(cx, |v, _| v.0), 0.5);
}

#[gpui::test]
fn scroll_slide_is_not_relative(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let at = cx.debug_bounds("s-handle").unwrap().center();
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(18.0), px(0.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    assert_eq!(view.read_with(cx, |v, _| v.0), 0.5);
}

#[gpui::test]
fn grabbing_does_not_move_the_value(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let b = cx.debug_bounds("s-handle").unwrap();
    // ハンドルの端を掴んでも値は動かない。ポインタの絶対位置 − 掴んだずれ が値になる。
    let edge = point(b.origin.x + px(2.0), b.center().y);
    cx.simulate_mouse_down(edge, MouseButton::Left, Modifiers::none());
    assert_eq!(view.read_with(cx, |v, _| v.0), 0.5);
    cx.simulate_mouse_move(
        point(edge.x + px(18.0), edge.y),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert!((view.read_with(cx, |v, _| v.0) - 0.6).abs() < 0.01);
}
