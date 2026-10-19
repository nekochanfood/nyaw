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

struct PressSample(Theme, Variant);

impl Render for PressSample {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        design::components::card(&self.0)
            .p(px(40.))
            .child(button(&self.0, "press", "Press", self.1))
    }
}

#[gpui::test]
fn press_sinks_and_release_restores_hover_without_moving_the_hit_area(cx: &mut TestAppContext) {
    for original in [Theme::DARK, Theme::LIGHT] {
        for theme in [original, original.translucent()] {
            for variant in [
                Variant::Default,
                Variant::OnBackground,
                Variant::Ghost,
                Variant::Primary,
                Variant::Danger,
            ] {
                let (_, cx) = cx.add_window_view(|_, _| PressSample(theme, variant));
                cx.simulate_resize(gpui::size(px(400.), px(200.)));
                cx.run_until_parked();
                let bounds = cx.debug_bounds("press").unwrap();
                let at = bounds.center();
                cx.simulate_mouse_move(at, None, Modifiers::none());
                cx.run_until_parked();
                let hover = cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    window.painted_quads().last().unwrap().background
                });
                cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    assert_eq!(
                        window.painted_quads().last().unwrap().background.as_solid(),
                        Some(gpui::rgba(theme.background).into()),
                        "押している間は背景の色"
                    );
                });
                assert_eq!(cx.debug_bounds("press").unwrap(), bounds);
                cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
                cx.run_until_parked();
                cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    assert_eq!(
                        window.painted_quads().last().unwrap().background,
                        hover,
                        "離すとホバーの色へ戻る"
                    );
                });
                assert_eq!(cx.debug_bounds("press").unwrap(), bounds);
            }
        }
    }
}
