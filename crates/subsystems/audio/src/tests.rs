use super::*;

const RATE: u32 = 48_000;

/// 指定フレーム数をレンダリングし、左右それぞれのエネルギー（二乗和）を返す。
fn energy(engine: &mut Engine, frames: usize) -> (f32, f32) {
    let mut out = vec![0.0; frames * 2];
    engine.render(&mut out);
    out.as_chunks::<2>()
        .0
        .iter()
        .fold((0.0, 0.0), |(l, r), s| (l + s[0] * s[0], r + s[1] * s[1]))
}

/// 一定振幅のテスト音声（`channels` は 1 または 2。2ch 時は左 `left`、右 `right`）。
fn tone(left: f32, right: f32, channels: u16, frames: usize) -> Clip {
    let samples: Vec<f32> = (0..frames)
        .flat_map(|i| {
            // 符号を交互に反転して DC 成分を除去する（HRTF は低域を減衰させる）。
            let s = if i % 2 == 0 { 1.0 } else { -1.0 };
            [left * s, right * s]
        })
        .take(frames * channels as usize)
        .collect();
    Clip::from_samples(&samples, channels, RATE).unwrap()
}

fn speaker(position: Vec3) -> Speaker {
    Speaker {
        position,
        width: 1.0,
        ..Default::default()
    }
}

#[test]
fn identical_stereo_is_mono_and_auto_follows_the_clip() {
    let mono = Clip::from_samples(&[0.1, 0.1, 0.2, 0.2], 2, RATE).unwrap();
    assert_eq!(mono.channels(), Channels::Mono);
    assert_eq!(mono.frames(), 2);
    let stereo = Clip::from_samples(&[0.1, 0.0, 0.2, 0.2], 2, RATE).unwrap();
    assert_eq!(stereo.channels(), Channels::Stereo);
    let one = Clip::from_samples(&[0.1, 0.2], 1, RATE).unwrap();
    assert_eq!(one.channels(), Channels::Mono);
    assert_eq!(Layout::Auto.resolve(&mono), Channels::Mono);
    assert_eq!(Layout::Auto.resolve(&stereo), Channels::Stereo);
    assert_eq!(Layout::Stereo.resolve(&mono), Channels::Stereo);
    assert_eq!(Layout::Mono.resolve(&stereo), Channels::Mono);
    // 3ch 以上は先頭 2ch のみ使用。
    let surround = Clip::from_samples(&[0.1, 0.2, 0.9, 0.3, 0.4, 0.9], 3, RATE).unwrap();
    assert_eq!(surround.channels(), Channels::Stereo);
    assert_eq!(surround.frames(), 2);
    assert!(Clip::from_samples(&[], 0, RATE).is_err());
}

#[test]
fn reads_wav() {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut bytes, spec).unwrap();
    for s in [16384i16, 0, -16384, 0] {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
    let clip = Clip::from_wav(std::io::Cursor::new(bytes.into_inner())).unwrap();
    assert_eq!(clip.channels(), Channels::Stereo);
    assert_eq!(clip.sample_rate(), 44_100);
    assert_eq!(&clip.samples[..], &[0.5, 0.0, -0.5, 0.0]);
    assert!(Clip::from_wav(std::io::Cursor::new(b"not a wav")).is_err());
}

#[test]
fn spatial_speaker_is_heard_from_its_side() {
    let mut e = Engine::offline(RATE, false);
    assert_eq!(e.spatializer(), Spatializer::Panning);
    e.set_speaker(1, speaker(Vec3::new(2.0, 0.0, 0.0)));
    e.play(1, tone(0.5, 0.5, 1, RATE as usize), true);
    let (l, r) = energy(&mut e, 4096);
    assert!(r > l * 10.0, "右側に配置したスピーカーの音は右から聞こえる: {l} {r}");
    // 頭を 180° 右回転（後方を向く）すると左から聞こえる。
    e.set_listener(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::PI));
    let (l, r) = energy(&mut e, 4096);
    assert!(l > r * 10.0, "{l} {r}");
}

