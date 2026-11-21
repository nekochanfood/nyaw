//! 音声サブシステム。OS の出力デバイス（cpal）・音声ファイルの読み込み・
//! Steam Audio による空間音響（HRTF）の差異を抽象化する。
//!
//! 音は「スピーカー」（音源）から再生される。スピーカーは位置・向き・幅を持ち、モノラルなら中央の 1 点、
//! ステレオなら幅の両端の 2 点から聞こえる。空間音響（`Speaker::spatial`）を無効にすると、位置に関係なくそのまま出力される。
//! 出力・ミキシング・空間音響処理は別スレッド（オーディオコールバック）で実行し、メインスレッドはコマンド送信のみで待機しない。

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

/// 再生中の音を識別するハンドル。
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

/// 再生チャンネル数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channels {
    /// 1ch。スピーカー中央の 1 点から再生する。
    #[default]
    Mono,
    /// 2ch。スピーカーの左端・右端の 2 点から再生する。
    Stereo,
}

/// スピーカーのモノラル・ステレオ再生モード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// 音声データのチャンネル数に従う（[`Clip::channels`]）。
    #[default]
    Auto,
    /// ステレオ音声を左右ミックスしてモノラル再生する。
    Mono,
    /// モノラル音声を左右同一でステレオ再生する。
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

/// デコード済みの音声データ（f32 インターリーブ）。複製は低コスト（内部データは共有）。
#[derive(Clone)]
pub struct Clip {
    samples: Arc<[f32]>,
    /// 1 か 2。
    channels: u16,
    sample_rate: u32,
}

impl Clip {
    /// インターリーブ形式のサンプル列から生成する。3ch 以上は先頭 2ch（左・右）のみ使用する。
    /// 左右が完全に同一のステレオはモノラルとして保持する（[`Layout::Auto`] でモノラル再生となる）。
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

    /// WAV ファイルを読み込む（整数 8〜32bit・浮動小数点 32bit 対応）。
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

    /// この音声のチャンネル数。1ch および左右が完全に同一の 2ch はモノラルとなる。
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

    /// 再生速度を `pitch` 倍にした音声（1.0 = 原速。値を上げると高音・短縮される）。内部データは共有のまま、サンプルレートのみ変更する。
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

/// 同一の音声データを参照していれば等しいと判定する。
impl PartialEq for Clip {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.samples, &other.samples)
    }
}

/// 再生する音声の参照先。読み込み（デコード）はワーカースレッドで実行する。
#[derive(Debug, Clone, PartialEq)]
pub enum Sound {
    File(PathBuf),
    /// 実行ファイルに埋め込まれた WAV データ。
    Wav(&'static [u8]),
}

/// 音源。トラッキング空間（メートル単位・前方 -Z）に配置する。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speaker {
    pub position: Vec3,
    pub rotation: Quat,
    /// ステレオ再生時の左右音源間の距離（m）。ローカル X 軸方向に配置される。
    pub width: f32,
    /// 空間音響を有効にする。無効時は位置・リスナーに関係なく、音声がそのまま左右に出力される。
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

/// 空間音響処理の方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spatializer {
    /// 出力を準備中。
    #[default]
    Starting,
    /// Steam Audio の HRTF。
    SteamAudio,
    /// Steam Audio が利用不可のため左右パンニングのみ。
    Panning,
    /// 出力デバイスが存在しない（音声は破棄される）。
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

/// メインスレッドから使用するエントリポイント。コマンドはチャンネル経由で出力スレッドへ送信し、待機しない。
pub struct Engine {
    commands: Sender<Command>,
    done_tx: Sender<Done>,
    done: Receiver<Done>,
    /// 再生中（読み込み中を含む）の音とその音源スピーカー。
    voices: HashMap<VoiceId, SpeakerId>,
    next: u64,
    spatializer: Arc<AtomicU8>,
    /// [`Self::offline`] 時のみ使用。出力スレッドの代わりに [`Self::render`] で処理を進める。
    offline: Option<Mixer>,
    /// ドロップすると出力スレッドが停止する。
    _stop: Option<Sender<()>>,
}

impl Engine {
    /// 既定の出力デバイスで再生する。デバイスのオープンおよび Steam Audio の読み込みは別スレッドで実行し、即座に返る。
    pub fn open() -> Self {
        let (commands, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let spatializer = Arc::new(AtomicU8::new(0));
        let state = spatializer.clone();
        let done_sender = done_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || device::run(rx, done_sender, &state, stopped));
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

    /// デバイスを開かず、[`Self::render`] で呼び出し側が駆動するエンジン（テスト・書き出し用）。
    /// `steam_audio` が true の場合は Steam Audio を読み込む（失敗時はパンニングにフォールバック）。
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

    /// [`Self::offline`] エンジンを進め、ステレオインターリーブ形式で `out` を埋める。デバイス再生時は何もしない。
    pub fn render(&mut self, out: &mut [f32]) {
        if let Some(m) = &mut self.offline {
            m.render(out);
        }
    }

    pub fn spatializer(&self) -> Spatializer {
        Spatializer::from_u8(self.spatializer.load(Ordering::Relaxed))
    }

    /// リスナー（頭部）の位置と向きを設定する。
    pub fn set_listener(&self, position: Vec3, rotation: Quat) {
        let _ = self.commands.send(Command::Listener(position, rotation));
    }

    pub fn set_speaker(&self, id: SpeakerId, speaker: Speaker) {
        let _ = self.commands.send(Command::Speaker(id, speaker));
    }

    /// スピーカーを除去し、再生中（読み込み中を含む）の音を停止する。
    pub fn remove_speaker(&self, id: SpeakerId) {
        self.stop_speaker(id);
        let _ = self.commands.send(Command::RemoveSpeaker(id));
    }

    /// デコード済みの音声をスピーカーで再生する。同一スピーカーに複数の音声を重ねて再生可能。
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
            // 出力スレッドが存在しない（デバイスなし）。再生せず終了として処理する。
            let _ = self.done_tx.send((id, Some(clip)));
        }
        id
    }

