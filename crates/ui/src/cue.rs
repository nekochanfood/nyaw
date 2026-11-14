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
}
