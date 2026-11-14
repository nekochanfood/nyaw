use design::components::{self, DIALOG_WIDTH, Variant};
use design::{PANEL_MARGIN, Theme, spacing};
use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, TestAppContext, VisualTestContext, Window,
    div, px,
};

const SHORT: &str = "Close it?";
const LONG: &str = "This text is long enough to wrap onto many more lines than the three a dialog shows. \
    Whatever does not fit scrolls, while the icon, the title and the buttons stay in place.";

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let no = components::button(&theme, "no", "Cancel", Variant::Default);
        let yes = components::button(&theme, "yes", "OK", Variant::Primary);
        // 本番と同じく窓いっぱいの箱に載せる（パネルは影の余白の内側に収まる）。
        div().size_full().child(components::dialog(
            &theme,
            "d",
            &design::icons::INFO,
            "Title",
            LONG,
            [no, yes],
        ))
    }
}

/// ダイアログの幅で、高さ `h` の窓にする。
fn resize(cx: &mut VisualTestContext, h: u32) {
    cx.simulate_resize(gpui::size(px(DIALOG_WIDTH as f32), px(h as f32)));
    cx.run_until_parked();
}

#[gpui::test]
fn the_panel_fits_the_text_and_shows_at_most_three_lines(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    let h = |cx: &mut VisualTestContext, d: &str| {
        cx.update(|w, _| components::dialog_height(w, "Title", d))
    };
    let (short, long, longer) = (h(cx, SHORT), h(cx, LONG), h(cx, &LONG.repeat(3)));
    // Longer text grows the panel, but only up to three lines.
    assert!(short < long, "{short} {long}");
    assert_eq!(long, longer);

    // At that height the text shows three lines and scrolls the rest (nothing else gets squeezed).
    resize(cx, long);
    let body = cx.debug_bounds("d-body").unwrap().size.height;
    let three = 3.0 * design::font_size::MD * 1.4;
    assert!((f32::from(body) - three).abs() < 1.0, "{body:?}");
}

/// 見出し（アイコン・タイトル・概要）は左揃えで、縁の内側の左端に揃う。ボタンは同じ幅・同じ高さで横 1 列に、渡した順に
/// 左から並ぶ（否定が左、肯定が右端）。パネルは横長。
#[gpui::test]
fn the_heading_is_left_aligned_over_one_row_of_buttons(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    let h = cx.update(|w, _| components::dialog_height(w, "Title", LONG));
    assert!(h < DIALOG_WIDTH, "{h}");
    resize(cx, h);
    let [title, body, no, yes] =
        ["d-title", "d-body", "no", "yes"].map(|s| cx.debug_bounds(s).unwrap());
    let edge = PANEL_MARGIN + spacing::XL;
    let near = |a: gpui::Pixels, b: f32| (a - px(b)).abs() < px(1.0);
    for b in [title, body, no] {
        assert!(near(b.left(), edge), "{b:?}");
    }
    assert!(near(yes.right(), DIALOG_WIDTH as f32 - edge), "{yes:?}");
    assert!(no.right() < yes.left() && no.top() == yes.top());
    assert!(near(no.size.width, yes.size.width.into()), "{no:?} {yes:?}");
    assert_eq!(no.size.height, px(components::BUTTON_HEIGHT));
    // 文 → ボタンは `XL`、ボタンの下は縁（上下の中央に置くので、余りは無い）。
    assert!(
        near(no.top() - body.bottom(), spacing::XL),
        "{body:?} {no:?}"
    );
    assert!(near(no.bottom(), h as f32 - edge), "{no:?}");
}

/// 文字の大きさの倍率を上げると、パネルの高さも概要の枠も同じ倍率の行で測り直される（文字だけ大きくなって潰れない）。
#[gpui::test]
fn the_panel_grows_with_the_text_scale(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    let h =
        |cx: &mut VisualTestContext| cx.update(|w, _| components::dialog_height(w, "Title", LONG));
    let base = h(cx);
    cx.update(|_, cx| design::font::set_text(1.5, true, cx));
    let large = h(cx);
    // アイコン（64）・タイトル 1 行・概要 3 行が 1.5 倍になった分だけ高くなる（余白とボタンは変わらない）。
    let text = 64.0 + (design::font_size::LG + 3.0 * design::font_size::MD) * 1.4;
    assert!(
        (large as f32 - base as f32 - text * 0.5).abs() <= 1.0,
        "{base} {large}"
    );
    resize(cx, large);
    let body = cx.debug_bounds("d-body").unwrap().size.height;
    let three = 3.0 * design::font_size::MD * 1.4 * 1.5;
    assert!((f32::from(body) - three).abs() < 1.5, "{body:?}");
}
