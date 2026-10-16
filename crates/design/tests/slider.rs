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

fn slide(at: gpui::Point<gpui::Pixels>, dx: f32, phase: TouchPhase) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(dx), px(0.0))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
    }
}

#[gpui::test]
fn touch_slide_on_the_handle_follows_the_finger(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let at = cx.debug_bounds("s-handle").unwrap().center();
    // 長押しなしで、指の移動量の合計だけハンドルが動く（180px の移動幅で 18px = 0.1）。
    cx.simulate_event(slide(at, 9.0, TouchPhase::Started));
    cx.simulate_event(slide(at, 9.0, TouchPhase::Moved));
    assert!((view.read_with(cx, |v, _| v.0) - 0.6).abs() < 0.01);
    // 行き過ぎても、指が戻れば戻る。
    cx.simulate_event(slide(at, 900.0, TouchPhase::Moved));
    assert_eq!(view.read_with(cx, |v, _| v.0), 1.0);
    cx.simulate_event(slide(at, -900.0, TouchPhase::Moved));
    assert!((view.read_with(cx, |v, _| v.0) - 0.6).abs() < 0.01);
    // 終わったら次のスライドは今の位置から。
    cx.simulate_event(slide(at, 0.0, TouchPhase::Ended));
    let now = cx.debug_bounds("s-handle").unwrap().center();
    cx.simulate_event(slide(now, -18.0, TouchPhase::Started));
    assert!((view.read_with(cx, |v, _| v.0) - 0.5).abs() < 0.01);
}

#[gpui::test]
fn touch_slide_off_the_handle_does_nothing(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let s = cx.debug_bounds("s").unwrap();
    let groove = point(s.origin.x + px(180.0), s.center().y);
    cx.simulate_event(slide(groove, 50.0, TouchPhase::Moved));
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
