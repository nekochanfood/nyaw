//! パネルのスピーカー。パネルの中で鳴らす音は、そのパネルの置き場所・向き・幅のスピーカーから聞こえる。
//! 立体化は既定で有効で、`PanelCommand::Spatial(false)` で外す（オプトアウト）。

use super::*;
use audio::{Layout, Sound, Spatializer};

/// パネルのスピーカーの設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SpeakerConfig {
    pub spatial: bool,
    pub layout: Layout,
}

impl Default for SpeakerConfig {
    fn default() -> Self {
        Self {
            spatial: true,
            layout: Layout::Auto,
        }
    }
}

/// パネルのスピーカーの状態。ビューの表示用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStatus {
    /// このパネルで鳴っている（読み込み中を含む）音の数。
    pub playing: usize,
    pub spatial: bool,
    pub layout: Layout,
    /// 立体化の手段（全パネル共通）。
    pub spatializer: Spatializer,
}

impl Default for AudioStatus {
    fn default() -> Self {
        let c = SpeakerConfig::default();
        Self {
            playing: 0,
            spatial: c.spatial,
            layout: c.layout,
            spatializer: Spatializer::default(),
        }
    }
}

/// 点の音（[`PanelCommand::PlayAt`]）に使うスピーカーの数（パネルごと）。順に使い回すので、続けて鳴らしても
/// 前の音の残りは元の点から聞こえる。
// ponytail: 8 個を超えて同時に鳴らすと、いちばん古い音が新しい点へ移る。足りなければ増やす。
const POINT_SPEAKERS: u64 = 8;

/// 点の音のスピーカー。パネルのスピーカー（`panel as u64`）とは重ならない。
fn point_speaker(panel: PanelId, slot: u64) -> audio::SpeakerId {
    1 << 32 | (panel as u64) << 8 | slot
}

impl<B: VrBackend> Kernel<B> {
    fn speaker(&self, panel: PanelId) -> audio::Speaker {
        let p = self.overlay.panel(panel);
        let c = self.speakers.get(&panel).copied().unwrap_or_default();
        audio::Speaker {
            position: p.pose.position,
            rotation: p.pose.orientation,
            width: p.size.x,
            spatial: c.spatial,
            layout: c.layout,
            volume: 1.0,
        }
    }

    pub(super) fn play_audio(&mut self, panel: PanelId, sound: Sound, looping: bool) {
        if self.overlay.panel(panel).closed {
            return;
        }
        self.speakers.entry(panel).or_default();
        // 音が届くより先に置き場所を伝える（同じチャンネルなので順に届く）。
        self.audio
            .set_listener(self.hmd().position, self.hmd().orientation);
        self.audio.set_speaker(panel as u64, self.speaker(panel));
        self.audio.play_sound(panel as u64, sound, looping);
    }

    /// パネルの中の点から鳴らす。音源はその点の 1 点（左右を混ぜる）で、鳴らした時の置き場所に留まる（短い音のため）。
    pub(super) fn play_audio_at(&mut self, panel: PanelId, sound: Sound, at: Vec2, pitch: f32) {
        if self.overlay.panel(panel).closed {
            return;
        }
        let config = *self.speakers.entry(panel).or_default();
        let slot = self.point_slots.entry(panel).or_default();
        let id = point_speaker(panel, *slot);
        *slot = (*slot + 1) % POINT_SPEAKERS;
        let p = self.overlay.panel(panel);
        let speaker = audio::Speaker {
            position: p.pixel_to_world(at),
            rotation: p.pose.orientation,
            width: 0.0,
            spatial: config.spatial,
            layout: Layout::Mono,
            volume: 1.0,
        };
        self.audio
            .set_listener(self.hmd().position, self.hmd().orientation);
        self.audio.set_speaker(id, speaker);
        self.audio.play_sound_pitched(id, sound, false, pitch);
    }

