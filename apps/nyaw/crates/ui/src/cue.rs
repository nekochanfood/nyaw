//! 部品の効果音要求（`design::cue`）をパネルのスピーカーへ転送する。種類ごとの音はここで一元管理する。

use design::cue::Cue;
use gpui::{App, Window};
use kernel::{PanelCommand, PanelId, Vec2, audio::Sound};

use crate::PanelControl;

/// 効果音の種類に対応する音声データと再生速度（1.0 = 等速）。
fn sound(cue: Cue) -> (&'static [u8], f32) {
    match cue {
        Cue::Hover => (include_bytes!("../assets/pointer/hover.wav"), 1.),
        Cue::Click => (include_bytes!("../assets/pointer/click.wav"), 1.),
    }
}

/// 全体設定（タップ音・ホバー音）に基づく有効判定。
fn enabled(cue: Cue, settings: &kernel::Settings) -> bool {
    match cue {
        Cue::Hover => settings.hover_sound,
        Cue::Click => settings.tap_sound,
    }
}

/// 入力配送直後に呼び出す。`window` 宛ての効果音要求を、ポインタ位置を音源として `panel` の操作要求にキューイングする。
/// 全体設定で無効化されている種類は破棄する。
pub(crate) fn flush(panel: PanelId, window: &Window, cx: &mut App) {
    let settings = crate::settings(cx);
    let cues = design::cue::take(window, cx);
    let cues = cues.into_iter().filter(|(cue, _)| enabled(*cue, &settings));
    let commands = &mut cx
        .default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default()
        .commands;
    commands.extend(cues.map(|(cue, at)| {
        let (wav, pitch) = sound(cue);
        PanelCommand::PlayAt {
            sound: Sound::Wav(wav),
            at: Vec2::new(f32::from(at.x), f32::from(at.y)),
            pitch,
            volume: 1.,
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cue_follows_its_own_setting() {
        let on = kernel::Settings::default();
        assert!(enabled(Cue::Hover, &on) && enabled(Cue::Click, &on));
        let tap_off = kernel::Settings {
            tap_sound: false,
            ..on
        };
        assert!(enabled(Cue::Hover, &tap_off) && !enabled(Cue::Click, &tap_off));
        let hover_off = kernel::Settings {
            hover_sound: false,
            ..on
        };
        assert!(!enabled(Cue::Hover, &hover_off) && enabled(Cue::Click, &hover_off));
    }

    /// 押下可能なカード内のボタンは、ポインタ進入時に 1 回だけ鳴る（ポインタの移動を続けても再発音しない）。
    /// カードの背景へ戻ると、カードのホバー音が再度鳴る。
    #[gpui::test]
    fn a_button_inside_a_card_sounds_once_per_entry(cx: &mut gpui::TestAppContext) {
        use design::components::{self, Variant};
        use gpui::{MouseMoveEvent, PlatformInput, div, point, prelude::*, px};
        struct Nested;
        impl Render for Nested {
            fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
                let theme = design::Theme::current(cx);
                div().size_full().child(
                    components::tile(&theme, "card")
                        .w(px(320.0))
                        .h(px(240.0))
                        .child(components::button(&theme, "inner", "OK", Variant::Default)),
                )
            }
        }
        let (_view, cx) = cx.add_window_view(|_, _| Nested);
        cx.run_until_parked();
        let card = cx.debug_bounds("card").unwrap();
        let inner = cx.debug_bounds("inner").unwrap().center();
        let hovers = |cx: &mut gpui::VisualTestContext, at: gpui::Point<gpui::Pixels>| {
            // 同一種類の音は連続再生しない（`Cue::min_gap`）ため、その間隔分だけ時計を進めてから操作する。
            design::drive::advance(std::time::Duration::from_millis(100));
            cx.update(|window, cx| {
                let input = PlatformInput::MouseMove(MouseMoveEvent {
                    position: at,
                    pressed_button: None,
                    modifiers: Default::default(),
                });
                crate::dispatch_input(0, kernel::vr::Hand::Right, input, window, cx);
                let state = cx
                    .default_global::<PanelControl>()
                    .panels
                    .entry(0)
                    .or_default();
                std::mem::take(&mut state.commands).len()
            })
        };
        let body = card.bottom_right() - point(px(20.0), px(20.0));
        assert_eq!(hovers(cx, body), 1, "カードへ進入");
        assert_eq!(hovers(cx, inner), 1, "内部ボタンへ進入");
        assert_eq!(
            hovers(cx, inner + point(px(2.0), px(0.0))),
            0,
            "ボタン上の移動では再発音しない"
        );
        assert_eq!(hovers(cx, inner + point(px(4.0), px(0.0))), 0);
        assert_eq!(hovers(cx, body), 1, "カード背景へ復帰");
    }
}
