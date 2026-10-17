//! 音声テストの画面（`AudioTest`）のボタンから、パネルのスピーカーで音が鳴るまで。
//! テスト用のカーネルは出力デバイスを開かないので、`kernel.audio.render` で回して左右に出た音を測る。

use gpui::TestAppContext;
use kernel::PanelCommand;
use kernel::audio::{BLOCK, Layout};
use std::time::{Duration, Instant};
use testing::AudioTest;
use testkit::{Driver, VrKit};

/// 鳴り始めるまで（読み込みは別スレッド）描き、そこから約 0.3 秒の左右の二乗和を返す。
fn heard(kit: &mut VrKit<'_, AudioTest>) -> (f32, f32) {
    let block = |kit: &mut VrKit<'_, AudioTest>| {
        let mut out = [0.0; BLOCK * 2];
        kit.kernel.audio.render(&mut out);
        out.as_chunks::<2>()
            .0
            .iter()
            .fold((0.0, 0.0), |(l, r), s| (l + s[0] * s[0], r + s[1] * s[1]))
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut e = block(kit);
    while e.0 + e.1 == 0.0 && Instant::now() < deadline {
        std::thread::yield_now();
        e = block(kit);
    }
    for _ in 0..60 {
        let (l, r) = block(kit);
        e = (e.0 + l, e.1 + r);
    }
    e
}

/// 鳴っている音が無くなるまで回す。
fn settle(kit: &mut VrKit<'_, AudioTest>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while kit.kernel.status(0).audio.playing > 0 && Instant::now() < deadline {
        kit.kernel.audio.render(&mut [0.0; BLOCK * 2]);
        kit.step();
    }
}

#[gpui::test]
fn panel_speaker_plays_from_the_panel_edges(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| AudioTest::new(0));
    // 右だけ鳴る 2ch の音は、正面のパネルの右端から聞こえる。
    kit.click("audio-right");
    assert_eq!(kit.kernel.status(0).audio.playing, 1);
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1.5, "{l} {r}");
    settle(&mut kit);
    kit.click("audio-left");
    let (l, r) = heard(&mut kit);
    assert!(l > r * 1.5, "{l} {r}");
    settle(&mut kit);
    // 左右が同じ音は自動でモノラル（中央の 1 点）になり、正面なら左右同じ。
    kit.click("audio-centre");
    let (l, r) = heard(&mut kit);
    assert!(l > 0.0 && (l - r).abs() < l * 0.05, "{l} {r}");
    settle(&mut kit);
}

#[gpui::test]
fn buttons_switch_spatialization_layout_and_mix_loops(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| AudioTest::new(0));
    assert!(kit.kernel.status(0).audio.spatial, "立体化は既定で有効");
    kit.click("audio-spatial");
    assert!(!kit.kernel.status(0).audio.spatial);
    // 立体化を外すと、片側の音はそのまま片側に鳴る（置き場所に関係しない）。
    kit.click("audio-right");
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1000.0, "{l} {r}");
    settle(&mut kit);

    kit.click("audio-layout-mono");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Mono);
    // モノラルに固定すると、片側の音も左右に混ぜて鳴る。
    kit.click("audio-right");
    let (l, r) = heard(&mut kit);
    assert!(l > 0.0 && (l - r).abs() < l * 1e-3, "{l} {r}");
    settle(&mut kit);
    kit.click("audio-layout-stereo");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Stereo);
    kit.click("audio-layout-auto");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Auto);

    // ループは重ねて鳴らせ、止めるまで鳴り続ける。
    kit.click("audio-loop-1").click("audio-loop-2");
    assert_eq!(kit.kernel.status(0).audio.playing, 2);
    heard(&mut kit);
    kit.click("audio-stop");
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);

    // パネルを閉じると、そのパネルの音は止まる。
    kit.click("audio-loop-3");
    heard(&mut kit);
    kit.kernel.apply(0, PanelCommand::Close);
    kit.step();
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);
}
