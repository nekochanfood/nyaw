//! 見た目の確認用: 設定画面の「外観」を、テーマとパーソナルカラーを替えながら実 GPU で描き、`target/tmp/look-*.bmp` に書き出す。
//! 塗りが選んだ色そのままで描かれていることだけ検査する（細かい見た目は書き出した絵を目で見る）。

use std::{cell::RefCell, rc::Rc, time::Duration};

use design::{Accent, Mode};
use gpui::AppContext;
use ui::{PanelControl, capture, open_panel_window};

/// 書き出す状態: 名前、テーマ、パーソナルカラー（[`Accent::PRESETS`] の名前）。
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
        let window = open_panel_window(cx, testing::SETTINGS_PX, |_, cx| {
            cx.new(|_| {
                let mut settings = testing::Settings::new(0);
                settings.category = 1;
                settings
            })
        })
        .expect("cannot open panel window");
        cx.spawn(async move |cx| {
            for (name, mode, color) in STATES {
                let accent = preset(color);
                cx.update(|cx| {
                    cx.set_global(mode);
                    cx.set_global(accent);
                    let settings = &mut cx.default_global::<PanelControl>().settings;
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
                // 読み戻しは 1 回遅れなので、間を空けて何度か描き、最後の絵を使う。
                let mut last = None;
                for _ in 0..6 {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let frame = cx
                        .update_window(window.into(), |_, window, cx| capture(window, cx))
                        .expect("window is closed");
                    last = frame.ok().filter(|f| f.width == testing::SETTINGS_PX.0);
                }
                out.borrow_mut()
                    .push((name, accent, last.expect("captured")));
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let shots = shots.take();
    assert_eq!(shots.len(), STATES.len());
    for (name, accent, frame) in shots {
        save(&frame, &format!("target/tmp/look-{name}.bmp"));
        let [r, g, b, _] = accent.color().to_be_bytes();
        let filled = frame.rgba.as_chunks::<4>().0.iter();
        let filled = filled.filter(|p| p[..3] == [r, g, b]).count();
        assert!(
            filled > 1000,
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
