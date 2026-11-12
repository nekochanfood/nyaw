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

/// 文字の大きさの倍率を上げると、パネルの高さも概要の枠も同じ倍率の行で測り直される（文字だけ大きくなって潰れない）。
#[gpui::test]
fn the_panel_grows_with_the_text_scale(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    let h = |cx: &mut gpui::VisualTestContext| {
        cx.update(|w, _| components::dialog_height(w, 440, "Title", LONG, 1))
    };
    let base = h(cx);
    cx.update(|_, cx| design::font::set_text(1.5, true, cx));
    let large = h(cx);
    // アイコン（64）・タイトル 1 行・概要 3 行が 1.5 倍になった分だけ高くなる（余白とボタンは変わらない）。
    let text = 64.0 + (design::font_size::LG + 3.0 * design::font_size::MD) * 1.4;
    assert!(
        (large as f32 - base as f32 - text * 0.5).abs() <= 1.0,
        "{base} {large}"
    );
    cx.simulate_resize(gpui::size(px(440.0), px(large as f32)));
    cx.run_until_parked();
    let body = cx.debug_bounds("d-body").unwrap().size.height;
    let three = 3.0 * design::font_size::MD * 1.4 * 1.5;
    assert!((f32::from(body) - three).abs() < 1.5, "{body:?}");
}
