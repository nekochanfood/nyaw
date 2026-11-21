//! 音声テスト画面（`AudioTest`）のボタン操作からパネルスピーカーの発音までを検証する。
//! テスト用カーネルは出力デバイスを開かないため、`kernel.audio.render` でレンダリングし左右チャンネルの出力を計測する。

use gpui::TestAppContext;
use kernel::audio::{BLOCK, Layout};
use kernel::subsystems::keyboard::{Database, Layout as KeyLayout};
use kernel::{PanelCommand, Setting};
use settings::Settings;
use std::time::{Duration, Instant};
use testing::AudioTest;
use testkit::{Action, Driver, VrKit};

/// 発音開始まで（読み込みは別スレッド）描画を進め、約 0.3 秒間の左右チャンネルの二乗和を返す。
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

/// `selector` をクリックし、効果音（ホバー・クリック。`ui::cue`）の再生完了後、
/// 再生中の音数が `sounds` になるまで進める。効果音の混入は左右チャンネルの計測精度を損なう。
// ponytail: 効果音（0.23 秒）より短いテスト音の場合は先に再生終了する。その場合は効果音の停止手段を追加する
fn press(kit: &mut VrKit<'_, AudioTest>, selector: &'static str, sounds: usize) {
    kit.click(selector);
    let deadline = Instant::now() + Duration::from_secs(5);
    while kit.kernel.status(0).audio.playing > sounds && Instant::now() < deadline {
        kit.kernel.audio.render(&mut [0.0; BLOCK * 2]);
        kit.step();
    }
    assert_eq!(kit.kernel.status(0).audio.playing, sounds);
}

/// 再生中の音がすべて終了するまで進める。
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
    // 右チャンネルのみの 2ch 音源は、正面パネルの右端から聞こえる。
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1.5, "{l} {r}");
    settle(&mut kit);
    press(&mut kit, "audio-left", 1);
    let (l, r) = heard(&mut kit);
    assert!(l > r * 1.5, "{l} {r}");
    settle(&mut kit);
    // 左右同一の音源は自動的にモノラル（中央 1 点）に変換され、正面配置では左右均等になる。
    press(&mut kit, "audio-centre", 1);
    let (l, r) = heard(&mut kit);
    assert!(l > 0.0 && (l - r).abs() < l * 0.05, "{l} {r}");
    settle(&mut kit);
}

#[gpui::test]
fn buttons_switch_spatialization_layout_and_mix_loops(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| AudioTest::new(0));
    assert!(kit.kernel.status(0).audio.spatial, "空間化は既定で有効");
    kit.click("audio-spatial");
    assert!(!kit.kernel.status(0).audio.spatial);
    // 空間化を無効にすると、片側チャンネルの音はパネル位置に依存せずそのまま片側から出力される。
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(r > l * 1000.0, "{l} {r}");
    settle(&mut kit);

    kit.click("audio-layout-mono");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Mono);
    // モノラル固定時は、片側チャンネルの音も左右均等にミックスされる。
    press(&mut kit, "audio-right", 1);
    let (l, r) = heard(&mut kit);
    assert!(l > 0.0 && (l - r).abs() < l * 1e-3, "{l} {r}");
    settle(&mut kit);
    kit.click("audio-layout-stereo");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Stereo);
    kit.click("audio-layout-auto");
    assert_eq!(kit.kernel.status(0).audio.layout, Layout::Auto);

    // ループ音源は重ねて再生でき、明示的に停止するまで継続する。
    press(&mut kit, "audio-loop-1", 1);
    press(&mut kit, "audio-loop-2", 2);
    heard(&mut kit);
    kit.click("audio-stop");
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);

    // パネルを閉じると、そのパネルに属する音源は停止する。
    press(&mut kit, "audio-loop-3", 1);
    heard(&mut kit);
    kit.kernel.apply(0, PanelCommand::Close);
    kit.step();
    settle(&mut kit);
    assert_eq!(kit.kernel.status(0).audio.playing, 0);
}

/// 再生中の音がすべて終了するまで描画を進め、その間の左右チャンネル出力の二乗和を返す。
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

/// 設定画面のキーボードプレビューのキーは、打鍵音設定に従って発音する。音量 50% で振幅が半分になり、オフでは無音になる。
#[gpui::test]
fn preview_key_sound_is_as_loud_as_the_volume_setting(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| {
        // 非同期読み込みスレッドや OS のレイアウト検出を待たず、固定のレイアウトで開始する。
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
    // レーザーをキー上に合わせてホバー音の終了を待ってから押下し、打鍵音のみを計測する。
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
    assert!(full > 0.0, "既定設定では発音される");
    // 二乗和は音量の二乗に比例する。打鍵ごとにピッチが 1 ± 0.1 で変動し、その分だけ持続時間（二乗和）も変化する。
    let ratio = half / full;
    assert!((0.15..0.4).contains(&ratio), "{half} / {full} = {ratio}");
    assert_eq!(off, 0.0, "オフ設定では無音");
}
