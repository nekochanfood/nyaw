//! `components::markdown` を実際に組んで描く: ブロックが縦に並び、見出しは本文より大きく、一覧の文字は印の右に下がり、
//! 長い行は幅の中で折り返す。書いたリンクも裸の URL も、押すとブラウザで開く。

use design::{Theme, components::markdown};
use gpui::{
    Bounds, Context, IntoElement, Modifiers, Pixels, Render, TestAppContext, VisualTestContext,
    Window, div, point, prelude::*, px,
};

/// 幅 500px の箱に `source` を描く。
struct View(&'static str);

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p(px(20.0)).child(
            markdown(&Theme::LIGHT, "md", self.0)
                .w(px(500.0))
                .debug_selector(|| "md".into()),
        )
    }
}

fn draw<'a>(cx: &'a mut TestAppContext, source: &'static str) -> &'a mut VisualTestContext {
    let (_, cx) = cx.add_window_view(move |_, _| View(source));
    cx.simulate_resize(gpui::size(px(800.0), px(1200.0)));
    cx.run_until_parked();
    cx
}

fn bounds(cx: &mut VisualTestContext, name: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(name)
        .unwrap_or_else(|| panic!("{name} is not drawn"))
}

const NOTES: &str = "## What's Changed
* feat: logger by @someone in https://github.com/o/r/pull/4
* fix: a much longer line about what changed in this release, long enough that it cannot fit on one line of the box and has to wrap

Plain paragraph.

```
let x = 1;
```
";

#[gpui::test]
fn release_notes_lay_out_as_styled_blocks(cx: &mut TestAppContext) {
    let cx = draw(cx, NOTES);
    let md = bounds(cx, "md");
    let (heading, list, para, code) = (
        bounds(cx, "md-0"),
        bounds(cx, "md-1"),
        bounds(cx, "md-2"),
        bounds(cx, "md-3"),
    );
    // ブロックは上から順に、重ならずに並ぶ。
    for (upper, lower) in [(heading, list), (list, para), (para, code)] {
        assert!(upper.bottom() <= lower.top(), "{upper:?} / {lower:?}");
    }
    assert!(code.bottom() <= md.bottom());
    // 見出しは 1 行でも本文の 1 行より高い（大きい文字）。
    assert!(
        heading.size.height > para.size.height,
        "{heading:?} / {para:?}"
    );
    // 一覧の文字は印の右から始まり、一覧の左端より下がる。
    let (marker, first) = (bounds(cx, "md-1-0-marker"), bounds(cx, "md-1-0-0"));
    assert!(marker.right() <= first.left(), "{marker:?} / {first:?}");
    assert!(first.left() > list.left());
    // 長い行は箱の幅の中で折り返して、何行にもなる（1 行の段落の倍より高い）。
    let long = bounds(cx, "md-1-1-0");
    assert!(long.right() <= md.right(), "{long:?} / {md:?}");
    assert!(
        long.size.height >= para.size.height * 2.0,
        "{long:?} / {para:?}"
    );
    // コードは地の上に余白を取る（1 行でも本文の 1 行より高い）。
    assert!(code.size.height > para.size.height);
}

/// `name` の左端近く（最初の数文字）を押す。
fn click_start_of(cx: &mut VisualTestContext, name: &'static str) {
    let b = bounds(cx, name);
    let at = point(b.left() + px(6.0), b.top() + b.size.height / 2.0);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn links_and_bare_urls_open_in_the_browser(cx: &mut TestAppContext) {
    let cx = draw(
        cx,
        "Plain words.\n\n[the docs](https://example.com/docs)\n\nhttps://example.com/bare.",
    );
    click_start_of(cx, "md-0");
    assert_eq!(cx.opened_url(), None, "リンクの無い文字は何も開かない");
    click_start_of(cx, "md-1");
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs"));
    click_start_of(cx, "md-2");
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://example.com/bare"),
        "文末の句点は URL に入らない"
    );
}
