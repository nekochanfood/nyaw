//! オーディオコールバックスレッド上で動作するミキサー。メインスレッドとの通信はチャンネル経由で行う。

use crate::phonon::{Binaural, Phonon};
use crate::{Channels, Clip, Spatializer, Speaker, SpeakerId, VoiceId};
use glam::{Quat, Vec3};
use std::sync::mpsc::{Receiver, Sender};

/// 1 回に処理するフレーム数（Steam Audio のフレームサイズ）。48kHz で約 5ms。
pub const BLOCK: usize = 256;
/// 同時発音数の上限。HRTF 状態（音源ごとに L / R の 2 つ）を起動時にこの数だけ確保し、
/// コールバック内での割り当てを回避する。
/// ponytail: 固定上限。超過分は HRTF を適用せずパンニングで再生する。不足時は値を増やす。
pub(crate) const MAX_VOICES: usize = 32;
/// 距離減衰の開始距離（m）。この距離以下では減衰しない（Steam Audio の既定逆距離モデルと同等）。
const MIN_DISTANCE: f32 = 1.0;
/// 再生開始前にキャンセルされた音の保持数上限。
const CANCELLED: usize = 256;

pub(crate) enum Command {
    Listener(Vec3, Quat),
    Speaker(SpeakerId, Speaker),
    RemoveSpeaker(SpeakerId),
    Play {
        id: VoiceId,
        speaker: SpeakerId,
        clip: Clip,
        looping: bool,
    },
    Stop(VoiceId),
}

/// 再生終了（停止含む）した音。音声データの解放はメインスレッドで行い、コールバック内では解放しない。
pub(crate) type Done = (VoiceId, Option<Clip>);

struct Voice {
    id: VoiceId,
    speaker: SpeakerId,
    clip: Clip,
    looping: bool,
    /// 音声データ内の再生位置（フレーム単位。出力レートとの差を補間するため小数値で保持）。
    position: f64,
    /// L / R 各音源の HRTF 状態（Steam Audio が無効または不足時は `None`）。
    effects: [Option<Binaural>; 2],
    /// このブロックで再生が終了した。
    ended: bool,
}

pub(crate) struct Mixer {
    rate: u32,
    commands: Receiver<Command>,
    done: Sender<Done>,
    listener: (Vec3, Quat),
    speakers: Vec<(SpeakerId, Speaker)>,
    voices: Vec<Voice>,
    /// 読み込み完了前にキャンセルされた音。到着時に即終了させる。
    /// ponytail: 再生終了直後の停止も残留する。上限超過時は古いものから破棄。
    cancelled: Vec<VoiceId>,
    phonon: Option<Phonon>,
    /// 未使用の HRTF 状態のプール。
    pool: Vec<Binaural>,
    /// ブロック途中で出力バッファが満杯になった場合の残余データ（ステレオインターリーブ）。
    pending: Vec<f32>,
    pending_at: usize,
    scratch: Scratch,
}

/// 1 ブロック分の作業バッファ（コールバック内でのメモリ割り当てを避けるため事前確保して再利用する）。
struct Scratch {
    source: [Vec<f32>; 2],
    left: Vec<f32>,
    right: Vec<f32>,
    out: Vec<f32>,
}

impl Mixer {
    /// `steam_audio` が true の場合は Steam Audio を読み込む（読み込み失敗時はパンニングにフォールバック）。
    pub(crate) fn new(
        rate: u32,
        steam_audio: bool,
        commands: Receiver<Command>,
        done: Sender<Done>,
    ) -> Self {
        let (phonon, pool) = load(rate, steam_audio);
        Self {
            rate,
            commands,
            done,
            listener: (Vec3::ZERO, Quat::IDENTITY),
            speakers: Vec::with_capacity(64),
            voices: Vec::with_capacity(MAX_VOICES * 2),
            cancelled: Vec::with_capacity(CANCELLED),
            phonon,
            pool,
            pending: vec![0.0; BLOCK * 2],
            pending_at: BLOCK * 2,
            scratch: Scratch {
                source: [vec![0.0; BLOCK], vec![0.0; BLOCK]],
                left: vec![0.0; BLOCK],
                right: vec![0.0; BLOCK],
                out: vec![0.0; BLOCK * 2],
            },
        }
    }

    /// 出力サンプルレートを変更する（デバイス切り替え時）。変更があれば Steam Audio を再読み込みする。再生中の音は継続する。
    pub(crate) fn set_rate(&mut self, rate: u32) {
        if rate == self.rate {
            return;
        }
        let steam_audio = self.phonon.is_some();
        self.release();
        (self.phonon, self.pool) = load(rate, steam_audio);
        self.rate = rate;
        for i in 0..self.voices.len() {
            self.voices[i].effects = [self.take_effect(), self.take_effect()];
        }
    }

