//! ポインタの方式（マウス / タッチのエミュレート）の E2E。PointerTest の画面を VR のレーザーで操作する。
//! 既定はマウス（押したまま動かすとドラッグ）。`pointer-mode` でタッチにすると、押したまま動かすとスワイプ、
//! 動かさずに離すとタップになる。

use gpui::TestAppContext;
use kernel::PointerMode;
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG, PointerTest};
use testkit::{Action, Driver, Vec2, VrKit};

/// `swipe-list` の下寄りを押し、上へ 1/2 の高さぶん少しずつ動かして離す。
fn swipe_up(kit: &mut VrKit<PointerTest>) {
    let b = kit.bounds("swipe-list");
    let (x, y0, h) = (
        f32::from(b.center().x),
        f32::from(b.center().y),
        f32::from(b.size.height),
    );
    kit.point_at(Vec2::new(x, y0 + h / 4.0))
        .press(Action::Select);
    for i in 1..=8 {
        kit.point_at(Vec2::new(x, y0 + h / 4.0 - h / 2.0 * i as f32 / 8.0));
    }
    kit.up();
}

fn scrolled(kit: &mut VrKit<PointerTest>) -> f32 {
    kit.read(|v| f32::from(v.list.offset().y))
}

#[gpui::test]
fn mouse_mode_drags_and_does_not_swipe(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    assert_eq!(
        kit.kernel.pointer_mode(),
        PointerMode::Mouse,
        "既定はマウス"
    );
    assert_eq!(kit.color_at("pointer-mode"), BUTTON_BG);
    kit.drag("drag-chip", "drop-zone");
    assert_eq!(kit.read(|v| v.dropped), 1);
    swipe_up(&mut kit);
    assert_eq!(
        scrolled(&mut kit),
        0.0,
        "マウスでは押したまま動かしてもスクロールしない"
    );
    kit.click("tap-me");
    assert_eq!(kit.read(|v| v.taps), 1);
}

#[gpui::test]
fn touch_mode_swipes_and_taps(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    kit.click("pointer-mode");
    assert_eq!(kit.kernel.pointer_mode(), PointerMode::Touch);
    // レーザーが乗っているとホバーの色になるので、外してから見る。
    assert_eq!(
        kit.hover("tap-me").color_at("pointer-mode"),
        BUTTON_ACTIVE_BG
    );

    swipe_up(&mut kit);
    let s = scrolled(&mut kit);
    assert!(s < -50.0, "指に付いて下へスクロールする（{s}）");
    kit.click("tap-me");
    assert_eq!(kit.read(|v| v.taps), 1, "動かさずに離すとタップ");
    kit.drag("drag-chip", "drop-zone");
    assert_eq!(kit.read(|v| v.dropped), 0, "タッチではドラッグにならない");

    // タッチのままでもボタンで戻せる。
    kit.click("pointer-mode");
    assert_eq!(kit.kernel.pointer_mode(), PointerMode::Mouse);
    assert_eq!(kit.hover("tap-me").color_at("pointer-mode"), BUTTON_BG);
}
