//! キーボード（入力の行 `settings-keyboard` から開く詳しい画面）: 上に見本（押せて音も鳴るが、どこにも打たない）、
//! 下に種類・キーの音・その音量（鳴らす間だけ）。

use design::Theme;
use design::components;
use gpui::{Context, Div, Window, prelude::*};
use kernel::Setting;
use kernel::subsystems::keyboard::KeyboardType;

use crate::{ROW_SLIDER_PX, Settings, change};

pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let now = ui::settings(cx);
    let keys = s
        .keyboard_preview
        .get_or_init(|| ui::prefs::new_keyboard_preview(s.panel, cx));
    let preview = ui::prefs::keyboard_preview(keys, cx);
    let kind = s.select(
        theme,
        window,
        cx,
        "setting-keyboard",
        &design::icons::KEYBOARD,
        i18n::t(cx, "settings.keyboard"),
        now.keyboard,
        [
            ("setting-keyboard-full", "keyboard.full", KeyboardType::Full),
            (
                "setting-keyboard-tkl",
                "keyboard.tkl",
                KeyboardType::Tenkeyless,
            ),
            (
                "setting-keyboard-compact",
                "keyboard.compact",
                KeyboardType::Compact,
            ),
        ]
        .map(|(id, key, k)| (id, i18n::t(cx, key), k))
        .to_vec(),
        Setting::Keyboard,
    );
    let sound = components::switch_item(
        theme,
        "setting-key-sound",
        &design::icons::WAVEFORM,
        i18n::t(cx, "settings.key_sound"),
        now.key_sound,
    )
    .on_click(change(Setting::KeySound(!now.key_sound)));
    // 音量は 10% の刻み（カーネルも同じ刻みに丸める）。刻みが変わったときだけ設定を変える。
    let (volume, step) = (now.key_sound_volume, kernel::SCALE_STEP);
    let slider = components::slider(
        theme,
        "setting-key-volume-slider",
        volume,
        move |at, _, cx| {
            let to = (at / step).round() * step;
            if (to - volume).abs() > step / 2.0 {
                ui::set(Setting::KeySoundVolume(to), cx)
            }
        },
    );
    let volume = components::slider_item(
        theme,
        "setting-key-volume",
        &design::icons::SPEAKER_HIGH,
        i18n::t(cx, "settings.key_sound_volume"),
        format!("{:.0}%", volume * 100.0),
        slider.length(ROW_SLIDER_PX),
    );
    let rows = [kind, sound]
        .into_iter()
        .chain(now.key_sound.then_some(volume));
    vec![preview, components::list(theme, rows)]
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, px};
    use ui::PanelControl;

    use crate::{Category, PX, Page, Settings};

    /// キーボードの見本は、本番と同じ載せ方（影の余白の内側）の設定画面でも、もっと狭い画面でも、どの種類でも
    /// 下の行の幅と見本の箱に収まり、地がキーを四方から囲む。幅か高さのどちらかは、いっぱいまで使う。
    #[gpui::test]
    fn keyboard_preview_fits_the_settings_screen_for_every_type(cx: &mut TestAppContext) {
        use gpui::AppContext;
        use kernel::subsystems::keyboard::{Database, KeyboardType, Layout};
        let keyboard = Database::load().unwrap();
        let keyboard = keyboard.keyboard(&Layout::new("us", "")).unwrap();
        let (_, cx) = cx.add_window_view(|_, cx| {
            let keys = ui::prefs::new_keyboard_preview(0, cx);
            keys.update(cx, |view, _| {
                (view.automatic, view.loading) = (false, false);
                view.keyboard = Some(keyboard);
            });
            let settings = Settings {
                category: Category::Input,
                pages: vec![Page::Keyboard],
                ..Settings::new(0)
            };
            assert!(settings.keyboard_preview.set(keys).is_ok());
            ui::PanelRoot::new(cx.new(|_| settings).into())
        });
        for width in [PX.0, 800] {
            cx.simulate_resize(gpui::size(px(width as f32), px(PX.1 as f32)));
            for kind in KeyboardType::ALL {
                cx.update(|window, cx| {
                    cx.default_global::<PanelControl>().settings.keyboard = kind;
                    window.refresh();
                });
                cx.run_until_parked();
                let [row, place, card, keys] = [
                    "setting-keyboard",
                    "keyboard-preview",
                    "keyboard-preview-card",
                    "virtual-keyboard",
                ]
                .map(|s| cx.debug_bounds(s).unwrap_or_else(|| panic!("{s}")));
                let slack = px(0.01);
                let at = format!("{width}px {kind:?}: {card:?} in {place:?}, rows {row:?}");
                assert!(
                    card.left() >= row.left() - slack && card.right() <= row.right() + slack,
                    "{at}"
                );
                assert!(
                    card.top() >= place.top() - slack && card.bottom() <= place.bottom() + slack,
                    "{at}"
                );
                let free =
                    (row.size.width - card.size.width).min(place.size.height - card.size.height);
                assert!(free < px(1.0), "{at}");
                // 地の余白は四方で同じ。キーは縦横の比を保ち、実寸（1u = 64px）より小さい（配置は画素に丸まる）。
                let pad = keys.left() - card.left();
                assert!(pad > px(1.0), "{at}");
                for other in [
                    keys.top() - card.top(),
                    card.right() - keys.right(),
                    card.bottom() - keys.bottom(),
                ] {
                    assert!((other - pad).abs() < px(1.0), "{at}: {keys:?}");
                }
                let unit = f32::from(keys.size.width) / kind.width();
                assert!(unit < 64.0, "{at}");
                assert!(
                    (f32::from(keys.size.height) - kind.height() * unit).abs() < 1.0,
                    "{at}: {keys:?}"
                );
            }
        }
    }
}
