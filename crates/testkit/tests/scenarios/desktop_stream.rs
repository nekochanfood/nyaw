//! UI とオーバーレイを含むシナリオ。依頼に従い実行せず、明示時だけ動くようにする。
use gpui::TestAppContext;
use kernel::{
    PanelCommand,
    desktop::{Frame, Status, TouchPhase, Update, mock},
};
use std::sync::Arc;
use testing::PointerTest;
use testkit::{Driver, VrKit};

#[gpui::test]
#[ignore = "desktop streaming E2E; run explicitly with --ignored"]
fn stream_pixels_and_native_touch_bypass_gpui_gestures(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    let (width, height) = kit.kernel.panel(0).resolution;
    let (stream, control) = mock::stream();
    kit.kernel.set_desktop_stream(0, stream).unwrap();
    control.publish(Update {
        status: Status::Streaming,
        frame: Some(Arc::new(
            Frame::new(
                width,
                height,
                vec![80; width as usize * height as usize * 4],
            )
            .unwrap(),
        )),
    });
    kit.step().click("tap-me");
    let touches = control.touches();
    assert!(touches.iter().any(|t| t.phase == TouchPhase::Down));
    assert!(touches.iter().any(|t| t.phase == TouchPhase::Up));
    assert_eq!(
        kit.read(|v| v.taps),
        0,
        "stream input must not become a GPUI tap"
    );
    assert_eq!(
        kit.texture().pixel(width / 2, height / 2),
        [80; 4],
        "GPUI rasterization must not overwrite the stream"
    );
    kit.down("tap-me");
    kit.kernel.apply(0, PanelCommand::Close);
    kit.step();
    assert!(
        control.stopped(),
        "closing a pressed stream must release its OS session"
    );
}

#[gpui::test]
#[ignore = "desktop streaming E2E; run explicitly with --ignored"]
fn stopped_stream_returns_to_normal_ui_input(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    let (stream, control) = mock::stream();
    kit.kernel.set_desktop_stream(0, stream).unwrap();
    kit.kernel.apply(
        0,
        PanelCommand::Stream(None, kernel::desktop::DEFAULT_FRAME_RATE),
    );
    kit.step().click("tap-me");
    assert_eq!(kit.read(|v| v.taps), 1);
    assert!(control.stopped());
    assert!(control.touches().is_empty());
}