    fn take_effect(&mut self) -> Option<Binaural> {
        let mut e = self.pool.pop()?;
        self.phonon.as_ref()?.reset(&mut e);
        Some(e)
    }

    /// すべての HRTF 状態を Steam Audio へ返却し、Steam Audio を解放する。
    fn release(&mut self) {
        for v in &mut self.voices {
            self.pool
                .extend(v.effects.iter_mut().filter_map(Option::take));
        }
        if let Some(p) = self.phonon.take() {
            for e in self.pool.drain(..) {
                p.release(e);
            }
        }
    }

    pub(crate) fn spatializer(&self) -> Spatializer {
        if self.phonon.is_some() {
            Spatializer::SteamAudio
        } else {
            Spatializer::Panning
        }
    }

    /// ステレオインターリーブ形式で `out` を埋める。
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        let mut written = 0;
        while written < out.len() {
            if self.pending_at == self.pending.len() {
                self.drain_commands();
                self.mix_block();
                self.pending.copy_from_slice(&self.scratch.out);
                self.pending_at = 0;
            }
            let n = (out.len() - written).min(self.pending.len() - self.pending_at);
            out[written..written + n]
                .copy_from_slice(&self.pending[self.pending_at..self.pending_at + n]);
            written += n;
            self.pending_at += n;
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                Command::Listener(position, rotation) => self.listener = (position, rotation),
                Command::Speaker(id, speaker) => {
                    match self.speakers.iter_mut().find(|(i, _)| *i == id) {
                        Some((_, s)) => *s = speaker,
                        None => self.speakers.push((id, speaker)),
                    }
                }
                Command::RemoveSpeaker(id) => {
                    self.speakers.retain(|(i, _)| *i != id);
                    self.finish_where(|v| v.speaker == id);
                }
                Command::Play {
                    id,
                    speaker,
                    clip,
                    looping,
                } => {
                    if let Some(i) = self.cancelled.iter().position(|c| *c == id) {
                        self.cancelled.swap_remove(i);
                        let _ = self.done.send((id, Some(clip)));
                        continue;
                    }
                    let effects = [self.take_effect(), self.take_effect()];
                    self.voices.push(Voice {
                        id,
                        speaker,
                        clip,
                        looping,
                        position: 0.0,
                        effects,
                        ended: false,
                    });
                }
                Command::Stop(id) => {
                    if self.voices.iter().any(|v| v.id == id) {
                        self.finish_where(|v| v.id == id);
                    } else {
                        if self.cancelled.len() == CANCELLED {
                            self.cancelled.remove(0);
                        }
                        self.cancelled.push(id);
                    }
                }
            }
        }
    }

    fn finish_where(&mut self, mut pred: impl FnMut(&Voice) -> bool) {
        let mut i = 0;
        while i < self.voices.len() {
            if pred(&self.voices[i]) {
                let v = self.voices.swap_remove(i);
                self.finish(v);
            } else {
                i += 1;
            }
        }
    }

    fn finish(&mut self, v: Voice) {
        self.pool.extend(v.effects.into_iter().flatten());
        let _ = self.done.send((v.id, Some(v.clip)));
    }

    fn mix_block(&mut self) {
        let Self {
            rate,
            listener,
            speakers,
            voices,
            phonon,
            scratch,
            ..
        } = self;
        scratch.out.fill(0.0);
        for voice in voices.iter_mut() {
            let speaker = speakers
                .iter()
                .find(|(i, _)| *i == voice.speaker)
                .map_or(Speaker::default(), |(_, s)| *s);
            let channels = speaker.layout.resolve(&voice.clip);
            voice.ended = read(voice, *rate, channels, &mut scratch.source);
            let gain = speaker.volume;
            if !speaker.spatial {
                for i in 0..BLOCK {
                    let (l, r) = match channels {
                        Channels::Mono => (scratch.source[0][i], scratch.source[0][i]),
                        Channels::Stereo => (scratch.source[0][i], scratch.source[1][i]),
                    };
                    scratch.out[i * 2] += l * gain;
                    scratch.out[i * 2 + 1] += r * gain;
                }
            } else {
                let right = speaker.rotation * Vec3::X * (speaker.width * 0.5);
                let points: &[(Vec3, usize)] = match channels {
                    Channels::Mono => &[(speaker.position, 0)],
                    Channels::Stereo => {
                        &[(speaker.position - right, 0), (speaker.position + right, 1)]
                    }
                };
                for &(point, ch) in points {
                    // リスナー座標系（右 +X・上 +Y・前 -Z）における方向と距離を算出。
                    let local = listener.1.inverse() * (point - listener.0);
                    let distance = local.length();
                    let direction = if distance > 1e-4 {
                        local / distance
                    } else {
                        Vec3::NEG_Z
                    };
                    let g = gain * (MIN_DISTANCE / distance.max(MIN_DISTANCE));
                    match (phonon.as_ref(), voice.effects[ch].as_mut()) {
                        (Some(p), Some(effect)) => {
                            p.apply(
                                effect,
                                direction,
                                &mut scratch.source[ch],
                                &mut scratch.left,
                                &mut scratch.right,
                            );
                        }
                        _ => {
                            let (l, r) = pan(direction);
                            for i in 0..BLOCK {
                                scratch.left[i] = scratch.source[ch][i] * l;
                                scratch.right[i] = scratch.source[ch][i] * r;
                            }
                        }
                    }
                    for i in 0..BLOCK {
                        scratch.out[i * 2] += scratch.left[i] * g;
                        scratch.out[i * 2 + 1] += scratch.right[i] * g;
                    }
                }
            }
        }
        for s in scratch.out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        self.finish_where(|v| v.ended);
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.release();
    }
}

