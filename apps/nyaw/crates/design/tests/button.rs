use design::{
    Motion, Theme,
    components::{Variant, button},
};
use gpui::{
    Context, IntoElement, Modifiers, MouseButton, Render, TestAppContext, Window, div, prelude::*,
    px,
};

struct View(bool);

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let b = button(&Theme::DARK, "b", "Go", Variant::Primary);
        div()
            .p(px(40.0))
            .child(if self.0 { b.no_ripple() } else { b })
    }
}

/// ボタンを押した直後に、波紋のために描き続けようとしているか。
fn animating_after_press(cx: &mut TestAppContext, opt_out: bool) -> bool {
    cx.update(|cx| cx.set_global(Motion::default()));
    let (_, cx) = cx.add_window_view(|_, _| View(opt_out));
    cx.simulate_resize(gpui::size(px(400.0), px(200.0)));
    cx.run_until_parked();
    let at = cx.debug_bounds("b").unwrap().center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    // 押下の色の移り変わり（NORMAL）が終わり、波紋（SLOW）だけが残る時点まで待つ。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update(|w, cx| {
        cx.global_mut::<Motion>().animating.clear();
        w.refresh();
    });
    cx.run_until_parked();
    cx.update(|_, cx| !cx.global::<Motion>().animating.is_empty())
}

#[gpui::test]
fn press_starts_a_ripple_unless_opted_out(cx: &mut TestAppContext) {
    assert!(animating_after_press(cx, false));
}

#[gpui::test]
fn no_ripple_opts_out(cx: &mut TestAppContext) {
    assert!(!animating_after_press(cx, true));
}

/// ホバーを覚える要素。
struct Hovered(std::rc::Rc<std::cell::Cell<bool>>);

impl Render for Hovered {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.0.clone();
        div().p(px(40.0)).child(
            div()
                .id("h")
                .debug_selector(|| "h".into())
                .size(px(100.0))
                .on_hover(move |h, _, _| seen.set(*h)),
        )
    }
}

/// ポインタがパネルから離れたら、描き直してもホバーに戻らない（最後の位置の下の要素が光り続けない。中身が動く通知の
/// カードで、指していないカードがホバーの色になっていた）。
#[gpui::test]
fn leaving_the_window_does_not_rehover_on_redraw(cx: &mut TestAppContext) {
    let seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let (_, cx) = cx.add_window_view({
        let seen = seen.clone();
        move |_, _| Hovered(seen)
    });
    cx.run_until_parked();
    let at = cx.debug_bounds("h").unwrap().center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(seen.get());
    cx.simulate_event(gpui::MouseExitEvent {
        position: at,
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    assert!(!seen.get());
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    assert!(!seen.get(), "描き直してもホバーに戻らない");
}
