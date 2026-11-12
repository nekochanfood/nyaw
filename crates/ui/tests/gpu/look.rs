//! 見た目の確認用: 設定画面の「外観」「時計」・色を作るダイアログ・wrist の文字盤の見本・セットアップウィザードの各画面を、テーマとパーソナルカラーを替えながら実 GPU で描き、
//! `target/tmp/look-*.bmp` に書き出す。塗りが選んだ色そのままで描かれていることだけ検査する（細かい見た目は絵を目で見る）。

use std::{cell::RefCell, rc::Rc, time::Duration};

use design::{Accent, Mode};
use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px, rgba};
use ui::{PanelControl, capture, open_panel_window};

/// wrist の文字盤の見本を、地の上に並べた画面（種類ごとに 1 行、デザインを横に）。
struct Faces;

const FACES_PX: (u32, u32) = (900, 900);
/// セットアップウィザードのパネルの大きさ。
const SETUP_PX: (u32, u32) = (800, 864);

impl Render for Faces {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = design::Theme::current(cx);
        let rows = kernel::WristFace::ALL.iter().map(|face| {
            let designs = face.designs().iter();
            let previews = designs.map(|&design| wrist::preview(design, 268.0, &theme, cx));
            div().flex().gap(px(24.0)).children(previews)
        });
        div()
            .size_full()
            .bg(rgba(theme.background))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.0))
            .children(rows.collect::<Vec<_>>())
    }
}

/// 書き出す状態: 名前、テーマ、パーソナルカラー（[`Accent::PRESETS`] の名前）。ダークは英語、ライトは日本語で描く。
const STATES: [(&str, Mode, &str); 4] = [
    ("dark-blue", Mode::Dark, "blue"),
    ("dark-yellow", Mode::Dark, "yellow"),
    ("light-yellow", Mode::Light, "yellow"),
    ("light-pink", Mode::Light, "pink"),
];

fn preset(name: &str) -> Accent {
    let found = Accent::PRESETS.iter().find(|(n, _)| *n == name);
    found.expect("known preset").1
}

pub fn main() {
    if !crate::common::should_run("look") {
        return;
    }
    let shots = Rc::new(RefCell::new(Vec::new()));
    let out = shots.clone();
    ui::application().run(move |cx| {
        design::font::install(cx);
        design::corner::install();
        // 本番では wrist が置く、文字盤の見本の描き方。
        cx.set_global(ui::prefs::FacePreview(wrist::preview));
        // 設定画面のカテゴリ `category` を開いた窓。
        let settings = |category: usize, cx: &mut gpui::App| {
            open_panel_window(cx, testing::SETTINGS_PX, move |_, cx| {
                cx.new(|_| {
                    let mut settings = testing::Settings::new(0);
                    settings.category = category;
                    settings
                })
            })
            .expect("cannot open panel window")
        };
        let (appearance, watch) = (settings(1, cx), settings(2, cx));
        // 色を作るダイアログ（作りかけの色は、今のパーソナルカラーと違う緑）。
        let dialog = open_panel_window(cx, ui::prefs::COLOR_DIALOG_PX, |_, cx| {
            cx.new(|_| ui::prefs::ColorDialog::new(0, preset("green")))
        })
        .expect("cannot open panel window");
        let faces = open_panel_window(cx, FACES_PX, |_, cx| cx.new(|_| Faces))
            .expect("cannot open panel window");
        // 描く窓: 書き出す名前の前置き、窓、塗りの色で描かれているはずの画素の下限。
        let mut windows: Vec<(String, gpui::AnyWindowHandle, u32, usize)> = vec![
            (
                String::new(),
                appearance.into(),
                testing::SETTINGS_PX.0,
                1000,
            ),
            ("watch-".into(), watch.into(), testing::SETTINGS_PX.0, 100),
            (
                "color-".into(),
                dialog.into(),
                ui::prefs::COLOR_DIALOG_PX.0,
                1000,
            ),
            ("faces-".into(), faces.into(), FACES_PX.0, 100),
        ];
        // セットアップウィザードの画面を、手順ごとに 1 枚ずつ。最後に、一枚目で言語の選択肢を開いたもの。
        let pages = (0..setup::STEPS.len()).map(|i| (i, false));
        for (i, languages) in pages.chain([(0, true)]) {
            let page = open_panel_window(cx, SETUP_PX, move |_, cx| {
                cx.new(|_| {
                    let mut wizard = setup::SetupWizard::new(0);
                    for _ in 0..i {
                        wizard.wizard.next();
                    }
                    wizard.language_open = languages;
                    wizard
                })
            })
            .expect("cannot open panel window");
            let step = format!("{:?}", setup::STEPS[i]).to_lowercase();
            let open = if languages { "-languages" } else { "" };
            let name = format!("setup-{i}-{step}{open}-");
            // 開いた選択肢は「はじめる」（塗りの色）に重なるので、塗りの色が見えなくてよい。
            let min = if languages { 0 } else { 100 };
            windows.push((name, page.into(), SETUP_PX.0, min));
        }

        cx.spawn(async move |cx| {
            for (name, mode, color) in STATES {
                let accent = preset(color);
                cx.update(|cx| {
                    cx.set_global(mode);
                    cx.set_global(accent);
                    let japanese = mode == Mode::Light;
                    cx.set_global(if japanese {
                        i18n::Lang::Ja
                    } else {
                        i18n::Lang::En
                    });
                    let settings = &mut cx.default_global::<PanelControl>().settings;
                    settings.language = if japanese {
                        kernel::Language::Ja
                    } else {
                        kernel::Language::En
                    };
                    settings.theme = match mode {
                        Mode::Dark => kernel::ThemeMode::Dark,
                        Mode::Light => kernel::ThemeMode::Light,
                    };
                    settings.personal_color = Some(kernel::PersonalColor {
                        lightness: accent.lightness,
                        chroma: accent.chroma,
                        hue: accent.hue,
                    });
                });
                for (prefix, window, width, min) in windows.clone() {
                    // 読み戻しは 1 回遅れなので、間を空けて何度か描き、最後の絵を使う。
                    let mut last = None;
                    for _ in 0..6 {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let frame = cx
                            .update_window(window, |_, window, cx| capture(window, cx))
                            .expect("window is closed");
                        last = frame.ok().filter(|f| f.width == width);
                    }
                    let name = format!("{prefix}{name}");
                    out.borrow_mut()
                        .push((name, accent, min, last.expect("captured")));
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let shots = shots.take();
    assert_eq!(shots.len(), (5 + setup::STEPS.len()) * STATES.len());
    for (name, accent, min, frame) in shots {
        save(&frame, &format!("target/tmp/look-{name}.bmp"));
        let [r, g, b, _] = accent.color().to_be_bytes();
        let filled = frame.rgba.as_chunks::<4>().0.iter();
        let filled = filled.filter(|p| p[..3] == [r, g, b]).count();
        assert!(
            filled >= min,
            "{name}: only {filled} px of the picked color"
        );
        println!("look: {name} ok ({filled} px of the personal color)");
    }
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
