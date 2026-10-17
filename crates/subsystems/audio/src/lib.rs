//! 音声サブシステム。OS の出力デバイス（cpal: Windows は WASAPI、Linux は ALSA）・音声ファイルの読み込み・
//! Steam Audio による立体化（HRTF）の差を吸収する。
//!
//! 音は「スピーカー」（音源）から鳴る。スピーカーは置き場所・向き・幅を持ち、モノラルなら中央の 1 点、
//! ステレオなら幅の両端の 2 点から聞こえる。立体化（`Speaker::spatial`）を外すと、置き場所に関係なくそのまま鳴る。
//! 出力・ミックス・立体化は別スレッド（オーディオのコールバック）で行い、メインスレッドは命令を送るだけで待たない。

mod mixer;
mod phonon;

pub use glam::{Quat, Vec3};
pub use mixer::BLOCK;

use mixer::{Command, Done, Mixer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

pub type SpeakerId = u64;

/// 再生中の音を指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// 鳴らす経路の数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channels {
    /// 1ch。スピーカーの中央の 1 点から鳴る。
    #[default]
    Mono,
    /// 2ch。スピーカーの左端・右端の 2 点から鳴る。
    Stereo,
}

/// スピーカーをモノラル・ステレオのどちらで鳴らすか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// 鳴らす音に合わせる（[`Clip::channels`]）。
    #[default]
    Auto,
    /// ステレオの音は左右を混ぜて鳴らす。
    Mono,
    /// モノラルの音は左右に同じ音を鳴らす。
    Stereo,
}

impl Layout {
    pub fn resolve(self, clip: &Clip) -> Channels {
        match self {
            Layout::Auto => clip.channels(),
            Layout::Mono => Channels::Mono,
            Layout::Stereo => Channels::Stereo,
        }
    }
}

/// デコード済みの音声（f32 のインターリーブ）。複製は安い（中身は共有）。
#[derive(Clone)]
pub struct Clip {
    samples: Arc<[f32]>,
    /// 1 か 2。
    channels: u16,
    sample_rate: u32,
}

impl Clip {
    /// インターリーブのサンプルから作る。3ch 以上は前の 2ch（左・右）だけを使う。
    /// 左右がまったく同じステレオはモノラルとして持つ（[`Layout::Auto`] でモノラルに鳴る）。
    pub fn from_samples(samples: &[f32], channels: u16, sample_rate: u32) -> Result<Self, Error> {
        if channels == 0 || sample_rate == 0 {
            return Err(Error(format!(
                "invalid audio format: {channels}ch {sample_rate}Hz"
            )));
        }
        let stride = channels as usize;
        let frames = samples.chunks_exact(stride);
        let mono = channels == 1 || frames.clone().all(|f| f[0] == f[1]);
        let samples: Arc<[f32]> = if mono {
            frames.map(|f| f[0]).collect()
        } else {
            frames.flat_map(|f| [f[0], f[1]]).collect()
        };
        Ok(Self {
            samples,
            channels: if mono { 1 } else { 2 },
            sample_rate,
        })
    }

    /// WAV（整数 8〜32bit・浮動小数点 32bit）を読む。
    pub fn from_wav(reader: impl std::io::Read) -> Result<Self, Error> {
        let err = |e: hound::Error| Error(e.to_string());
        let mut wav = hound::WavReader::new(reader).map_err(err)?;
        let spec = wav.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => wav.samples::<f32>().collect::<Result<_, _>>(),
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                wav.samples::<i32>()
                    .map(|s| s.map(|s| s as f32 * scale))
                    .collect::<Result<_, _>>()
            }
        }
        .map_err(err)?;
        Self::from_samples(&samples, spec.channels, spec.sample_rate)
    }

    pub fn load(sound: &Sound) -> Result<Self, Error> {
        match sound {
            Sound::File(path) => {
                let file = std::fs::File::open(path)
                    .map_err(|e| Error(format!("{}: {e}", path.display())))?;
                Self::from_wav(std::io::BufReader::new(file))
            }
            Sound::Wav(bytes) => Self::from_wav(std::io::Cursor::new(bytes)),
        }
    }

    /// この音の経路の数。1ch と、左右がまったく同じ 2ch はモノラル。
    pub fn channels(&self) -> Channels {
        if self.channels == 1 {
            Channels::Mono
        } else {
            Channels::Stereo
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 速さを `pitch` 倍にした音（1 = そのまま。上げると高く短くなる）。中身は共有したまま、標本化レートだけを変える。
    pub fn pitched(mut self, pitch: f32) -> Self {
        self.sample_rate = (self.sample_rate as f32 * pitch).round().max(1.0) as u32;
        self
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("frames", &self.frames())
            .finish()
    }
}

/// 同じ音声データを指していれば等しい。
impl PartialEq for Clip {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.samples, &other.samples)
    }
}

