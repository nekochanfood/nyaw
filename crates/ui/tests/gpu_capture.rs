//! Checks that a hidden panel window's render can be read back through the production path (real platform + GPU).
//! The GUI event loop must run on the main thread, so this runs in main without the test harness.

#[cfg(windows)]
fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{AppContext, Context, IntoElement, Render, Styled, Window, div, rgb};
    use ui::{capture, open_panel_window};

    const PANEL_BG: u32 = 0x204060;
    /// A view that only fills the background (views sit above the UI layer, so UI-layer tests provide their own).
    struct Solid;
    impl Render for Solid {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(PANEL_BG))
        }
    }

    const RES: (u32, u32) = (200, 100);
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    gpui_platform::application().run(move |cx| {
        let window = open_panel_window(cx, RES, |_, cx| cx.new(|_| Solid))
            .expect("cannot open panel window");
        cx.spawn(async move |cx| {
            // resizing is applied asynchronously, so wait until the readback has the real size (up to 5 s)
            for _ in 0..100 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let captured = cx
                    .update_window(window.into(), |_, window, cx| {
                        (capture(window, cx), window.scale_factor())
                    })
                    .expect("window is closed");
                let done = matches!(&captured.0, Ok(f) if f.width > 1) || captured.0.is_err();
                *out.borrow_mut() = Some(captured);
                if done {
                    break;
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let (frame, scale) = result.take().expect("app did not start");
    let frame = frame.expect("cannot read back from GPU");
    assert_eq!(
        (frame.width, frame.height),
        ((RES.0 as f32 * scale) as u32, (RES.1 as f32 * scale) as u32)
    );
    assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    let [r, g, b] = [PANEL_BG >> 16, PANEL_BG >> 8, PANEL_BG].map(|c| (c & 0xff) as u8);
    assert_eq!(
        &frame.rgba[..4],
        &[r, g, b, 0xff],
        "top-left is the panel background color"
    );
    println!("gpu_capture: ok ({}x{})", frame.width, frame.height);
}

// ponytail: render_to_image is only implemented for Windows in gpui. Remove this branch when Linux is supported.
#[cfg(not(windows))]
fn main() {
    println!("gpu_capture: skipped (non-Windows unsupported)");
}