/// `steam_audio` が true の場合は Steam Audio と HRTF 状態を初期化する（読み込み失敗時はパンニングにフォールバック）。
fn load(rate: u32, steam_audio: bool) -> (Option<Phonon>, Vec<Binaural>) {
    let phonon = steam_audio
        .then(|| match Phonon::load(rate, BLOCK) {
            Ok(p) => Some(p),
            Err(e) => {
                logger::warn!("steam audio unavailable, using panning", error = e);
                None
            }
        })
        .flatten();
    let pool = phonon
        .as_ref()
        .map(|p| (0..MAX_VOICES * 2).filter_map(|_| p.binaural()).collect())
        .unwrap_or_default();
    (phonon, pool)
}

/// 音声データから 1 ブロック分を `channels` 構成で読み出す（出力レートへ線形補間でリサンプリング）。再生完了時は `true` を返す。
/// ponytail: 線形補間によるリサンプリング。高域がわずかに減衰する。品質が必要な場合は窓付き sinc に変更する。
fn read(voice: &mut Voice, rate: u32, channels: Channels, out: &mut [Vec<f32>; 2]) -> bool {
    let clip = &voice.clip;
    let frames = clip.frames();
    let step = clip.sample_rate as f64 / rate as f64;
    let stride = clip.channels as usize;
    let sample = |frame: usize, ch: usize| clip.samples[frame * stride + ch.min(stride - 1)];
    let mut finished = false;
    let [left, right] = out;
    for (lo, ro) in left.iter_mut().zip(right.iter_mut()).take(BLOCK) {
        if voice.position >= frames as f64 {
            if voice.looping && frames > 0 {
                voice.position -= frames as f64;
            } else {
                finished = true;
                *lo = 0.0;
                *ro = 0.0;
                continue;
            }
        }
        let at = voice.position.floor() as usize;
        let t = (voice.position - at as f64) as f32;
        // ループ時は先頭に戻り、非ループ時は最終フレームの値で補間する。
        let next = if at + 1 < frames {
            at + 1
        } else if voice.looping {
            0
        } else {
            at
        };
        let lerp = |ch| sample(at, ch) * (1.0 - t) + sample(next, ch) * t;
        let (l, r) = (lerp(0), lerp(1));
        match channels {
            Channels::Mono => *lo = (l + r) * 0.5,
            Channels::Stereo => {
                *lo = l;
                *ro = r;
            }
        }
        voice.position += step;
    }
    finished
}

/// Steam Audio 不在時の左右パンニング（等パワー方式）。方向ベクトルの左右成分のみを使用する。
fn pan(direction: Vec3) -> (f32, f32) {
    let angle = (direction.x.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_is_equal_power_and_follows_side() {
        let (l, r) = pan(Vec3::NEG_Z);
        assert!((l - r).abs() < 1e-6 && (l * l + r * r - 1.0).abs() < 1e-6);
        let (l, r) = pan(Vec3::X);
        assert!(r > 0.99 && l < 0.01);
        let (l, r) = pan(Vec3::NEG_X);
        assert!(l > 0.99 && r < 0.01);
    }
}
