//! `corner_superellipse` (vendor/gpui) reshapes rounded corners per pixel in the DirectX shader.
//! A 100x100 red box at (20, 20) with radius 50 is probed on its corner's diagonal, 11 and 20 px in from the corner:
//! a circle (k=1) covers 20 but not 11, a squircle (k=2) covers both, a bevel (k=0) covers neither.
//! Real platform + GPU, so it needs a display. The GUI event loop must run on the main thread, so it has its own main.

pub fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{
        AppContext, Context, IntoElement, Render, Styled, Window, div, prelude::*, px, rgb,
    };
    use ui::{capture, open_panel_window};

    const RES: (u32, u32) = (140, 140);
    const RED: [u8; 3] = [0xff, 0x00, 0x00];
    /// (k, covers the pixel 11 px in, covers the pixel 20 px in)
    const CASES: [(f32, bool, bool); 3] =
        [(0.0, false, false), (1.0, false, true), (2.0, true, true)];
    struct Box(f32);
    impl Render for Box {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0x204060)).child(
                div()
                    .absolute()
                    .left(px(20.0))
                    .top(px(20.0))
                    .w(px(100.0))
                    .h(px(100.0))
                    .rounded(px(50.0))
                    .corner_superellipse(self.0)
                    .bg(rgb(0xff0000)),
            )
        }
    }

    let frames = Rc::new(RefCell::new(Vec::new()));
    let out = frames.clone();
    ui::application().run(move |cx| {
        for (k, ..) in CASES {
            let out = out.clone();
            let window = open_panel_window(cx, RES, move |_, cx| cx.new(|_| Box(k)))
                .expect("cannot open panel window");
            cx.spawn(async move |cx| {
                for _ in 0..100 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let frame = cx
                        .update_window(window.into(), |_, window, cx| capture(window, cx))
                        .expect("window is closed");
                    if matches!(&frame, Ok(f) if f.width == RES.0) {
                        out.borrow_mut().push((k, frame.unwrap()));
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
    assert_eq!(frames.len(), CASES.len(), "all windows were captured");
    for (k, f) in frames {
        let (_, in11, in20) = *CASES.iter().find(|c| c.0 == k).unwrap();
        assert_eq!(at(&f, 70, 70), RED, "k={k}: middle");
        assert_eq!(at(&f, 70, 21), RED, "k={k}: straight edge");
        assert_eq!(at(&f, 31, 31) == RED, in11, "k={k}: 11 px in");
        assert_eq!(at(&f, 40, 40) == RED, in20, "k={k}: 20 px in");
        println!("superellipse: k={k} ok");
    }
}
