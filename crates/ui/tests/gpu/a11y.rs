//! アクセシビリティの見た目検証: 文字サイズ倍率と太字化を切り替えながら、設定画面の各カテゴリ・色作成ダイアログ・wrist の文字盤
//! プレビューを実 GPU で描画し、`target/tmp/look-a11y-*.bmp` に書き出す（詳細な見た目は画像を目視確認する）。検査対象は、倍率・太字化で
//! 文字のインクが増加すること、等幅書体も崩れないこと、適用対象外の文字（`.text_exact()` の文字盤）が 1 ピクセルも変化しないこと。

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use settings::{Category, Page, Settings};
use ui::{PanelControl, capture, open_panel_window};

use crate::look::{FACES_PX, Faces, save};

/// 白地に黒い文字（既定の書体と、ウェイトが 1 種類のみの等幅書体）。インク量を比較する。
struct Lines(bool);

const LINES_PX: (u32, u32) = (900, 120);
/// 仮想キーボード（フルサイズが収まるサイズ）・ダッシュボードのバー・バーにはめ込む画面のウィンドウ。
const KEYBOARD_PX: (u32, u32) = (1700, 760);
const BAR_PX: (u32, u32) = (1024, 118);
const PAGE_PX: (u32, u32) = (1024, 768);
const SETUP_PX: (u32, u32) = (1152, 864);

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

/// 書き出す状態: 名前、文字サイズ倍率、太字化。
const STATES: [(&str, f32, bool); 5] = [
    ("base", 1.0, false),
    ("small", kernel::TEXT_SCALE_MIN, false),
    ("large", kernel::TEXT_SCALE_MAX, false),
    ("bold", 1.0, true),
    ("large-bold", kernel::TEXT_SCALE_MAX, true),
];

/// 設定画面のカテゴリ・その上に表示する詳細画面・書き出す名前。
const PAGES: [(Category, Option<Page>, &str); 8] = [
    (Category::General, None, "general"),
    (Category::General, Some(Page::About), "about"),
    (Category::Appearance, None, "appearance"),
    (Category::Appearance, Some(Page::Watch), "watch"),
    (Category::Input, None, "input"),
    (Category::Input, Some(Page::Pointer), "pointer"),
    (Category::Display, None, "display"),
    (Category::Accessibility, None, "accessibility"),
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
        for (category, page, name) in PAGES {
            let window = open_panel_window(cx, settings::PX, move |_, cx| {
                cx.new(|_| {
                    let mut settings = Settings::new(0);
                    settings.category = category;
                    settings.pages.extend(page);
                    settings
                })
            });
            open(
                name,
                window.expect("cannot open panel window").into(),
                settings::PX.0,
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
        // 仮想キーボード・各部品が一通り並ぶ Testing・ダッシュボード（バーと、バーから開くクイック設定・ランチャー）。
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
                Settings::new(id)
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
        // Setup wizard, one window per step.
        for (i, step) in setup::STEPS.into_iter().enumerate() {
            let page = open_panel_window(cx, SETUP_PX, move |_, cx| {
                cx.new(|_| {
                    let mut wizard = setup::SetupWizard::new(0);
                    for _ in 0..i {
                        wizard.wizard.next();
                    }
                    wizard
                })
            });
            let name = format!("setup-{i}-{step:?}").to_lowercase();
            open(
                &name,
                page.expect("cannot open panel window").into(),
                SETUP_PX.0,
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
                    // 本番ではフレームループ（`ui::sync_settings`）がグローバル設定から書き込む。
                    let settings = &mut cx.default_global::<PanelControl>().settings;
                    (settings.text_scale, settings.bold_text) = (scale, bold);
                    // ウィザードの SteamVR 手順は OpenVR 使用時のみ表示されるため、OpenVR として描画する。
                    settings.auto_launch = Some(false);
                    design::font::set_text(scale, bold, cx);
                });
                for (name, window, width) in &windows {
                    // 読み戻しは 1 フレーム遅れのため、間隔を空けて複数回描画し、最後の画像を使用する。
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
    assert_eq!(
        shots.len(),
        STATES.len() * (PAGES.len() + 9 + setup::STEPS.len())
    );
    for (state, name, frame) in &shots {
        save(frame, &format!("target/tmp/look-a11y-{state}-{name}.bmp"));
    }
    let frame = |state: &str, name: &str| {
        let found = shots.iter().find(|(s, n, _)| *s == state && n == name);
        &found.expect("captured").2
    };
    // インク = 画面全体の暗さの合計値（白地に黒い文字）。
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
    // 複数ウェイトを持つ既定の書体は、太字化で明確に濃くなる。
    assert!(ink("base", "lines") < ink("bold", "lines"));
    assert!(ink("large", "lines") < ink("large-bold", "lines"));
    // 文字盤は画像。グローバル設定では 1 ピクセルも変化しない。
    for (state, ..) in STATES {
        assert!(
            frame(state, "faces") == frame("base", "faces"),
            "{state}: the wrist faces changed"
        );
    }
    println!("a11y: ok ({} shots)", shots.len());
}
