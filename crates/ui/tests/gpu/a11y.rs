//! アクセシビリティの見た目: 文字の大きさの倍率と太字化を替えながら、設定画面の各カテゴリ・色を作るダイアログ・wrist の文字盤の
//! 見本を実 GPU で描き、`target/tmp/look-a11y-*.bmp` に書き出す（細かい見た目は絵を目で見る）。検査するのは、倍率・太字化で
//! 文字のインクが増えること、等幅の書体も崩れないこと、従わない文字（`.text_exact()` の文字盤）が 1 画素も変わらないこと。

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use ui::{PanelControl, capture, open_panel_window};

use crate::look::{FACES_PX, Faces, save};

/// 白地に黒い文字（既定の書体と、太さが 1 つしか無い等幅の書体）。インクの量を比べる。
struct Lines(bool);

const LINES_PX: (u32, u32) = (900, 120);
/// 仮想キーボード（フルサイズが収まる大きさ）・ダッシュボードのバー・そこにはめる画面の窓。
const KEYBOARD_PX: (u32, u32) = (1700, 760);
const BAR_PX: (u32, u32) = (1024, 118);
const PAGE_PX: (u32, u32) = (1024, 768);

impl Render for Lines {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let line = div()
            .size_full()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x000000))
            .text_size(px(24.0));
        if self.0 {
            line.font(design::font::mono())
                .child("Mono 0123456789 kernel::tick()")
        } else {
            line.child("Weight 吾輩は猫である。設定を開く")
        }
    }
}

/// 書き出す状態: 名前、文字の大きさの倍率、太字化。
const STATES: [(&str, f32, bool); 5] = [
    ("base", 1.0, false),
    ("small", kernel::TEXT_SCALE_MIN, false),
    ("large", kernel::TEXT_SCALE_MAX, false),
    ("bold", 1.0, true),
    ("large-bold", kernel::TEXT_SCALE_MAX, true),
];

/// 設定画面のカテゴリ（添字と書き出す名前）。
const PAGES: [(usize, &str); 6] = [
    (0, "general"),
    (1, "appearance"),
    (2, "watch"),
    (3, "accessibility"),
    (4, "pointer"),
    (8, "about"),
];