#[test]
fn opting_out_of_spatialization_ignores_position() {
    let mut e = Engine::offline(RATE, false);
    e.set_speaker(
        1,
        Speaker {
            spatial: false,
            ..speaker(Vec3::new(5.0, 0.0, 0.0))
        },
    );
    e.play(1, tone(0.5, 0.5, 1, RATE as usize), true);
    let (l, r) = energy(&mut e, 4096);
    assert!(l > 0.0 && (l - r).abs() < 1e-3, "{l} {r}");
    // 距離による減衰もない。
    let (near, _) = {
        let mut e = Engine::offline(RATE, false);
        e.set_speaker(
            1,
            Speaker {
                spatial: false,
                ..Default::default()
            },
        );
        e.play(1, tone(0.5, 0.5, 1, RATE as usize), true);
        energy(&mut e, 4096)
    };
    assert!((near - l).abs() < 1e-3);
}

#[test]
fn distance_attenuates_beyond_one_meter() {
    let level = |z: f32| {
        let mut e = Engine::offline(RATE, false);
        e.set_speaker(1, speaker(Vec3::new(0.0, 0.0, -z)));
        e.play(1, tone(0.5, 0.5, 1, RATE as usize), true);
        energy(&mut e, 4096).0
    };
    assert!((level(0.5) - level(1.0)).abs() < 1e-3, "1m 以内では減衰しない");
    // 2 倍の距離で振幅 1/2 = エネルギー 1/4 となる。
    assert!((level(2.0) / level(1.0) - 0.25).abs() < 0.01);
}

#[test]
fn stereo_speaker_plays_each_channel_from_its_edge() {
    // 左チャンネルのみの音声を、正面 2m・幅 4m のスピーカーで再生。左端（-2, 0, -2）から聞こえる。
    let left_only = || tone(0.5, 0.0, 2, RATE as usize);
    let mut e = Engine::offline(RATE, false);
    e.set_speaker(
        1,
        Speaker {
            width: 4.0,
            ..speaker(Vec3::new(0.0, 0.0, -2.0))
        },
    );
    e.play(1, left_only(), true);
    let (l, r) = energy(&mut e, 4096);
    assert!(l > r * 4.0, "{l} {r}");
    // モノラルに切り替えると中央から再生される（左右同一）。
    e.set_speaker(
        1,
        Speaker {
            width: 4.0,
            layout: Layout::Mono,
            ..speaker(Vec3::new(0.0, 0.0, -2.0))
        },
    );
    let (l, r) = energy(&mut e, 4096);
    assert!(l > 0.0 && (l - r).abs() < l * 1e-3, "{l} {r}");
}

#[test]
fn voices_mix_simultaneously_and_finish() {
    let mut e = Engine::offline(RATE, false);
    e.set_speaker(1, speaker(Vec3::NEG_Z));
    e.set_speaker(2, speaker(Vec3::NEG_Z));
    let short = tone(0.1, 0.1, 1, 100);
    let a = e.play(1, short.clone(), false);
    let b = e.play(1, short.clone(), false);
    let c = e.play(2, short, true);
    assert_eq!((e.playing(1), e.playing(2)), (2, 1));
    let one = {
        let mut solo = Engine::offline(RATE, false);
        solo.set_speaker(1, speaker(Vec3::NEG_Z));
        solo.play(1, tone(0.1, 0.1, 1, 100), false);
        energy(&mut solo, 64).0
    };
    let (both, _) = energy(&mut e, 64);
    assert!(
        both > one * 8.0,
        "3 音重畳で振幅 3 倍 = エネルギー 9 倍: {one} {both}"
    );
    energy(&mut e, BLOCK * 2);
    e.poll();
    assert!(!e.is_playing(a) && !e.is_playing(b) && e.is_playing(c));
    assert_eq!(e.playing(1), 0);
    e.stop(c);
    energy(&mut e, BLOCK);
    e.poll();
    assert_eq!(e.playing(2), 0);
}

#[test]
fn removing_speaker_stops_its_voices_only() {
    let mut e = Engine::offline(RATE, false);
    e.set_speaker(1, speaker(Vec3::NEG_Z));
    e.set_speaker(2, speaker(Vec3::NEG_Z));
    let clip = tone(0.1, 0.1, 1, RATE as usize);
    e.play(1, clip.clone(), true);
    e.play(2, clip, true);
    e.remove_speaker(1);
    energy(&mut e, BLOCK);
    e.poll();
    assert_eq!((e.playing(1), e.playing(2)), (0, 1));
    e.stop_speaker(2);
    energy(&mut e, BLOCK);
    e.poll();
    assert_eq!(e.playing(2), 0);
}

