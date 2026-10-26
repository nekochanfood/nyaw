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

/// 入力を配送した直後に呼ぶ。`window` 宛ての要求を、ポインタの点から聞こえるよう `panel` の操作要求に積む。
pub(crate) fn flush(panel: PanelId, window: &Window, cx: &mut App) {
    let cues = design::cue::take(window, cx);
    let commands = &mut cx.default_global::<PanelControl>().panels.entry(panel).or_default().commands;
    commands.extend(cues.into_iter().map(|(cue, at)| {
        let (wav, pitch) = sound(cue);
        PanelCommand::PlayAt {
            sound: Sound::Wav(wav),
            at: Vec2::new(f32::from(at.x), f32::from(at.y)),
            pitch,
        }
    }));
}
