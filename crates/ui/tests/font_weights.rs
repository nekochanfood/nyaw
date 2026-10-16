//! The embedded font draws each weight we use (NORMAL / MEDIUM / BOLD) with its own face. The weights ship as
//! separate static files, so a face that fails to register or match silently falls back to another weight.
//! The GUI event loop must run on the main thread, so this runs in main without the test harness.

#[cfg(windows)]
fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{
        AppContext, Context, FontWeight, IntoElement, ParentElement, Render, Styled, Window, div,
        rgb,
    };
    use ui::{PanelRoot, capture, open_panel_window};

    const TEXT: &str = "Weight 吾輩は猫である。設定を開く";
    const RES: (u32, u32) = (440, 60);
    const WEIGHTS: [FontWeight; 3] = [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::BOLD];
    struct Line(FontWeight);
    impl Render for Line {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(rgb(0xffffff))
                .text_color(rgb(0x000000))
                .text_size(gpui::px(24.0))
                .font_weight(self.0)
                .child(TEXT)
        }
    }

    let result = Rc::new(RefCell::new(Vec::new()));
    let out = result.clone();
    ui::application().run(move |cx| {
        design::font::install(cx);
        let windows: Vec<_> = WEIGHTS
            .into_iter()
            .map(|weight| {
                open_panel_window(cx, RES, |_, cx| {
                    let v = cx.new(|_| Line(weight)).into();
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
    let [normal, medium, bold] = &frames[..] else {
        panic!("rendered {} of 3 windows", frames.len());
    };
    // ink = darkness summed over the frame; a heavier face puts down more of it
    let ink = |f: &ui::Frame| -> u64 {
        f.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| 255 - p[0] as u64)
            .sum()
    };
    let (n, m, b) = (ink(normal), ink(medium), ink(bold));
    assert!(n > 0, "the NORMAL line has ink");
    assert!(
        n < m && m < b,
        "each weight is heavier than the last: {n} / {m} / {b}"
    );
    println!("font_weights: ok ({n} / {m} / {b})");
}

#[cfg(not(windows))]
fn main() {
    println!("font_weights: skipped (non-Windows unsupported)");
}