pub fn main() {
    if !crate::common::should_run("a11y") {
        return;
    }
    let shots = Rc::new(RefCell::new(Vec::new()));
    let out = shots.clone();
    ui::application().run(move |cx| {
        design::font::install(cx);
        design::corner::install();
        cx.set_global(ui::prefs::FacePreview(wrist::preview));
        let mut windows: Vec<(String, gpui::AnyWindowHandle, u32)> = Vec::new();
        let mut open = |name: &str, handle: gpui::AnyWindowHandle, width: u32| {
            windows.push((name.to_owned(), handle, width));
        };
        for (category, name) in PAGES {
            let window = open_panel_window(cx, testing::SETTINGS_PX, move |_, cx| {
                cx.new(|_| {
                    let mut settings = testing::Settings::new(0);
                    settings.category = category;
                    settings
                })
            });
            open(
                name,
                window.expect("cannot open panel window").into(),
                testing::SETTINGS_PX.0,
            );
        }
        let (w, _) = ui::prefs::COLOR_DIALOG_PX;
        let dialog = open_panel_window(cx, ui::prefs::COLOR_DIALOG_PX, |_, cx| {
            cx.new(|_| ui::prefs::ColorDialog::new(0, design::Accent::DEFAULT))
        });
        open("color", dialog.expect("cannot open panel window").into(), w);
        let faces = open_panel_window(cx, FACES_PX, |_, cx| cx.new(|_| Faces));
        open(
            "faces",
            faces.expect("cannot open panel window").into(),
            FACES_PX.0,
        );
        // 仮想キーボード・部品が一通り並ぶ Testing・ダッシュボード（バーと、そこから開くクイック設定・ランチャー）。
        let keyboard = open_panel_window(cx, KEYBOARD_PX, |_, cx| {
            cx.new(|cx| ui::keyboard::KeyboardView::new(0, cx))
        });
        open(
            "keyboard",
            keyboard.expect("cannot open panel window").into(),
            KEYBOARD_PX.0,
        );
        let screen = open_panel_window(cx, testing::TESTING_PX, |_, cx| {
            cx.new(|_| testing::Testing::new(0))
        });
        open(
            "testing",
            screen.expect("cannot open panel window").into(),
            testing::TESTING_PX.0,
        );
        let summon: dashboard::Summon = |from, opened| {
            ui::PanelSpec::spawn(from, |id, _, cx| {
                opened(id, cx);
                testing::Settings::new(id)
            })
        };
        let mut bar = None;
        let window = open_panel_window(cx, BAR_PX, |window, cx| {
            let view = dashboard::spec(|_, _| {}, summon, summon, false).build(0, window, cx);
            bar = view.clone().downcast::<dashboard::Dashboard>().ok();
            cx.new(|_| ui::PanelRoot::new(view))
        });
        open(
            "dashboard",
            window.expect("cannot open panel window").into(),
            BAR_PX.0,
        );
        let bar = bar.expect("the dashboard view");
        for (id, (page, name)) in [
            (dashboard::Page::QuickSettings, "quick"),
            (dashboard::Page::Launcher, "launcher"),
        ]
        .into_iter()
        .enumerate()
        {
            bar.update(cx, |bar, cx| bar.toggle(page, cx));
            let spec = cx.global_mut::<PanelControl>().spawns.pop();
            let spec = spec.expect("the dashboard opens the page");
            let window = open_panel_window(cx, PAGE_PX, move |window, cx| {
                let view = spec.build(id + 1, window, cx);
                cx.new(|_| ui::PanelRoot::new(view))
            });
            open(
                name,
                window.expect("cannot open panel window").into(),
                PAGE_PX.0,
            );
        }
        for (name, mono) in [("lines", false), ("mono", true)] {
            let lines = open_panel_window(cx, LINES_PX, move |_, cx| cx.new(|_| Lines(mono)));
            open(
                name,
                lines.expect("cannot open panel window").into(),
                LINES_PX.0,
            );
        }
        cx.spawn(async move |cx| {
            for (state, scale, bold) in STATES {
                cx.update(|cx| {
                    // 本番ではフレームループ（`ui::sync_settings`）が全体の設定から書く。
                    let settings = &mut cx.default_global::<PanelControl>().settings;
                    (settings.text_scale, settings.bold_text) = (scale, bold);
                    design::font::set_text(scale, bold, cx);
                });
                for (name, window, width) in &windows {
                    // 読み戻しは 1 回遅れなので、間を空けて何度か描き、最後の絵を使う。
                    let mut last = None;
                    for _ in 0..6 {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let frame = cx
                            .update_window(*window, |_, window, cx| {
                                window.refresh();
                                capture(window, cx)
                            })
                            .expect("window is closed");
                        last = frame.ok().filter(|f| f.width == *width);
                    }
                    out.borrow_mut()
                        .push((state, name.clone(), last.expect("captured")));
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let shots = shots.take();
    assert_eq!(shots.len(), STATES.len() * (PAGES.len() + 9));
    for (state, name, frame) in &shots {
        save(frame, &format!("target/tmp/look-a11y-{state}-{name}.bmp"));
    }
    let frame = |state: &str, name: &str| {
        let found = shots.iter().find(|(s, n, _)| *s == state && n == name);
        &found.expect("captured").2
    };
    // インク = 画面全体の暗さの合計（白地に黒い文字）。
    let ink = |state: &str, name: &str| -> u64 {
        let pixels = frame(state, name).rgba.as_chunks::<4>().0.iter();
        pixels.map(|p| 255 - p[0] as u64).sum()
    };
    for name in ["lines", "mono"] {
        let [base, small, large, bold, both] = STATES.map(|(state, ..)| ink(state, name));
        println!("a11y: {name} ink {small} < {base} < {large}, bold {bold}, both {both}");
        assert!(small > 0, "{name}: the small text has ink");
        assert!(
            small < base && base < large,
            "{name}: bigger text, more ink"
        );
        assert!(large <= both, "{name}: bold never thins the text");
        assert!(base <= bold, "{name}: bold never thins the text");
    }
    // 太さを持つ既定の書体は、太字化ではっきり濃くなる。
    assert!(ink("base", "lines") < ink("bold", "lines"));
    assert!(ink("large", "lines") < ink("large-bold", "lines"));
    // 文字盤は絵。全域の設定では 1 画素も変わらない。
    for (state, ..) in STATES {
        assert!(
            frame(state, "faces") == frame("base", "faces"),
            "{state}: the wrist faces changed"
        );
    }
    println!("a11y: ok ({} shots)", shots.len());
}