#[test]
fn resamples_to_the_output_rate() {
    // 24kHz の 240 フレーム = 10ms は、48kHz の出力で 480 フレーム鳴る。
    let samples = vec![0.5; 240];
    let clip = Clip::from_samples(&samples, 1, 24_000).unwrap();
    let mut e = Engine::offline(RATE, false);
    e.set_speaker(
        1,
        Speaker {
            spatial: false,
            ..Default::default()
        },
    );
    e.play(1, clip, false);
    let mut out = vec![0.0; 1024 * 2];
    e.render(&mut out);
    let sounding = out
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|s| s[0] != 0.0)
        .count();
    assert_eq!(sounding, 480);
}

#[test]
fn sound_is_decoded_off_thread_and_failures_finish() {
    let mut e = Engine::offline(RATE, false);
    let v = e.play_sound(1, Sound::File("does/not/exist.wav".into()), false);
    assert!(e.is_playing(v), "読み込み中も再生中として扱われる");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while e.is_playing(v) && std::time::Instant::now() < deadline {
        e.poll();
        std::thread::yield_now();
    }
    assert!(!e.is_playing(v));
}

#[test]
fn stopping_while_loading_never_sounds() {
    let mut bytes = std::io::Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::new(&mut bytes, spec).unwrap();
    for _ in 0..RATE {
        w.write_sample(8000i16).unwrap();
    }
    w.finalize().unwrap();
    let wav: &'static [u8] = bytes.into_inner().leak();
    for removal in [false, true] {
        let mut e = Engine::offline(RATE, false);
        e.set_speaker(1, speaker(Vec3::NEG_Z));
        let v = e.play_sound(1, Sound::Wav(wav), true);
        // 読み込み完了前に停止する（Stop が Play より先にミキサーへ到達する）。
        if removal {
            e.remove_speaker(1);
        } else {
            e.stop(v);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut heard = 0.0;
        while e.is_playing(v) && std::time::Instant::now() < deadline {
            heard += energy(&mut e, BLOCK).0;
            e.poll();
            std::thread::yield_now();
        }
        assert!(
            !e.is_playing(v) && heard == 0.0,
            "removal={removal}: {heard}"
        );
    }
}

/// 実際の Steam Audio（`NYAW_PHONON` または OS 既定パスの phonon）を用いて、HRTF により右側から聞こえることを検証する。
/// `cargo test -p audio -- --ignored steam_audio`
#[test]
#[ignore]
fn steam_audio_spatializes_with_hrtf() {
    let mut e = Engine::offline(RATE, true);
    assert_eq!(e.spatializer(), Spatializer::SteamAudio);
    e.set_speaker(1, speaker(Vec3::new(2.0, 0.0, 0.0)));
    e.play(1, tone(0.5, 0.5, 1, RATE as usize), true);
    // 畳み込みの過渡応答を捨ててから測定する。
    energy(&mut e, BLOCK * 4);
    let (l, r) = energy(&mut e, 4096);
    assert!(r > l * 2.0, "{l} {r}");
    // ステレオスピーカーでも 2 つの音源それぞれが HRTF を通過する。
    e.set_speaker(2, speaker(Vec3::NEG_Z));
    e.play(2, tone(0.5, 0.0, 2, RATE as usize), true);
    let (l2, r2) = energy(&mut e, 4096);
    assert!(l2 > l && r2 > 0.0);
}

/// 既定の出力デバイスで実際に再生し、コールバックにより再生が完了するまで待機する（音声が出力される）。
/// `cargo test -p audio -- --ignored output_device`（Steam Audio 使用時は `NYAW_PHONON` または実行ファイル同一ディレクトリに phonon を配置）
#[test]
#[ignore]
fn output_device_plays_to_the_end() {
    let mut e = Engine::open();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while e.spatializer() == Spatializer::Starting && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_ne!(
        e.spatializer(),
        Spatializer::Unavailable,
        "出力デバイスが存在しない"
    );
    e.set_speaker(1, speaker(Vec3::new(1.0, 0.0, -1.0)));
    let v = e.play(1, tone(0.2, 0.2, 1, RATE as usize / 4), false);
    while e.is_playing(v) && std::time::Instant::now() < deadline {
        e.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!e.is_playing(v), "コールバックが実行されていない");
}
