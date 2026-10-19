use design::{Effects, SurfacePointers, Theme, components};
use gpui::{
    Context, IntoElement, Render, TestAppContext, Window, div, point, prelude::*, px, rgba,
};

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        components::card_panel(&Theme::DARK)
            .rounded(px(40.))
            .child(div().size(px(20.)).bg(rgba(Theme::DARK.accent_solid)))
    }
}

#[gpui::test]
fn effects_toggle_independently_without_fading_content(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    cx.simulate_resize(gpui::size(px(400.), px(300.)));
    for translucent in [false, true] {
        for pointer_light in [false, true] {
            cx.update(|window, cx| {
                Effects {
                    translucent,
                    pointer_light,
                }
                .set(cx);
                SurfacePointers::set(window, 0, Some(point(px(160.), px(140.))), cx);
                window.draw(cx).clear(cx);
                let quads = window.painted_quads();
                let original: gpui::Hsla = rgba(Theme::DARK.card).into();
                let card = quads
                    .iter()
                    .find_map(|q| {
                        q.background
                            .as_solid()
                            .filter(|c| (c.h, c.s, c.l) == (original.h, original.s, original.l))
                    })
                    .expect("カードの面");
                assert!((card.a - if translucent { 0.86 } else { 1. }).abs() < 0.001);
                assert!(
                    quads
                        .iter()
                        .any(|q| q.background.as_solid()
                            == Some(rgba(Theme::DARK.accent_solid).into())),
                    "部品は不透明なまま"
                );
                let lights = quads
                    .iter()
                    .filter(|q| q.background.as_solid().is_some_and(|c| c.a < 0.01))
                    .count();
                assert_eq!(lights > 0, pointer_light);
            });
        }
    }
}

#[gpui::test]
fn leaving_one_hand_preserves_the_other_and_window_state_is_removed(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    cx.simulate_resize(gpui::size(px(400.), px(300.)));
    cx.update(|window, cx| {
        Effects::ENABLED.set(cx);
        let at = Some(point(px(120.), px(120.)));
        assert!(SurfacePointers::set(window, 0, at, cx));
        assert!(
            !SurfacePointers::set(window, 0, at, cx),
            "静止時は描き直さない"
        );
        SurfacePointers::set(window, 1, at, cx);
        SurfacePointers::set(window, 0, None, cx);
        window.draw(cx).clear(cx);
        let lights = |window: &Window| {
            window
                .painted_quads()
                .iter()
                .filter(|q| q.background.as_solid().is_some_and(|c| c.a < 0.01))
                .count()
        };
        assert!(lights(window) > 0, "もう一方の手が照らしている");
        SurfacePointers::remove(window.window_handle().window_id(), cx);
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(lights(window), 0, "閉じた窓の光源を捨てる");
    });
}
