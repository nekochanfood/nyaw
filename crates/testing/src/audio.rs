use super::*;
use kernel::audio::{Layout, Sound, Spatializer};

/// パネルのスピーカーから鳴る音を試す画面。パネルを動かす・頭を回すと、音が聞こえる向きが変わる。
/// `left` / `right` は片側の 2ch、`centre` は左右が同じ（自動ではモノラル）、Loop は 2ch のループで、重ねて鳴らせる。
pub struct AudioTest {
    panel: PanelId,
}

impl AudioTest {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
    }
}

const VOICES: [(&str, &str, &[u8]); 3] = [
    ("audio-left", "audio.left", include_bytes!("../assets/audio/left.wav")),
    ("audio-centre", "audio.centre", include_bytes!("../assets/audio/centre.wav")),
    ("audio-right", "audio.right", include_bytes!("../assets/audio/right.wav")),
];

const LOOPS: [(&str, &[u8]); 4] = [
    ("audio-loop-1", include_bytes!("../assets/audio/Loop1.wav")),
    ("audio-loop-2", include_bytes!("../assets/audio/Loop2.wav")),
    ("audio-loop-3", include_bytes!("../assets/audio/Loop3.wav")),
    ("audio-loop-4", include_bytes!("../assets/audio/Loop4.wav")),
];

const LAYOUTS: [(&str, &str, Layout); 3] = [
    ("audio-layout-auto", "audio.layout_auto", Layout::Auto),
    ("audio-layout-mono", "audio.layout_mono", Layout::Mono),
    ("audio-layout-stereo", "audio.layout_stereo", Layout::Stereo),
];

fn spatializer_text(s: Spatializer, cx: &App) -> SharedString {
    i18n::t(
        cx,
        match s {
            Spatializer::Starting => "audio.spatializer_starting",
            Spatializer::SteamAudio => "audio.spatializer_steam",
            Spatializer::Panning => "audio.spatializer_panning",
            Spatializer::Unavailable => "audio.spatializer_unavailable",
        },
    )
}

impl Render for AudioTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let st = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.audio)
            .unwrap_or_default();
        let panel = self.panel;
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(div().child(i18n::t(cx, "audio.instructions")))
            .child(row().children(VOICES.map(|(id, label, wav)| {
                themed_button(&theme, id, i18n::t(cx, label), false)
                    .on_click(command(panel, PanelCommand::Play(Sound::Wav(wav), false)))
            })))
            .child(
                row()
                    .children(LOOPS.into_iter().enumerate().map(|(i, (id, wav))| {
                        themed_button(
                            &theme,
                            id,
                            i18n::tf(cx, "audio.loop", &[("n", &(i + 1))]),
                            false,
                        )
                        .on_click(command(panel, PanelCommand::Play(Sound::Wav(wav), true)))
                    }))
                    .child(
                        themed_button(&theme, "audio-stop", i18n::t(cx, "audio.stop"), false)
                            .on_click(command(panel, PanelCommand::StopAudio)),
                    ),
            )
            .child(
                row()
                    .child(
                        themed_button(&theme, "audio-spatial", i18n::t(cx, "audio.spatial"), st.spatial)
                            .on_click(command(panel, PanelCommand::Spatial(!st.spatial))),
                    )
                    .children(LAYOUTS.map(|(id, label, layout)| {
                        themed_button(&theme, id, i18n::t(cx, label), st.layout == layout)
                            .on_click(command(panel, PanelCommand::SpeakerLayout(layout)))
                    })),
            )
            .child(
                div().debug_selector(|| "audio-status".into()).child(i18n::tf(
                    cx,
                    "audio.status",
                    &[
                        ("count", &st.playing),
                        ("spatializer", &spatializer_text(st.spatializer, cx)),
                    ],
                )),
            )
    }
}
