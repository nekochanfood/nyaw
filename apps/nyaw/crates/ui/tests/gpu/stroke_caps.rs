//! The ends of strokes are round: the corner bracket (`beam::bracket`, shared by the dock ghost and others) has round line
//! caps, and the laser (`beam::Laser`) is rounded at the hand end. Renders both and checks the pixels.
//! Real platform + GPU, so it needs a display. The GUI event loop must run on the main thread, so it has its own main.

pub fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use beam::bracket::{self, ARM_PX, Corner};
    use design::{BRACKET_LINE_PX, radius};
    use gpui::{
        AnyWindowHandle, AppContext, Context, IntoElement, Render, Window, div, prelude::*,
    };
    use ui::{capture, open_panel_window};

    const FRAME: (u32, u32) = (400, 400);
    /// Draws the bottom-right corner of a bracket around the whole window in opaque white.
    struct Bracket;
    impl Render for Bracket {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(bracket::frame(&[Corner::BottomRight], 0xffffffff).size_full())
        }
    }

    let frames = Rc::new(RefCell::new(Vec::new()));
    let out = frames.clone();
    ui::application().run(move |cx| {
        let windows: [(AnyWindowHandle, (u32, u32)); 2] = [
            (
                open_panel_window(cx, FRAME, |_, cx| cx.new(|_| Bracket))
                    .expect("cannot open panel window")
                    .into(),
                FRAME,
            ),
            (
                open_panel_window(cx, beam::LASER_PX, |_, cx| cx.new(|_| beam::Laser))
                    .expect("cannot open panel window")
                    .into(),
                beam::LASER_PX,
            ),
        ];
        for (i, (window, res)) in windows.into_iter().enumerate() {
            let out = out.clone();
            cx.spawn(async move |cx| {
                for _ in 0..100 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let frame = cx
                        .update_window(window, |_, window, cx| capture(window, cx))
                        .expect("window is closed");
                    if matches!(&frame, Ok(f) if f.width == res.0) {
                        out.borrow_mut().push((i, frame.unwrap()));
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

    let alpha = |f: &ui::Frame, x: u32, y: u32| f.rgba[((y * f.width + x) * 4 + 3) as usize];
    let mut frames = frames.take();
    frames.sort_by_key(|(i, _)| *i);
    assert_eq!(frames.len(), 2, "both windows were captured");

    // The bracket's arm on the right edge ends `ARM_PX` above where the corner starts to round.
    let f = &frames[0].1;
    let half = BRACKET_LINE_PX / 2.0;
    let x = FRAME.0 as f32 - half;
    let end = FRAME.1 as f32 - half - (radius::XL - half) - ARM_PX;
    let (x, end) = (x as u32, end as u32);
    assert_eq!(alpha(f, x - 1, end + 10), 255, "the arm is drawn");
    // Beyond the end along the line, within half the width: covered by a round (or square) cap, empty with a flat end.
    assert_eq!(
        alpha(f, x - 1, end - 2),
        255,
        "the line runs past its end (a cap)"
    );
    // Off the end diagonally, where a square cap would still cover: empty, so the cap is round.
    let corner = alpha(f, FRAME.0 - 1, end - 3);
    assert!(corner < 128, "the cap is round, not square: alpha {corner}");
    println!("stroke_caps: bracket ok");

    // The laser fills the texture top to bottom, except at the hand end (left), which is rounded.
    let f = &frames[1].1;
    let mid = beam::LASER_PX.1 / 2;
    assert!(alpha(f, 20, 0) > 150, "the body reaches the top edge");
    assert!(
        alpha(f, 0, mid) > 150,
        "the hand end reaches the left edge in the middle"
    );
    let corner = alpha(f, 0, 0);
    assert!(corner < 30, "the hand end's corner is cut: alpha {corner}");
    println!("stroke_caps: laser ok");
}
