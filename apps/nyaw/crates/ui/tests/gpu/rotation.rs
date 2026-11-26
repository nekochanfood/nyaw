//! `Window::with_rotation` (vendor/gpui) draws quads rotated clockwise around the given point.
//! Real platform + GPU, so it needs a display. The GUI event loop must run on the main thread, so it has its own main.

pub fn main() {
    use std::{cell::RefCell, f32::consts::FRAC_PI_2, rc::Rc, time::Duration};

    use gpui::{
        AppContext, Context, IntoElement, Radians, Render, Styled, Window, bounds, canvas, div,
        fill, point, prelude::*, px, rgb, size,
    };
    use ui::{capture, open_panel_window};

    const RES: (u32, u32) = (200, 200);
    const RED: [u8; 3] = [0xff, 0x00, 0x00];
    /// A red bar from the middle to the right (60x8), turned a quarter clockwise around the middle.
    struct Bar;
    impl Render for Bar {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0x204060)).child(
                canvas(
                    |_, _, _| (),
                    |_, _, window, _| {
                        let center = point(px(100.0), px(100.0));
                        window.with_rotation(center, Radians(FRAC_PI_2), |window| {
                            let bar = bounds(point(px(100.0), px(96.0)), size(px(60.0), px(8.0)));
                            window.paint_quad(fill(bar, rgb(0xff0000)));
                        });
                    },
                )
                .size_full(),
            )
        }
    }

    let frames = Rc::new(RefCell::new(Vec::new()));
    let out = frames.clone();
    ui::application().run(move |cx| {
        let window = open_panel_window(cx, RES, move |_, cx| cx.new(|_| Bar))
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
                    out.borrow_mut().push(frame.unwrap());
                    break;
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let at = |f: &ui::Frame, x: u32, y: u32| -> [u8; 3] {
        let i = ((y * f.width + x) * 4) as usize;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
    };
    let frames = frames.take();
    let f = frames.first().expect("the window was captured");
    // Turned clockwise, the bar points down from the middle.
    assert_eq!(at(f, 100, 140), RED, "the bar points down");
    assert_ne!(at(f, 140, 100), RED, "not right (unrotated)");
    assert_ne!(at(f, 100, 60), RED, "not up (counterclockwise)");
    println!("rotation: ok");
}
