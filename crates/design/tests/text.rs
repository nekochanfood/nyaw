//! 全域の文字の大きさの倍率と太字化（`design::font::set_text`）。掛けるのはテキストシステム（文字を組む所）なので、
//! 画面のコードは等倍の大きさ・太さを書くだけで、要素の文字にも、自前で組んだ文字にも、アイコン・絵文字にも効く。
//! `.text_exact()` の部分木だけが従わない。倍率と太字化は独立で、どの組でも両方が効く。

use std::{cell::RefCell, rc::Rc};

use design::{emoji, font, font_size, icon, icons};
use gpui::{
    Context, Div, FontWeight, IntoElement, Render, TestAppContext, Window, canvas, div, prelude::*,
    px,
};

/// 描くときに、その場所で見えているもの。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    /// コードに書いた大きさと太さ（`window.text_style()`。設定で変わらない）。
    written: (f32, FontWeight),
    /// 描かれる行の高さ（px）と太さ。
    line: f32,
    weight: FontWeight,
    /// 倍率のことを何も知らずに、自前で 20px の文字を組んだときの幅（px）。
    hand_shaped: f32,
}

type Log = Rc<RefCell<Vec<(&'static str, Seen)>>>;

struct View(Log);

/// その場所で見えているものを `name` で書き留める、中身の無い要素。
fn probe(name: &'static str, log: &Log) -> impl IntoElement {
    let log = log.clone();
    canvas(
        move |_, window, _| {
            let style = window.text_style();
            let run = style.to_run(4);
            let hand_shaped =
                window
                    .text_system()
                    .shape_line("Hand".into(), px(20.0), &[run], None);
            let seen = Seen {
                written: (
                    f32::from(style.font_size.to_pixels(window.rem_size())),
                    style.font_weight,
                ),
                line: f32::from(window.line_height()),
                weight: gpui::adjusted_weight(style.font_weight),
                hand_shaped: f32::from(hand_shaped.width),
            };
            log.borrow_mut().push((name, seen));
        },
        |_, _, _, _| (),
    )
}

/// 既存の画面と同じ書き方の中身: 大きさ・太さは定数で決め、倍率のことは何も知らない。
fn content(log: &Log, tag: [&'static str; 6]) -> Div {
    div()
        .flex()
        .flex_col()
        .items_start()
        .font(font::font())
        .text_size(px(font_size::MD))
        .child(probe(tag[0], log))
        .child(
            div()
                .text_size(px(font_size::LG))
                .line_height(px(30.0))
                .font_weight(FontWeight::MEDIUM)
                .child(probe(tag[1], log)),
        )
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .child(probe(tag[2], log)),
        )
        .child(
            div()
                .debug_selector(move || tag[3].into())
                .child(icon(&icons::HEART).size(px(24.0))),
        )
        .child(
            div()
                .debug_selector(move || tag[4].into())
                .child(emoji("😀").size(px(24.0))),
        )
        .child(
            div()
                .debug_selector(move || tag[5].into())
                .child("Some text"),
        )
}

const PLAIN: [&str; 6] = ["body", "heading", "bold", "icon", "emoji", "text"];
const EXACT: [&str; 6] = [
    "x-body",
    "x-heading",
    "x-bold",
    "x-icon",
    "x-emoji",
    "x-text",
];

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.borrow_mut().clear();
        div()
            .child(content(&self.0, PLAIN))
            // 絵の中の文字は、全域の設定に従わない。
            .child(content(&self.0, EXACT).text_exact())
    }
}

