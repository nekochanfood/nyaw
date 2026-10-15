//! Japanese drawn through the font fallback (Noto Sans → Noto Sans JP) must look the same as Japanese drawn with
//! Noto Sans JP directly. gpui_windows used to look the fallback face up again by name, which could land on a
//! different face, so kana and kanji were drawn as unrelated glyphs.
//! The GUI event loop must run on the main thread, so this runs in main without the test harness.

#[cfg(windows)]
fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div, rgb};
    use ui::{PanelRoot, capture, open_panel_window};

    const TEXT: &str = "吾輩は猫である。設定を開く";
    const RES: (u32, u32) = (400, 60);
    /// `None` = the default font (Noto Sans, falling back to Noto Sans JP), `Some` = this family directly.
    struct Line(Option<&'static str>);
    impl Render for Line {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let d = div()
                .size_full()
                .bg(rgb(0xffffff))
                .text_color(rgb(0x000000))
                .text_size(gpui::px(24.0));
            match self.0 {
                Some(family) => d.font_family(family),
                None => d,
            }
            .child(TEXT)
        }
    }

    let result = Rc::new(RefCell::new(Vec::new()));
    let out = result.clone();
    gpui_platform::application().run(move |cx| {
        design::font::install(cx);
        let windows: Vec<_> = [None, Some(design::font::SECONDARY)]
            .into_iter()
            .map(|family| {
                open_panel_window(cx, RES, |_, cx| {
                    let v = cx.new(|_| Line(family)).into();
                    cx.new(|_| PanelRoot::new(v))
                })
                .expect("cannot open panel window")
            })
            .collect();
        cx.spawn(async move |cx| {
            for window in windows {
                // resizing is applied asynchronously, so wait until the readback has the real size (up to 5 s)
                for _ in 0..100 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let f = cx
                        .update_window(window.into(), |_, w, cx| capture(w, cx))
                        .unwrap()
                        .expect("cannot read back from GPU");
                    if f.width > 1 {
                        out.borrow_mut().push(f);
                        break;
                    }
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let frames = result.take();
    let [fallback, direct] = &frames[..] else {
        panic!("rendered {} of 2 windows", frames.len());
    };
    assert!(
        direct.rgba.chunks_exact(4).any(|p| p[0] < 128),
        "the reference line has ink"
    );
    assert!(
        fallback == direct,
        "Japanese through the fallback differs from {} drawn directly",
        design::font::SECONDARY
    );
    println!("jp_fallback: ok");
}

#[cfg(not(windows))]
fn main() {
    println!("jp_fallback: skipped (non-Windows unsupported)");
}
