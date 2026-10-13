//! 結合テスト: VR(モック) → 入力 → オーバーレイ → ブリッジ → GPUI ビューまで全層を通す。

use gpui::TestAppContext;
use overlay::Overlay;
use ui::{Home, PointerBridge};
use vr::{Hand, Pose, Quat, RawController, Vec2, Vec3, mock::MockBackend};

const SIZE: Vec2 = Vec2::new(0.8, 0.6);

/// 解像度 `res` のパネル上のピクセル `px` を指すように、パネル正面からレーザーを向ける。
fn aim_at(px: Vec2, res: (u32, u32)) -> Pose {
    let x = (px.x / res.0 as f32 - 0.5) * SIZE.x;
    let y = (0.5 - px.y / res.1 as f32) * SIZE.y;
    Pose::new(Vec3::new(x, y, 0.0), Quat::IDENTITY)
}

fn set_controller(vr: &mut MockBackend, aim: Pose, trigger: f32) {
    vr.frame.controllers = vec![RawController {
        hand: Hand::Left,
        profile: "oculus/touch_controller".into(),
        aim,
        inputs: vec![("trigger/value".into(), trigger)],
    }];
}

#[gpui::test]
fn trigger_on_button_clicks_gpui_view(cx: &mut TestAppContext) {
    let (home, cx) = cx.add_window_view(|_, _| Home::default());
    cx.run_until_parked();
    let c = cx
        .debug_bounds("click-me")
        .expect("ボタンが描画されている")
        .center();
    let target = Vec2::new(c.x.into(), c.y.into());
    // 実アプリと同じく、パネル解像度 = ウィンドウサイズ
    let vp = cx.update(|window, _| window.viewport_size());
    let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);

    let mut vr = MockBackend::default();
    let mut ov = Overlay::new();
    let panel = ov
        .add_panel(
            &mut vr,
            "home",
            Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
            SIZE,
            res,
        )
        .unwrap();
    let mut bridge = PointerBridge::default();

    for trigger in [0.0, 1.0, 0.0] {
        set_controller(&mut vr, aim_at(target, res), trigger);
        for ev in ov.tick(&mut vr) {
            assert_eq!(ev.panel, panel);
            let input = bridge.translate(ev.event);
            cx.update(|window, cx| {
                window.dispatch_event(input, cx);
            });
        }
    }
    cx.run_until_parked();
    assert_eq!(home.read_with(cx, |h, _| h.clicks), 1);
}