/// 倍率 `scale`・太字化 `bold` で描いた結果。
struct Drawn {
    log: Vec<(&'static str, Seen)>,
    sizes: Vec<(&'static str, f32, f32)>,
}

impl Drawn {
    fn seen(&self, name: &str) -> Seen {
        self.log.iter().find(|s| s.0 == name).expect(name).1
    }

    fn size(&self, name: &str) -> (f32, f32) {
        let (_, w, h) = self.sizes.iter().find(|s| s.0 == name).expect(name);
        (*w, *h)
    }
}

fn draw(cx: &mut TestAppContext, scale: f32, bold: bool) -> Drawn {
    let log = Log::default();
    let view = log.clone();
    let (_, cx) = cx.add_window_view(move |_, cx| {
        font::set_text(scale, bold, cx);
        View(view)
    });
    cx.simulate_resize(gpui::size(px(800.0), px(600.0)));
    cx.run_until_parked();
    let names = ["icon", "emoji", "text", "x-icon", "x-emoji", "x-text"];
    let sizes = names
        .map(|name| {
            let b = cx.debug_bounds(name).expect(name);
            (name, f32::from(b.size.width), f32::from(b.size.height))
        })
        .to_vec();
    Drawn {
        log: log.borrow().clone(),
        sizes,
    }
}

/// 配置は整数の px に丸められるので、1px までの差は同じとみなす。
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0
}

#[gpui::test]
fn default_draws_exactly_what_the_code_says(cx: &mut TestAppContext) {
    let d = draw(cx, 1.0, false);
    assert_eq!(d.seen("body").written, (font_size::MD, FontWeight::NORMAL));
    assert_eq!(d.seen("body").weight, FontWeight::NORMAL);
    assert_eq!(
        d.seen("heading").written,
        (font_size::LG, FontWeight::MEDIUM)
    );
    assert_eq!(d.seen("heading").line, 30.0);
    assert_eq!(d.seen("bold").weight, FontWeight::BOLD);
    assert_eq!(d.size("icon"), (24.0, 24.0));
    assert_eq!(d.size("emoji"), (24.0, 24.0));
    // 従わない部分木も、既定では同じ。
    for (plain, exact) in PLAIN.into_iter().zip(EXACT) {
        if plain == "icon" || plain == "emoji" || plain == "text" {
            assert_eq!(d.size(plain), d.size(exact), "{plain}");
        } else {
            assert_eq!(d.seen(plain), d.seen(exact), "{plain}");
        }
    }
}

#[gpui::test]
fn scale_reaches_every_text_icon_and_emoji_without_touching_the_code(cx: &mut TestAppContext) {
    let base = draw(cx, 1.0, false);
    for scale in [0.8, 1.3, 1.5] {
        let d = draw(cx, scale, false);
        for name in ["body", "heading", "bold"] {
            let (seen, was) = (d.seen(name), base.seen(name));
            // コードに書いた値はそのまま（画面のコードは等倍のつもりで書き続けられる）。
            assert_eq!(seen.written, was.written, "{scale} {name}");
            assert_eq!(seen.weight, was.weight, "倍率は太さを変えない");
            // 行の高さは、相対（body）でも絶対（heading の 30px）でも同じ倍率で伸びる。
            assert!(
                near(seen.line, was.line * scale),
                "{scale} {name}: {seen:?}"
            );
            // 倍率を知らずに自前で組んだ文字にも効く（効かせ忘れが起きない）。
            let ratio = seen.hand_shaped / was.hand_shaped;
            assert!((ratio - scale).abs() < 0.02, "{scale} {name}: {ratio}");
        }
        // 要素として置いた文字・アイコン・絵文字も同じ倍率。
        for name in ["icon", "emoji"] {
            let (w, h) = d.size(name);
            assert!(near(w, 24.0 * scale) && near(h, 24.0 * scale), "{name}");
        }
        let (now, was) = (d.size("text"), base.size("text"));
        assert!((now.0 / was.0 - scale).abs() < 0.05, "{scale}: {now:?}");
        assert!((now.1 / was.1 - scale).abs() < 0.05, "{scale}: {now:?}");
        // 従わない部分木は等倍のまま（後から測られる文字の大きさも含めて）。
        for name in ["x-body", "x-heading", "x-bold"] {
            assert_eq!(d.seen(name), base.seen(name), "{scale} {name}");
        }
        for name in ["x-icon", "x-emoji", "x-text"] {
            assert_eq!(d.size(name), base.size(name), "{scale} {name}");
        }
    }
}

#[gpui::test]
fn bold_reaches_every_text_and_keeps_sizes(cx: &mut TestAppContext) {
    let base = draw(cx, 1.0, false);
    let d = draw(cx, 1.0, true);
    for name in ["body", "heading", "bold"] {
        let (seen, was) = (d.seen(name), base.seen(name));
        assert_eq!(seen.weight, FontWeight::BOLD, "{name}");
        assert_eq!(seen.written, was.written, "コードに書いた値はそのまま");
        assert_eq!(seen.line, was.line, "太字化は大きさを変えない");
        // 従わない部分木は元の太さ。
        let exact = format!("x-{name}");
        assert_eq!(d.seen(&exact), base.seen(&exact), "{name}");
    }
    assert_eq!(d.size("icon"), base.size("icon"));
}

/// 倍率と太字化は対等で、互いに依らない。どの組でも両方がそのまま効く。
#[gpui::test]
fn scale_and_bold_combine_at_any_values(cx: &mut TestAppContext) {
    let base = draw(cx, 1.0, false);
    for scale in [0.8, 1.0, 1.5] {
        for bold in [false, true] {
            let d = draw(cx, scale, bold);
            let seen = d.seen("body");
            let line = base.seen("body").line * scale;
            assert!(near(seen.line, line), "{scale} {bold}: {seen:?}");
            let want = if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            };
            assert_eq!(seen.weight, want, "{scale} {bold}");
            assert!(near(d.size("icon").0, 24.0 * scale), "{scale} {bold}");
            assert_eq!(d.seen("x-body"), base.seen("x-body"), "{scale} {bold}");
            assert_eq!(d.size("x-text"), base.size("x-text"), "{scale} {bold}");
        }
    }
}

/// 設定の入口: 同じ値なら描き直さず、数でない・0 以下の倍率は等倍。文字ではない寸法を合わせる `scaled` も同じ倍率。
#[gpui::test]
fn set_text_reports_changes_and_sanitizes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(font::scaled(20.0), px(20.0));
        assert!(font::set_text(1.5, true, cx));
        assert!(!font::set_text(1.5, true, cx), "同じ値なら描き直さない");
        assert_eq!(font::scaled(20.0), px(30.0));
        assert_eq!(gpui::text_settings(), (1.5, true));
        for bad in [f32::NAN, f32::INFINITY, 0.0, -2.0] {
            font::set_text(bad, false, cx);
            assert_eq!(font::scaled(20.0), px(20.0), "{bad}");
            assert!(!font::set_text(bad, false, cx), "{bad}: 等倍のまま");
        }
    });
}