    /// パネルのスピーカーと点の音を止める。
    pub(super) fn stop_audio(&self, panel: PanelId) {
        self.audio.stop_speaker(panel as u64);
        for slot in 0..POINT_SPEAKERS {
            self.audio.stop_speaker(point_speaker(panel, slot));
        }
    }

    pub(super) fn configure_speaker(&mut self, panel: PanelId, f: impl FnOnce(&mut SpeakerConfig)) {
        f(self.speakers.entry(panel).or_default());
        self.audio.set_speaker(panel as u64, self.speaker(panel));
    }

    /// 毎 tick、鳴っているスピーカーをパネルに合わせて動かし、閉じたパネルの音を止める。
    /// 鳴っていなければ何も送らない（テスト用のカーネルは出力を回さないので、命令を溜めない）。
    pub(super) fn update_audio(&mut self) {
        self.audio.poll();
        let closed: Vec<PanelId> = self
            .speakers
            .keys()
            .copied()
            .filter(|&p| self.overlay.panel(p).closed)
            .collect();
        for p in closed {
            self.speakers.remove(&p);
            self.audio.remove_speaker(p as u64);
            if self.point_slots.remove(&p).is_some() {
                for slot in 0..POINT_SPEAKERS {
                    self.audio.remove_speaker(point_speaker(p, slot));
                }
            }
        }
        let playing: Vec<PanelId> = self
            .speakers
            .keys()
            .copied()
            .filter(|&p| self.audio.playing(p as u64) > 0)
            .collect();
        if playing.is_empty() {
            return;
        }
        self.audio
            .set_listener(self.hmd().position, self.hmd().orientation);
        for p in playing {
            self.audio.set_speaker(p as u64, self.speaker(p));
        }
    }

    pub(super) fn audio_status(&self, panel: PanelId) -> AudioStatus {
        let c = self.speakers.get(&panel).copied().unwrap_or_default();
        let points: usize = (0..POINT_SPEAKERS)
            .map(|slot| self.audio.playing(point_speaker(panel, slot)))
            .sum();
        AudioStatus {
            playing: self.audio.playing(panel as u64) + points,
            spatial: c.spatial,
            layout: c.layout,
            spatializer: self.audio.spatializer(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::mock::MockBackend;

    fn kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k
            .add_panel(
                "t",
                Pose::new(vr::Vec3::new(2.0, 0.0, 0.0), vr::Quat::IDENTITY),
                Vec2::ONE,
                (4, 4),
            )
            .unwrap();
        (k, p)
    }

    fn tone() -> Sound {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = std::io::Cursor::new(Vec::new());
        let mut w = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for i in 0..48_000 {
            w.write_sample(if i % 2 == 0 { 8000i16 } else { -8000 })
                .unwrap();
        }
        w.finalize().unwrap();
        Sound::Wav(bytes.into_inner().leak())
    }

    /// 読み込み（別スレッド）が終わり、鳴り始めるまで描く。左右の二乗和を返す。
    fn render_until_sounding(k: &mut Kernel<MockBackend>) -> (f32, f32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut out = vec![0.0; audio::BLOCK * 2];
            k.audio.render(&mut out);
            let e = out
                .chunks_exact(2)
                .fold((0.0, 0.0), |(l, r), s| (l + s[0] * s[0], r + s[1] * s[1]));
            if e.0 + e.1 > 0.0 || std::time::Instant::now() > deadline {
                return e;
            }
            std::thread::yield_now();
        }
    }

    /// 読み込み中の音も含めて、鳴っている音が無くなるまで回す。
    fn settle(k: &mut Kernel<MockBackend>, panel: PanelId) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while k.status(panel).audio.playing > 0 && std::time::Instant::now() < deadline {
            k.audio.render(&mut [0.0; audio::BLOCK * 2]);
            k.tick();
            std::thread::yield_now();
        }
    }

