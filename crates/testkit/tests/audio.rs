//! 音声テストの画面（`AudioTest`）のボタンから、パネルのスピーカーで音が鳴るまで。
//! テスト用のカーネルは出力デバイスを開かないので、`kernel.audio.render` で回して左右に出た音を測る。

use gpui::TestAppContext;
use kernel::audio::{BLOCK, Layout};
use kernel::subsystems::keyboard::{Database, Layout as KeyLayout};
use kernel::{PanelCommand, Setting};
use std::time::{Duration, Instant};
use testing::{AudioTest, Settings};
use testkit::{Action, Driver, VrKit};

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

/// `selector` を押し、押したときの効果音（ホバーとクリック。`ui::cue`）が鳴り終わって
/// 鳴っている音が `sounds` 個になるまで回す。効果音が混ざると左右の測り方が狂う。
// ponytail: 効果音（0.23 秒）より短いテスト音だと鳴り終わってしまう。そうなったら効果音を止める口を作る
fn press(kit: &mut VrKit<'_, AudioTest>, selector: &'static str, sounds: usize) {
    kit.click(selector);
    let deadline = Instant::now() + Duration::from_secs(5);
    while kit.kernel.status(0).audio.playing > sounds && Instant::now() < deadline {
        kit.kernel.audio.render(&mut [0.0; BLOCK * 2]);
        kit.step();
    }
    assert_eq!(kit.kernel.status(0).audio.playing, sounds);
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
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1.5, "{l} {r}");
    settle(&mut kit);
    press(&mut kit, "audio-left", 1);
    let (l, r) = heard(&mut kit);
    assert!(l > r * 1.5, "{l} {r}");
    settle(&mut kit);
    // 左右が同じ音は自動でモノラル（中央の 1 点）になり、正面なら左右同じ。
    press(&mut kit, "audio-centre", 1);
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
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1000.0, "{l} {r}");
    settle(&mut kit);

    kit.click("audio-layout-mono");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Mono);
    // モノラルに固定すると、片側の音も左右に混ぜて鳴る。
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(l > 0.0 && (l - r).abs() < l * 1e-3, "{l} {r}");
    settle(&mut kit);
    kit.click("audio-layout-stereo");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Stereo);
    kit.click("audio-layout-auto");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Auto);

    // ループは重ねて鳴らせ、止めるまで鳴り続ける。
    press(&mut kit, "audio-loop-1", 1);
    press(&mut kit, "audio-loop-2", 2);
    heard(&mut kit);
    kit.click("audio-stop");
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);

    // パネルを閉じると、そのパネルの音は止まる。
    press(&mut kit, "audio-loop-3", 1);
    heard(&mut kit);
    kit.kernel.apply(0, PanelCommand::Close);
    kit.step();
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);
}

/// 鳴っている音が無くなるまで描き、その間に左右に出た音の二乗和を返す。
fn drain(kit: &mut VrKit<'_, Settings>) -> f32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut energy = 0.0;
    while kit.kernel.status(0).audio.playing > 0 && Instant::now() < deadline {
        let mut out = [0.0; BLOCK * 2];
        kit.kernel.audio.render(&mut out);
        energy += out.iter().map(|s| s * s).sum::<f32>();
        kit.step();
    }
    assert_eq!(kit.kernel.status(0).audio.playing, 0);
    energy
}

/// 設定画面のキーボードの見本のキーは、キーの音の設定どおりに鳴る: 音量を半分にすると振幅が半分になり、オフでは鳴らない。
#[gpui::test]
fn preview_key_sound_is_as_loud_as_the_volume_setting(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| {
        // 読み込みのスレッドと OS の配列を待たず、同じ配列から始める。
        let layout = KeyLayout::new("us", "");
        let keyboard = Database::load().unwrap().keyboard(&layout).unwrap();
        let settings = Settings::new(0);
        let keys = ui::prefs::new_keyboard_preview(0, cx);
        keys.update(cx, |view, cx| {
            view.automatic = false;
            view.select_layout(layout, cx);
            view.keyboard = Some(keyboard);
            view.loading = false;
        });
        assert!(settings.keyboard_preview.set(keys).is_ok());
        settings
    });
    kit.click("settings-input").click("settings-keyboard");
    // レーザーをキーに載せてホバー音が鳴り終わってから押し、キーの音だけを測る。
    let heard = |kit: &mut VrKit<'_, Settings>, setting: Option<Setting>| {
        if let Some(setting) = setting {
            kit.kernel.set(setting);
            kit.step();
        }
        kit.hover("key-AC01");
        drain(kit);
        kit.press(Action::Select).release(Action::Select);
        drain(kit)
    };
    let full = heard(&mut kit, None);
    let half = heard(&mut kit, Some(Setting::KeySoundVolume(0.5)));
    let off = heard(&mut kit, Some(Setting::KeySound(false)));
    assert!(full > 0.0, "既定では鳴る");
    // 二乗和は音量の二乗に比例する。押すたびに高さが 1 ± 0.1 で揺れ、その分だけ長さ（二乗和）も変わる。
    let ratio = half / full;
    assert!((0.15..0.4).contains(&ratio), "{half} / {full} = {ratio}");
    assert_eq!(off, 0.0, "オフでは鳴らない");
}
