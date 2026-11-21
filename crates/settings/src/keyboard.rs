//! キーボード（入力カテゴリの `settings-keyboard` から開く詳細画面）: 上部にプレビュー（入力操作可能・打鍵音あり、テキスト出力なし）、
//! 下部に種類・サイズ・キー間隔・打鍵音・音量（打鍵音有効時のみ表示）。

use design::Theme;
use design::components;
use gpui::{Context, Div, Window, prelude::*};
use kernel::subsystems::keyboard::KeyboardType;
use kernel::{
    KEY_GAP_MAX, KEY_GAP_MIN, KEYBOARD_SCALE_MAX, KEYBOARD_SCALE_MIN, SCALE_STEP, Setting,
};

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
    // 倍率スライダー。`SCALE_STEP` にスナップし、ステップ境界を越えた場合のみ設定を更新する。
    let scale_item = |id: &'static str,
                      slider_id: &'static str,
                      glyph: &'static design::Icon,
                      key: &str,
                      (min, max, now): (f32, f32, f32),
                      set: fn(f32) -> Setting| {
        components::slider_item(
            theme,
            id,
            glyph,
            i18n::t(cx, key),
            format!("{:.0}%", now * 100.0),
            components::slider(
                theme,
                slider_id,
                (now - min) / (max - min),
                move |at, _, cx| {
                    let to = kernel::config::clamp_scale(min + at * (max - min), min, max);
                    if (to - now).abs() > SCALE_STEP / 2.0 {
                        ui::set(set(to), cx)
                    }
                },
            )
            .length(ROW_SLIDER_PX),
        )
    };
    // サイズは VR 上のキーボードに反映される（プレビューは実寸ではないため変化なし）。間隔はプレビューに即時反映。
    let size = scale_item(
        "setting-keyboard-size",
        "setting-keyboard-size-slider",
        &design::icons::ARROWS_OUT,
        "settings.keyboard_size",
        (KEYBOARD_SCALE_MIN, KEYBOARD_SCALE_MAX, now.keyboard_scale),
        Setting::KeyboardScale,
    );
    let gap = scale_item(
        "setting-key-gap",
        "setting-key-gap-slider",
        &design::icons::SQUARES_FOUR,
        "settings.key_gap",
        (KEY_GAP_MIN, KEY_GAP_MAX, now.key_gap),
        Setting::KeyGap,
    );
    let sound = components::switch_item(
        theme,
        "setting-key-sound",
        &design::icons::WAVEFORM,
        i18n::t(cx, "settings.key_sound"),
        now.key_sound,
    )
    .on_click(change(Setting::KeySound(!now.key_sound)));
    // 音量は 10% 刻み（カーネル側も同一ステップで丸める）。ステップ境界を越えた場合のみ設定を更新する。
    let (volume, step) = (now.key_sound_volume, SCALE_STEP);
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
    let rows = [kind, size, gap, sound]
        .into_iter()
        .chain(now.key_sound.then_some(volume));
    vec![preview, components::list(theme, rows)]
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, px};
    use ui::PanelControl;

    use crate::{Category, PX, Page, Settings};

    /// キーボードプレビューは、本番同一レイアウト（影余白内）の設定画面でも狭い画面でも全種類において
    /// 下部行の幅とプレビュー領域に収まり、背景がキー領域を四方から囲む。幅または高さの一方は最大まで使用する。
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
                // 背景余白は四方均一。キーはアスペクト比を維持し、実寸（1u = 64px）より縮小される（配置はピクセルに丸められる）。
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
