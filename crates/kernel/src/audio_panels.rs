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
        AudioStatus {
            playing: self.audio.playing(panel as u64),
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
            w.write_sample(if i % 2 == 0 { 8000i16 } else { -8000 }).unwrap();
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
            let e = out.chunks_exact(2).fold((0.0, 0.0), |(l, r), s| {
                (l + s[0] * s[0], r + s[1] * s[1])
            });
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
        assert_eq!((status.playing, status.spatial, status.layout), (0, true, Layout::Auto));
        assert_eq!(status.spatializer, Spatializer::Panning, "テスト用のカーネルはパンニング");
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
