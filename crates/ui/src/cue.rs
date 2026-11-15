//! 部品の効果音の要求（`design::cue`）を、そのパネルのスピーカーへ渡す。種類ごとの音はここ一箇所で決める。

use design::cue::Cue;
use gpui::{App, Window};
use kernel::{PanelCommand, PanelId, Vec2, audio::Sound};

use crate::PanelControl;

/// 種類ごとの音と速さ（1 = そのまま）。
fn sound(cue: Cue) -> (&'static [u8], f32) {
    match cue {
        Cue::Hover => (include_bytes!("../assets/pointer/hover.wav"), 1.),
        Cue::Click => (include_bytes!("../assets/pointer/click.wav"), 1.),
    }
}

/// 全体の設定（タップ音・ホバー音）で鳴らすか。
fn enabled(cue: Cue, settings: &kernel::Settings) -> bool {
    match cue {
        Cue::Hover => settings.hover_sound,
        Cue::Click => settings.tap_sound,
    }
}

/// 入力を配送した直後に呼ぶ。`window` 宛ての要求を、ポインタの点から聞こえるよう `panel` の操作要求に積む。
/// 全体の設定で切っている種類は捨てる。
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

    /// 押せるカードの中のボタンは、入ったときに 1 回だけ鳴る（ポインタを動かし続けても鳴り直さない）。カードの地へ
    /// 戻れば、カードの分がまた鳴る。
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
            // 同じ種類の音は続けて鳴らさない（`Cue::min_gap`）ので、その分だけ時計を進めてから動かす。
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
        assert_eq!(hovers(cx, body), 1, "カードに入った");
        assert_eq!(hovers(cx, inner), 1, "中のボタンに入った");
        assert_eq!(
            hovers(cx, inner + point(px(2.0), px(0.0))),
            0,
            "ボタンの上で動かしても鳴り直さない"
        );
        assert_eq!(hovers(cx, inner + point(px(4.0), px(0.0))), 0);
        assert_eq!(hovers(cx, body), 1, "カードの地へ戻った");
    }
}