/// 鳴らす音の在りか。読み込み（デコード）は別スレッドで行う。
#[derive(Debug, Clone, PartialEq)]
pub enum Sound {
    File(PathBuf),
    /// 実行ファイルに埋め込んだ WAV。
    Wav(&'static [u8]),
}

/// 音源。トラッキング空間（メートル・前方 -Z）に置く。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speaker {
    pub position: Vec3,
    pub rotation: Quat,
    /// ステレオの左右の点の間隔（m）。ローカルの X 方向に並ぶ。
    pub width: f32,
    /// 置き場所から聞こえるようにする。外すと置き場所・聞き手に関係なく、そのまま左右に鳴る。
    pub spatial: bool,
    pub layout: Layout,
    pub volume: f32,
}

impl Default for Speaker {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            width: 0.0,
            spatial: true,
            layout: Layout::Auto,
            volume: 1.0,
        }
    }
}

/// 立体化の手段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spatializer {
    /// 出力の準備中。
    #[default]
    Starting,
    /// Steam Audio の HRTF。
    SteamAudio,
    /// Steam Audio が無いので左右の音量の振り分けだけ。
    Panning,
    /// 出力デバイスが無い（音は捨てる）。
    Unavailable,
}

impl Spatializer {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::SteamAudio,
            2 => Self::Panning,
            3 => Self::Unavailable,
            _ => Self::Starting,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Starting => 0,
            Self::SteamAudio => 1,
            Self::Panning => 2,
            Self::Unavailable => 3,
        }
    }
}

/// メインスレッドから使う入口。命令はチャンネルで出力のスレッドへ送り、待たない。
pub struct Engine {
    commands: Sender<Command>,
    done_tx: Sender<Done>,
    done: Receiver<Done>,
    /// 鳴っている（読み込み中を含む）音と、そのスピーカー。
    voices: HashMap<VoiceId, SpeakerId>,
    next: u64,
    spatializer: Arc<AtomicU8>,
    /// [`Self::offline`] のときだけ。出力のスレッドの代わりに [`Self::render`] で回す。
    offline: Option<Mixer>,
    /// 落とすと出力のスレッドが止まる。
    _stop: Option<Sender<()>>,
}

impl Engine {
    /// 既定の出力デバイスで鳴らす。デバイスを開く・Steam Audio を読むのは別スレッドで、すぐ返る。
    pub fn open() -> Self {
        let (commands, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let spatializer = Arc::new(AtomicU8::new(0));
        let state = spatializer.clone();
        let done_sender = done_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || match device::start(rx, done_sender) {
                Ok((_stream, s)) => {
                    logger::info!("audio output started", spatializer = ?s);
                    state.store(s.to_u8(), Ordering::Relaxed);
                    // ストリームはこのスレッドで持ち続ける（cpal のストリームはスレッドをまたげない OS がある）。
                    let _ = stopped.recv();
                }
                Err(e) => {
                    logger::warn!("audio output unavailable", error = e);
                    state.store(Spatializer::Unavailable.to_u8(), Ordering::Relaxed);
                }
            });
        if let Err(e) = spawned {
            logger::warn!("failed to spawn audio thread", error = e);
            spatializer.store(Spatializer::Unavailable.to_u8(), Ordering::Relaxed);
        }
        Self {
            commands,
            done_tx,
            done,
            voices: HashMap::new(),
            next: 0,
            spatializer,
            offline: None,
            _stop: Some(stop),
        }
    }

