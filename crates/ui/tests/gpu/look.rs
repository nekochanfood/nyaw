//! 見た目の確認用: 設定画面の「外観」「時計」「キーボード」・色作成ダイアログ・ダイアログ・wrist の文字盤プレビュー・セットアップウィザードの各画面を、テーマとパーソナルカラーを切り替えながら実 GPU で描画し、
//! `target/tmp/look-*.bmp` に書き出す。塗りが選択した色のまま描画されていることのみ検査する（詳細な見た目は画像を目視確認する）。

use std::{cell::RefCell, rc::Rc, time::Duration};

use design::{Accent, Mode};
use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px, rgba};
use settings::{Category, Page, Settings};
use ui::{PanelControl, capture, open_panel_window};

/// wrist の文字盤プレビューを背景上に並べた画面（種類ごとに 1 行、デザインを横に配置）。
pub struct Faces;

pub const FACES_PX: (u32, u32) = (900, 900);
/// セットアップウィザードのパネルサイズ。
const SETUP_PX: (u32, u32) = (1152, 864);
/// ダイアログのサイズ。高さは 1 行のタイトルと 3 行の概要の分（`components::dialog_height` で算出する値）。
const DIALOG_PX: (u32, u32) = (design::components::DIALOG_WIDTH, 376);

impl Render for Faces {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = design::Theme::current(cx);
        let rows = kernel::WristFace::ALL.iter().map(|face| {
            let designs = face.designs().iter();
            let previews = designs.map(|&design| wrist::preview(design, 268.0, cx));
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

/// 書き出す状態: 名前、テーマ、パーソナルカラー（[`Accent::PRESETS`] の名前）。ダークは英語、ライトは日本語で描画する。
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
        // 本番では wrist が設定する、文字盤プレビューの描画方法。
        cx.set_global(ui::prefs::FacePreview(wrist::preview));
        // 設定画面のカテゴリ `category`（`page` があればその上に詳細画面を表示して）を開いたウィンドウ。
        let screen = |category: Category, page: Option<Page>, cx: &mut gpui::App| {
            open_panel_window(cx, settings::PX, move |_, cx| {
                let view = cx.new(|_| {
                    let mut settings = Settings::new(0);
                    settings.category = category;
                    settings.pages.extend(page);
                    settings
                });
                // 本番と同じ配置方法（影の余白の内側にパネルが収まり、コンテンツ幅も本番と同一になる）。
                cx.new(|_| ui::PanelRoot::new(view.into()))
            })
            .expect("cannot open panel window")
        };
        let (appearance, watch, pointer, keyboard) = (
            screen(Category::Appearance, None, cx),
            screen(Category::Appearance, Some(Page::Watch), cx),
            screen(Category::Input, Some(Page::Pointer), cx),
            screen(Category::Input, Some(Page::Keyboard), cx),
        );
        // 色作成ダイアログ（編集中の色は、現在のパーソナルカラーと異なる緑）。
        let dialog = open_panel_window(cx, ui::prefs::COLOR_DIALOG_PX, |_, cx| {
            cx.new(|_| ui::prefs::ColorDialog::new(0, preset("green")))
        })
        .expect("cannot open panel window");
        // ダイアログ（概要が 3 行を超えてスクロールするもの）。本番と同様に影の余白の内側に配置する。
        let required = open_panel_window(cx, DIALOG_PX, |_, cx| {
            let view = cx.new(|_| {
                testing::Dialog::new(
                    0,
                    gpui::WeakEntity::new_invalid(),
                    &design::icons::WARNING,
                    "dialog.required_title",
                    "dialog.required_body",
                )
            });
            cx.new(|_| ui::PanelRoot::new(view.into()))
        })
        .expect("cannot open panel window");
        let faces = open_panel_window(cx, FACES_PX, |_, cx| cx.new(|_| Faces))
            .expect("cannot open panel window");
        // 描画するウィンドウ: 書き出す名前のプレフィックス、ウィンドウ、塗りの色で描画されているべきピクセル数の下限。
        let mut windows: Vec<(String, gpui::AnyWindowHandle, u32, usize)> = vec![
            (String::new(), appearance.into(), settings::PX.0, 1000),
            ("watch-".into(), watch.into(), settings::PX.0, 100),
            // 塗りはスライダーの充填部分とスイッチ。
            ("pointer-".into(), pointer.into(), settings::PX.0, 100),
            // キーボードのプレビューはレイアウトをバックグラウンドで読み込んでから表示する（前のウィンドウの描画中に読み込み完了する）。塗りはスイッチとスライダー。
            ("keyboard-".into(), keyboard.into(), settings::PX.0, 100),
            (
                "color-".into(),
                dialog.into(),
                ui::prefs::COLOR_DIALOG_PX.0,
                1000,
            ),
            // 塗りは OK ボタンとアイコン。
            ("dialog-".into(), required.into(), DIALOG_PX.0, 100),
            ("faces-".into(), faces.into(), FACES_PX.0, 100),
        ];
        // セットアップウィザードの画面を手順ごとに 1 枚ずつ描画する。最後に、1 枚目で言語の選択肢を展開したものを追加する。
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
            // 展開した選択肢は「はじめる」（塗りの色）に重なるため、塗りの色が見えなくてよい。
            let min = if languages { 0 } else { 100 };
            windows.push((name, page.into(), SETUP_PX.0, min));
        }

        cx.spawn(async move |cx| {
            for (i, (name, mode, color)) in STATES.into_iter().enumerate() {
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
                    // キーボードのプレビューは状態ごとに種類も切り替えて全種類を描画する。
                    settings.keyboard = kernel::subsystems::keyboard::KeyboardType::ALL[i % 3];
                    // ウィザードの SteamVR 手順は OpenVR 使用時のみ表示されるため、OpenVR として描画する。
                    settings.auto_launch = Some(false);
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
                    // 読み戻しは 1 フレーム遅れのため、間隔を空けて複数回描画し、最後の画像を使用する。
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
    assert_eq!(shots.len(), (8 + setup::STEPS.len()) * STATES.len());
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

pub fn save(frame: &ui::Frame, path: &str) {
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