    #[test]
    fn panel_sound_is_spatial_by_default_and_comes_from_the_panel() {
        let (mut k, p) = kernel();
        let status = k.status(p).audio;
        assert_eq!(
            (status.playing, status.spatial, status.layout),
            (0, true, Layout::Auto)
        );
        assert_eq!(
            status.spatializer,
            Spatializer::Panning,
            "テスト用のカーネルはパンニング"
        );
        k.apply(p, PanelCommand::Play(tone(), true));
        assert_eq!(k.status(p).audio.playing, 1);
        let (l, r) = render_until_sounding(&mut k);
        assert!(r > l * 10.0, "右のパネルから聞こえる: {l} {r}");
        // オプトアウトすると置き場所に関係なく左右同じ。
        k.apply(p, PanelCommand::Spatial(false));
        assert!(!k.status(p).audio.spatial);
        let (l, r) = render_until_sounding(&mut k);
        assert!((l - r).abs() < l * 1e-3, "{l} {r}");
    }

    #[test]
    fn speaker_follows_the_panel_while_playing() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Play(tone(), true));
        render_until_sounding(&mut k);
        // 左へ動かす。
        k.apply(
            p,
            PanelCommand::Place(Pose::new(vr::Vec3::new(-2.0, 0.0, 0.0), vr::Quat::IDENTITY)),
        );
        k.tick();
        let (l, r) = render_until_sounding(&mut k);
        assert!(l > r * 10.0, "{l} {r}");
    }

    /// 点の音はパネルの中のその点から聞こえる（右のパネルの、さらに左端・右端）。
    #[test]
    fn point_sound_comes_from_the_point_in_the_panel() {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        // 正面 1m の幅 2m のパネル。左端と右端は頭から左右 45°。
        let p = k
            .add_panel(
                "t",
                Pose::new(vr::Vec3::new(0.0, 0.0, -1.0), vr::Quat::IDENTITY),
                Vec2::new(2.0, 1.0),
                (200, 100),
            )
            .unwrap();
        let play = |k: &mut Kernel<MockBackend>, x: f32| {
            k.apply(
                p,
                PanelCommand::PlayAt {
                    sound: tone(),
                    at: Vec2::new(x, 50.0),
                    pitch: 1.0,
                },
            );
        };
        play(&mut k, 0.0);
        assert_eq!(k.status(p).audio.playing, 1);
        let (l, r) = render_until_sounding(&mut k);
        assert!(l > r * 2.0, "左端から聞こえる: {l} {r}");
        k.apply(p, PanelCommand::StopAudio);
        settle(&mut k, p);
        play(&mut k, 200.0);
        let (l, r) = render_until_sounding(&mut k);
        assert!(r > l * 2.0, "右端から聞こえる: {l} {r}");
        k.apply(p, PanelCommand::Close);
        k.tick();
        settle(&mut k, p);
        assert_eq!(k.status(p).audio.playing, 0);
    }

    #[test]
    fn layout_switches_and_close_stops_the_panel_sound() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::SpeakerLayout(Layout::Stereo));
        assert_eq!(k.status(p).audio.layout, Layout::Stereo);
        k.apply(p, PanelCommand::Play(tone(), true));
        k.apply(p, PanelCommand::Play(tone(), true));
        assert_eq!(k.status(p).audio.playing, 2, "同時に鳴らせる");
        render_until_sounding(&mut k);
        k.apply(p, PanelCommand::StopAudio);
        settle(&mut k, p);
        assert_eq!(k.status(p).audio.playing, 0);
        k.apply(p, PanelCommand::Play(tone(), true));
        render_until_sounding(&mut k);
        k.apply(p, PanelCommand::Close);
        k.tick();
        settle(&mut k, p);
        assert_eq!(k.status(p).audio.playing, 0);
        // 閉じたパネルでは鳴らさない。
        k.apply(p, PanelCommand::Play(tone(), true));
        assert_eq!(k.status(p).audio.playing, 0);
    }
}