    /// 音声をワーカースレッドで読み込んでから再生する。読み込み失敗時はログ出力のみで再生しない。
    pub fn play_sound(&mut self, speaker: SpeakerId, sound: Sound, looping: bool) -> VoiceId {
        self.play_sound_pitched(speaker, sound, looping, 1.0)
    }

    /// [`Self::play_sound`] を再生速度 `pitch` 倍で再生する（[`Clip::pitched`]）。
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

    /// 音声を停止する。読み込み中の場合、読み込み完了後も再生しない。
    pub fn stop(&self, voice: VoiceId) {
        let _ = self.commands.send(Command::Stop(voice));
    }

    /// 指定スピーカーの再生中の音声をすべて停止する（スピーカー自体は保持）。
    pub fn stop_speaker(&self, speaker: SpeakerId) {
        for (&id, _) in self.voices.iter().filter(|(_, s)| **s == speaker) {
            self.stop(id);
        }
    }

    /// 再生終了した音声を回収する。毎フレーム呼び出す（音声データの解放もここで行う）。
    pub fn poll(&mut self) {
        while let Ok((id, _clip)) = self.done.try_recv() {
            self.voices.remove(&id);
        }
    }

    /// 指定スピーカーで再生中（読み込み中を含む）の音声数。
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
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::Duration;

    /// 既定の出力デバイスの変更を検出するポーリング間隔。
    const POLL: Duration = Duration::from_secs(1);
    /// デバイス不在時に音声を破棄しつつミキサーを進行させるサンプルレート。
    const DISCARD_RATE: usize = 48_000;

    fn lock(mixer: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> {
        mixer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 既定の出力デバイスで再生を継続する。デバイスの切断・既定デバイスの変更時は再オープンする（ミキサーは再利用し、
    /// 再生中の音声は継続する）。`stop` の送信側がドロップされると終了する。
    /// ストリームはこのスレッドが保持する（cpal のストリームはスレッド間移動できない OS がある）。
    pub(super) fn run(
        commands: Receiver<Command>,
        done: Sender<Done>,
        state: &AtomicU8,
        stop: Receiver<()>,
    ) {
        let host = cpal::default_host();
        let mixer = Arc::new(Mutex::new(Mixer::new(
            DISCARD_RATE as u32,
            true,
            commands,
            done,
        )));
        let (lost_tx, lost) = mpsc::channel();
        let mut discard = Vec::new();
        loop {
            let device = host.default_output_device();
            let name = device.as_ref().map(|d| d.name().unwrap_or_default());
            let stream = device
                .ok_or_else(|| "no default output device".to_string())
                .and_then(|d| open(&d, &mixer, lost_tx.clone()));
            let s = match &stream {
                Ok(_) => lock(&mixer).spatializer(),
                Err(e) => {
                    logger::warn!("audio output unavailable", error = e);
                    Spatializer::Unavailable
                }
            };
            logger::info!("audio output state", spatializer = ?s);
            state.store(s.to_u8(), Ordering::Relaxed);
            loop {
                match stop.recv_timeout(POLL) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => return,
                }
                if stream.is_err() {
                    // 再生済みの音声が終了処理されるよう、デバイスの代わりにミキサーを進める（音声は破棄）。
                    discard.resize(DISCARD_RATE * POLL.as_secs() as usize * 2, 0.0);
                    lock(&mixer).render(&mut discard);
                }
                let lost = lost.try_iter().count() > 0;
                let current = host
                    .default_output_device()
                    .map(|d| d.name().unwrap_or_default());
                if lost || current != name {
                    logger::info!("audio output device changed, reopening");
                    break;
                }
            }
            // 次のストリームをオープンする前に現在のストリームを解放する（同一ミキサーの同時駆動を防止）。
            drop(stream);
        }
    }

    /// デバイスをオープンし、コールバックでミキサーの駆動を開始する。デバイス切断時は `lost` へ通知する。
    fn open(
        device: &cpal::Device,
        mixer: &Arc<Mutex<Mixer>>,
        lost: Sender<()>,
    ) -> Result<cpal::Stream, String> {
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let channels = config.channels as usize;
        lock(mixer).set_rate(config.sample_rate.0);
        let mixer = mixer.clone();
        let mut stereo = Vec::new();
        // ponytail: f32 出力のみ対応。整数形式のみのデバイスが必要になった場合は cpal::Sample で変換する。
        let stream = device
            .build_output_stream::<f32, _, _>(
                &config,
                move |out, _| {
                    let frames = out.len() / channels;
                    // 最初の数回のみリサイズが発生する（デバイスのバッファサイズは一定）。
                    stereo.resize(frames * 2, 0.0);
                    lock(&mixer).render(&mut stereo);
                    for (o, s) in out
                        .chunks_exact_mut(channels)
                        .zip(stereo.as_chunks::<2>().0)
                    {
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
                move |e| {
                    if matches!(e, cpal::StreamError::DeviceNotAvailable) {
                        let _ = lost.send(());
                    }
                    logger::warn!("audio output error", error = e);
                },
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
        Ok(stream)
    }
}

#[cfg(test)]
mod tests;
