//! 実 GPU で SVG の配色・サイズ・文章内の折り返しと改行を確かめる。
#[cfg(windows)]
fn main() {
    use gpui::{
        AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div, px, rgb,
    };
    use std::{cell::RefCell, rc::Rc, time::Duration};
    use ui::{PanelRoot, capture, open_panel_window};

    struct Sample;
    impl Render for Sample {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(rgb(0xffffff))
                .text_color(rgb(0x000000))
                .text_size(px(32.))
                .flex()
                .flex_col()
                .child(div().h(px(64.)).child(design::emoji("😀").size(px(64.))))
                .child(
                    div()
                        .w(px(80.))
                        .line_height(px(40.))
                        .child(design::emoji_text("😀😀\n😀😀")),
                )
                .child(
                    div()
                        .w(px(80.))
                        .line_height(px(40.))
                        .child(design::emoji_text("😀😀😀😀")),
                )
                .child(
                    div()
                        .w(px(80.))
                        .line_height(px(40.))
                        .text_center()
                        .child(design::emoji_text("😀")),
                )
                .child(
                    div()
                        .w(px(80.))
                        .line_height(px(40.))
                        .text_right()
                        .child(design::emoji_text("😀")),
                )
                .child(div().child(design::emoji_text("日本語 ❤️ 👍🏽 👩‍💻 🇯🇵")))
        }
    }
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    gpui_platform::application().run(move |cx| {
        design::font::install(cx);
        let window = open_panel_window(cx, (440, 400), |_, cx| {
            let view = cx.new(|_| Sample).into();
            cx.new(|_| PanelRoot::new(view))
        })
        .unwrap();
        let demo = open_panel_window(cx, (800, 600), |_, cx| {
            let view = cx.new(|_| testing::EmojiTest::default()).into();
            cx.new(|_| PanelRoot::new(view))
        })
        .unwrap();
        cx.spawn(async move |cx| {
            for _ in 0..100 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let frame = cx
                    .update_window(window.into(), |_, w, cx| capture(w, cx))
                    .unwrap()
                    .unwrap();
                // 非同期で SVG が届くまで待つ。
                if frame.width == 440
                    && yellow(&frame, 0, 64) > 1000
                    && yellow(&frame, 184, 224) > 200
                {
                    *out.borrow_mut() = Some(frame);
                    break;
                }
            }
            for (name, mode) in [("dark", design::Mode::Dark), ("light", design::Mode::Light)] {
                cx.update(|cx| {
                    cx.set_global(mode);
                    cx.refresh_windows();
                });
                for _ in 0..10 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let frame = cx
                        .update_window(demo.into(), |_, w, cx| capture(w, cx))
                        .unwrap()
                        .unwrap();
                    if frame.width == 800 {
                        save(&frame, &format!("target/tmp/emoji-{name}.bmp"));
                    }
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let frame = result.take().expect("SVG did not render within 5s");
    save(&frame, "target/tmp/emoji-inline.bmp");
    let icon = yellow(&frame, 0, 64);
    let rows = [(64, 104), (104, 144), (144, 184), (184, 224)].map(|(a, b)| yellow(&frame, a, b));
    assert!(
        rows.iter().all(|&n| n > 200),
        "inline emojis on each explicit/wrapped line: {rows:?}"
    );
    assert!(icon > rows[0], "64px icon exceeds two 32px inline emoji");
    assert!(
        rows.windows(2).all(|r| r[0].abs_diff(r[1]) < 100),
        "same inline size on every line: {rows:?}"
    );
    for (top, expected) in [(224, 24), (264, 48)] {
        let left = frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                (top..top + 40).contains(&(i / frame.width as usize))
                    && p[0] > 200
                    && p[1] > 120
                    && p[2] < 100
            })
            .map(|(i, _)| i % frame.width as usize)
            .min()
            .expect("aligned emoji visible");
        assert!(
            (expected..expected + 3).contains(&left),
            "aligned emoji at {left}, expected {expected}"
        );
    }
    println!("emoji: ok (icon {icon}, inline {rows:?}, center/right alignment)");

    fn save(frame: &ui::Frame, path: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&(54 + frame.rgba.len() as u32).to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&frame.width.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-(frame.height as i32)).to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
        for p in frame.rgba.as_chunks::<4>().0 {
            bmp.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bmp).unwrap();
    }

    fn yellow(frame: &ui::Frame, top: usize, bottom: usize) -> usize {
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let y = i / frame.width as usize;
                (top..bottom).contains(&y) && p[0] > 200 && p[1] > 120 && p[2] < 100 && p[3] > 200
            })
            .count()
    }
}

#[cfg(not(windows))]
fn main() {
    println!("emoji: skipped (GPU readback requires Windows)");
}