    /// デバイスを開かず、[`Self::render`] で呼び出し側が回すエンジン（テスト・書き出し用）。
    /// `steam_audio` なら Steam Audio を読む（読めなければパンニング）。
    pub fn offline(sample_rate: u32, steam_audio: bool) -> Self {
        let (commands, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let mixer = Mixer::new(sample_rate, steam_audio, rx, done_tx.clone());
        let spatializer = Arc::new(AtomicU8::new(mixer.spatializer().to_u8()));
        Self {
            commands,
            done_tx,
            done,
            voices: HashMap::new(),
            next: 0,
            spatializer,
            offline: Some(mixer),
            _stop: None,
        }
    }

    /// [`Self::offline`] のエンジンを進め、ステレオのインターリーブで `out` を埋める。デバイスで鳴らしているときは何もしない。
    pub fn render(&mut self, out: &mut [f32]) {
        if let Some(m) = &mut self.offline {
            m.render(out);
        }
    }

    pub fn spatializer(&self) -> Spatializer {
        Spatializer::from_u8(self.spatializer.load(Ordering::Relaxed))
    }

    /// 聞き手（頭）の位置と向き。
    pub fn set_listener(&self, position: Vec3, rotation: Quat) {
        let _ = self.commands.send(Command::Listener(position, rotation));
    }

    pub fn set_speaker(&self, id: SpeakerId, speaker: Speaker) {
        let _ = self.commands.send(Command::Speaker(id, speaker));
    }

    /// スピーカーを外し、そこで鳴っている（読み込み中を含む）音を止める。
    pub fn remove_speaker(&self, id: SpeakerId) {
        self.stop_speaker(id);
        let _ = self.commands.send(Command::RemoveSpeaker(id));
    }

    /// デコード済みの音をスピーカーで鳴らす。同じスピーカーでもいくつでも重ねて鳴らせる。
    pub fn play(&mut self, speaker: SpeakerId, clip: Clip, looping: bool) -> VoiceId {
        let id = self.reserve(speaker);
        if let Err(mpsc::SendError(Command::Play { clip, .. })) =
            self.commands.send(Command::Play {
                id,
                speaker,
                clip,
                looping,
            })
        {
            // 出力のスレッドが無い（デバイスが無い）。鳴らずに終わったことにする。
            let _ = self.done_tx.send((id, Some(clip)));
        }
        id
    }

    /// 音を別スレッドで読み込んでから鳴らす。読めなければログに出し、鳴らずに終わる。
    pub fn play_sound(&mut self, speaker: SpeakerId, sound: Sound, looping: bool) -> VoiceId {
        self.play_sound_pitched(speaker, sound, looping, 1.0)
    }

    /// [`Self::play_sound`] を、速さを `pitch` 倍にして（[`Clip::pitched`]）鳴らす。
    pub fn play_sound_pitched(
        &mut self,
        speaker: SpeakerId,
        sound: Sound,
        looping: bool,
        pitch: f32,
    ) -> VoiceId {
        let id = self.reserve(speaker);
        let commands = self.commands.clone();
        let done = self.done_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("audio-decode".into())
            .spawn(move || match Clip::load(&sound) {
                Ok(clip) => {
                    let clip = clip.pitched(pitch);
                    if let Err(mpsc::SendError(Command::Play { clip, .. })) =
                        commands.send(Command::Play {
                            id,
                            speaker,
                            clip,
                            looping,
                        })
                    {
                        let _ = done.send((id, Some(clip)));
                    }
                }
                Err(e) => {
                    logger::warn!("failed to load sound", sound = ?sound, error = e);
                    let _ = done.send((id, None));
                }
            });
        if let Err(e) = spawned {
            logger::warn!("failed to spawn audio decoder", error = e);
            let _ = self.done_tx.send((id, None));
        }
        id
    }

    fn reserve(&mut self, speaker: SpeakerId) -> VoiceId {
        self.next += 1;
        let id = VoiceId(self.next);
        self.voices.insert(id, speaker);
        id
    }

    /// 音を止める。読み込み中なら、読み終えても鳴らさない。
    pub fn stop(&self, voice: VoiceId) {
        let _ = self.commands.send(Command::Stop(voice));
    }

    /// そのスピーカーで鳴っている音をすべて止める（スピーカーは残す）。
    pub fn stop_speaker(&self, speaker: SpeakerId) {
        for (&id, _) in self.voices.iter().filter(|(_, s)| **s == speaker) {
            self.stop(id);
        }
    }

    /// 鳴り終わった音を受け取る。毎フレーム呼ぶ（音声データの解放もここで行う）。
    pub fn poll(&mut self) {
        while let Ok((id, _clip)) = self.done.try_recv() {
            self.voices.remove(&id);
        }
    }

    /// そのスピーカーで鳴っている（読み込み中を含む）音の数。
    pub fn playing(&self, speaker: SpeakerId) -> usize {
        self.voices.values().filter(|s| **s == speaker).count()
    }

    pub fn is_playing(&self, voice: VoiceId) -> bool {
        self.voices.contains_key(&voice)
    }
}

mod device {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    /// 既定の出力デバイスを開き、ミキサーをコールバックで回し始める。
    pub(super) fn start(
        commands: Receiver<Command>,
        done: Sender<Done>,
    ) -> Result<(cpal::Stream, Spatializer), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let channels = config.channels as usize;
        let mut mixer = Mixer::new(config.sample_rate.0, true, commands, done);
        let spatializer = mixer.spatializer();
        let mut stereo = Vec::new();
        // ponytail: f32 の出力だけ。WASAPI の共有モードと PipeWire / PulseAudio の ALSA は f32 を受ける。
        // 整数しか受けないデバイスが出たら cpal::Sample で変換する。
        let stream = device
            .build_output_stream::<f32, _, _>(
                &config,
                move |out, _| {
                    let frames = out.len() / channels;
                    // 最初の数回だけ伸びる（デバイスのバッファの大きさは変わらない）。
                    stereo.resize(frames * 2, 0.0);
                    mixer.render(&mut stereo);
                    for (o, s) in out.chunks_exact_mut(channels).zip(stereo.chunks_exact(2)) {
                        match o {
                            [m] => *m = (s[0] + s[1]) * 0.5,
                            [l, r, rest @ ..] => {
                                *l = s[0];
                                *r = s[1];
                                rest.fill(0.0);
                            }
                            [] => {}
                        }
                    }
                },
                |e| logger::warn!("audio output error", error = e),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        logger::info!(
            "audio output device",
            device = device.name().unwrap_or_default(),
            sample_rate = config.sample_rate.0,
            channels = channels
        );
        Ok((stream, spatializer))
    }
}

#[cfg(test)]
mod tests;
