//! Checks that a hidden panel window's render can be read back through the production path (real platform + GPU).
//! The GUI event loop must run on the main thread, so this runs in main without the test harness.

#[cfg(windows)]
fn main() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::Duration,
    };

    use gpui::{AppContext, Context, IntoElement, Render, Styled, Window, div, rgb};
    use ui::{capture, open_panel_window};

    const PANEL_BG: u32 = 0x204060;
    /// A view that only fills the background (views sit above the UI layer, so UI-layer tests provide their own).
    struct Solid(Rc<Cell<u32>>);
    impl Render for Solid {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(self.0.get()))
        }
    }
    /// the color switched to when checking that readback is one call behind.
    const OTHER_BG: u32 = 0x602040;
    let color = Rc::new(Cell::new(PANEL_BG));
    let view_color = color.clone();
    let delayed = Rc::new(RefCell::new(None));
    let delayed_out = delayed.clone();

    const RES: (u32, u32) = (200, 104);
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    let lod_results = Rc::new(RefCell::new(Vec::new()));
    let lod_out = lod_results.clone();
    ui::application().run(move |cx| {
        let window = open_panel_window(cx, RES, |_, cx| cx.new(|_| Solid(view_color)))
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
            for lod in [0, 1, 2, 3, 0] {
                cx.update_window(window.into(), |_, window, cx| ui::set_render_lod(window, cx, RES, lod)).unwrap();
                for attempt in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    let (frame, viewport) = cx.update_window(window.into(), |_, window, cx| {
                        (capture(window, cx), window.viewport_size())
                    }).unwrap();
                    let ready = matches!(&frame, Ok(f) if (f.width, f.height) == (RES.0 >> lod, RES.1 >> lod));
                    if ready || attempt == 99 {
                        lod_out.borrow_mut().push((lod, frame, viewport));
                        break;
                    }
                }
            }
            // read_back does not wait for the GPU: it returns the previous picture, and collect returns the one just drawn.
            let pair = cx
                .update_window(window.into(), |_, window, cx| {
                    color.set(OTHER_BG);
                    window.refresh();
                    window.draw(cx).clear(cx);
                    let before = ui::read_back(window).map(|f| f.rgba[..4].to_vec());
                    let after = ui::collect(window).map(|f| f.rgba[..4].to_vec());
                    (before, after)
                })
                .unwrap();
            *delayed_out.borrow_mut() = Some(pair);
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
    for (lod, frame, viewport) in lod_results.take() {
        let frame = frame.unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (RES.0 >> lod, RES.1 >> lod),
            "LOD{lod}"
        );
        assert_eq!(
            viewport,
            gpui::size(gpui::px(RES.0 as f32), gpui::px(RES.1 as f32)),
            "LOD{lod}: logical size"
        );
        assert_eq!(
            &frame.rgba[frame.rgba.len() - 4..],
            &[r, g, b, 255],
            "LOD{lod}: rendered through bottom-right corner"
        );
        println!(
            "gpu_capture: LOD{lod} ok ({}x{})",
            frame.width, frame.height
        );
    }
    let rgba = |c: u32| {
        [c >> 16, c >> 8, c, 0xff]
            .map(|c| (c & 0xff) as u8)
            .to_vec()
    };
    let (before, after) = delayed.take().expect("delayed readback not checked");
    assert_eq!(
        before.unwrap(),
        rgba(PANEL_BG),
        "read_back returns the previous picture"
    );
    assert_eq!(
        after,
        Some(rgba(OTHER_BG)),
        "collect returns the picture just drawn"
    );
    println!("gpu_capture: delayed readback ok");
}

// ponytail: render_to_image is only implemented for Windows in gpui. Remove this branch when Linux is supported.
#[cfg(not(windows))]
fn main() {
    println!("gpu_capture: skipped (non-Windows unsupported)");
}
