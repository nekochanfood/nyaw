use design::{Theme, components};
use gpui::{Context, IntoElement, Render, TestAppContext, Window, px};

const SHORT: &str = "Close it?";
const LONG: &str = "This text is long enough to wrap onto many more lines than the three a dialog shows. \
    Whatever does not fit scrolls, while the icon, the title and the buttons stay in place.";

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let ok = components::button(&theme, "ok", "OK", components::Variant::Primary);
        components::dialog(&theme, "d", &design::icons::INFO, "Title", LONG, [ok])
    }
}

#[gpui::test]
fn the_panel_fits_the_text_and_shows_at_most_three_lines(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    let h = |cx: &mut gpui::VisualTestContext, d: &str| {
        cx.update(|w, _| components::dialog_height(w, 440, "Title", d, 1))
    };
    let (short, long, longer) = (h(cx, SHORT), h(cx, LONG), h(cx, &LONG.repeat(3)));
    // Longer text grows the panel, but only up to three lines.
    assert!(short < long, "{short} {long}");
    assert_eq!(long, longer);

    // At that height the text shows three lines and scrolls the rest (nothing else gets squeezed).
    cx.simulate_resize(gpui::size(px(440.0), px(long as f32)));
    cx.run_until_parked();
    let body = cx.debug_bounds("d-body").unwrap().size.height;
    let three = 3.0 * design::font_size::MD * 1.4;
    assert!((f32::from(body) - three).abs() < 1.0, "{body:?}");
}
