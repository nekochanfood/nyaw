//! 実 GPU でコントローラガイドを描画する: Testing のガイド画面（ダーク・ライト、割り当てに合わせた強調）と、
//! 全機種・左右の全ボタンを囲んだ一覧、全機種のセットアップの完了画面（左手を寄って映す）を `target/tmp/guide-*.bmp` に
//! 書き出す（円の見た目と寄り方は画像を目視確認する）。
//! 検査: 画像が読み込まれて描かれ、見えるボタンの円（アクセント色）が描かれる。
#[cfg(windows)]
pub fn main() {
    use gpui::{AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
    use guide::{Callout, Model};
    use kernel::Hand;
    use std::time::Duration;
    use testing::{GUIDE_PX, GuideTest, Shown};
    use ui::{PanelRoot, capture, open_panel_window};

    /// 全機種・左右の全ボタン（一覧の 1 枚）。
    struct Sheet;
    const CELL: f32 = 260.0;
    const COLUMNS: usize = 4;
    impl Render for Sheet {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = design::Theme::current(cx);
            let mut cells = Vec::new();
            for m in Model::ALL {
                for hand in [Hand::Left, Hand::Right] {
                    let callouts = guide::sheet(m, hand)
                        .iter()
                        .map(|p| Callout {
                            id: p.id.clone().into(),
                            parts: vec![p.id.as_str()],
                            title: guide::part_name(cx, &p.id),
                            detail: None,
                        })
                        .collect();
                    cells.push(guide::controller(m, hand).size(px(CELL)).callouts(callouts));
                }
            }
            div()
                .size_full()
                .bg(gpui::rgba(theme.background))
                .flex()
                .flex_wrap()
                .children(cells)
        }
    }

    let sheet_px = (
        (CELL * (1.0 + 2.0 * guide::GUTTER)) as u32 * COLUMNS as u32,
        (CELL as u32) * (Model::ALL.len() * 2).div_ceil(COLUMNS) as u32,
    );
    let shots = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let out = shots.clone();
    gpui_platform::application().run(move |cx| {
        design::font::install(cx);
        let open = |cx: &mut gpui::App, setup: fn(&mut GuideTest)| -> gpui::AnyWindowHandle {
            open_panel_window(cx, GUIDE_PX, move |_, cx| {
                let view = cx
                    .new(|cx| {
                        let mut g = GuideTest::new(0, cx);
                        setup(&mut g);
                        g
                    })
                    .into();
                cx.new(|_| PanelRoot::new(view))
            })
            .unwrap()
            .into()
        };
        // 使用中（コントローラが無いので Quest Touch Plus）・Index の左手で決定を強調・Vive の全ボタン。
        let mut windows = vec![
            ("in-use".to_owned(), open(cx, |_| {})),
            (
                "index-left-select".to_owned(),
                open(cx, |g| {
                    g.shown = Shown::Model(Model::Index);
                    g.hand = Hand::Left;
                    g.hovered = Some(0);
                }),
            ),
            (
                "vive-all".to_owned(),
                open(cx, |g| {
                    g.shown = Shown::Model(Model::Vive);
                    g.all_parts = true;
                }),
            ),
        ];
        // セットアップの完了画面（パネルと同じ 1152 × 864）。開発ビルドのドロップダウンと同じく、映す機種を選ぶ。
        for m in Model::ALL {
            let window = open_panel_window(cx, (1152, 864), move |_, cx| {
                cx.new(|_| {
                    let mut v = setup::SetupWizard::new(0);
                    while v.wizard.next() {}
                    v.preview = Some(m);
                    v
                })
            })
            .unwrap();
            windows.push((
                format!("done-{}", m.render_model(Hand::Left)),
                window.into(),
            ));
        }
        let sheet = open_panel_window(cx, sheet_px, |_, cx| {
            let view = cx.new(|_| Sheet).into();
            cx.new(|_| PanelRoot::new(view))
        })
        .unwrap();
        cx.spawn(async move |cx| {
            for mode in [design::Mode::Dark, design::Mode::Light] {
                cx.update(|cx| {
                    cx.set_global(mode);
                    cx.refresh_windows();
                });
                // 画像のデコードはバックグラウンドなので、届くまで待つ。
                cx.background_executor()
                    .timer(Duration::from_millis(2500))
                    .await;
                let name = format!("{mode:?}").to_lowercase();
                for (label, window) in &windows {
                    let frame = cx
                        .update_window(*window, |_, w, cx| capture(w, cx))
                        .unwrap()
                        .unwrap();
                    out.borrow_mut().push((format!("{label}-{name}"), frame));
                }
                let frame = cx
                    .update_window(sheet.into(), |_, w, cx| capture(w, cx))
                    .unwrap()
                    .unwrap();
                out.borrow_mut().push((format!("sheet-{name}"), frame));
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let shots = shots.take();
    assert_eq!(
        shots.len(),
        2 * (4 + Model::ALL.len()),
        "every window captured"
    );
    for (name, frame) in &shots {
        save(frame, &format!("target/tmp/guide-{name}.bmp"));
        // コントローラの画像（白〜灰の本体）と、円（アクセントの青）が描かれている。
        let px = frame.rgba.as_chunks::<4>().0;
        let body = px
            .iter()
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[0].abs_diff(p[2]) < 12)
            .count();
        let accent = px
            .iter()
            .filter(|p| p[2] > 180 && p[0] < 120 && p[2] as i32 - p[0] as i32 > 90)
            .count();
        assert!(body > 5000, "{name}: controller image not drawn ({body})");
        assert!(accent > 300, "{name}: no highlight ring ({accent})");
        println!("guide {name}: body {body}, accent {accent}");
    }

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
}

#[cfg(not(windows))]
pub fn main() {
    println!("guide: skipped (GPU readback requires Windows)");
}
