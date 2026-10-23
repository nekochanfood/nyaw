//! `clip_rounded` (vendor/gpui) cuts the contents of a rounded `overflow_hidden` element to its rounded corners.
//! Real platform + GPU, so it needs a display. The GUI event loop must run on the main thread, so it has its own main.

fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{
        AppContext, Context, IntoElement, Render, Styled, Window, div, prelude::*, px, rgb,
    };
    use ui::{capture, open_panel_window};

    const RES: (u32, u32) = (200, 104);
    const RED: [u8; 3] = [0xff, 0x00, 0x00];
    /// Fills a 160x64 box at (20, 20) with red, rounded by 32 (a capsule) and clipped to it.
    struct Capsule(bool);
    impl Render for Capsule {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clip = self.0;
            div().size_full().bg(rgb(0x204060)).child(
                div()
                    .absolute()
                    .left(px(20.0))
                    .top(px(20.0))
                    .w(px(160.0))
                    .h(px(64.0))
                    .rounded(px(32.0))
                    .overflow_hidden()
                    .when(clip, |d| d.clip_rounded())
                    .child(div().size_full().bg(rgb(0xff0000))),
            )
        }
    }

    let frames = Rc::new(RefCell::new(Vec::new()));
    let out = frames.clone();
    ui::application().run(move |cx| {
        for clip in [false, true] {
            let out = out.clone();
            let window = open_panel_window(cx, RES, move |_, cx| cx.new(|_| Capsule(clip)))
                .expect("cannot open panel window");
            cx.spawn(async move |cx| {
                for _ in 0..100 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let frame = cx
                        .update_window(window.into(), |_, window, cx| capture(window, cx))
                        .expect("window is closed");
                    if matches!(&frame, Ok(f) if f.width as u32 == RES.0) {
                        out.borrow_mut().push((clip, frame.unwrap()));
                        break;
                    }
                }
            })
            .detach();
        }
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let at = |f: &ui::Frame, x: u32, y: u32| -> [u8; 3] {
        let i = ((y * f.width + x) * 4) as usize;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
    };
    let frames = frames.take();
    assert_eq!(frames.len(), 2, "both windows were captured");
    for (clip, f) in frames {
        // The middle of the capsule is red either way.
        assert_eq!(at(&f, 100, 52), RED, "clip={clip}: middle");
        // The box's top-left corner is outside the rounded corner: red without the clip, cut with it.
        let corner = at(&f, 21, 21);
        if clip {
            assert_ne!(corner, RED, "clip=true: the corner is cut");
        } else {
            assert_eq!(corner, RED, "clip=false: the corner is not cut");
        }
        // The straight edge, away from the corners, is red either way.
        assert_eq!(at(&f, 100, 21), RED, "clip={clip}: straight edge");
        println!("round_clip: clip={clip} ok");
    }
}
