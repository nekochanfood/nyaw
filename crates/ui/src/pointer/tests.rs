use super::*;

#[test]
fn click_and_scroll_recheck_stationary_hover() {
    let mut bridge = PointerBridge::default();
    let pos = Vec2::new(20.0, 30.0);
    assert_eq!(
        bridge
            .translate(PointerEvent::Move { pos }, PointerMode::Mouse)
            .len(),
        1
    );
    bridge.translate(PointerEvent::Down { pos }, PointerMode::Mouse);
    bridge.translate(PointerEvent::Up { pos }, PointerMode::Mouse);
    assert_eq!(
        bridge
            .translate(PointerEvent::Move { pos }, PointerMode::Mouse)
            .len(),
        1
    );
    assert!(
        bridge
            .translate(PointerEvent::Move { pos }, PointerMode::Mouse)
            .is_empty()
    );
    bridge.translate(
        PointerEvent::Scroll {
            pos,
            delta: Vec2::Y,
        },
        PointerMode::Mouse,
    );
    assert_eq!(
        bridge
            .translate(PointerEvent::Move { pos }, PointerMode::Mouse)
            .len(),
        1
    );
}

#[test]
fn modified_click_uses_the_requested_button_in_touch_mode() {
    let mut bridge = PointerBridge::default();
    let pos = Vec2::new(20.0, 30.0);
    let down = bridge.translate(
        PointerEvent::DownButton {
            pos,
            button: ClickButton::Right,
        },
        PointerMode::Touch,
    );
    assert!(matches!(&down[..], [PlatformInput::MouseDown(e)] if e.button == MouseButton::Right));
    let up = bridge.translate(
        PointerEvent::UpButton {
            pos,
            button: ClickButton::Right,
        },
        PointerMode::Touch,
    );
    assert!(matches!(&up[..], [PlatformInput::MouseUp(e)] if e.button == MouseButton::Right));
}

/// 一方の手がパネルにマウスを動かしたら、もう一方の手は次に位置から送り直す（GPUI のマウスは 1 つ）。
#[test]
fn one_hand_moving_the_mouse_makes_the_other_resend_its_position() {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    let panel = kernel
        .add_panel(
            "keyboard",
            Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
            Vec2::ONE,
            (100, 100),
        )
        .unwrap();
    kernel.subsystems.backend.frame.controllers = vec![kernel::vr::RawController {
        hand: Hand::Right,
        profile: "valve/index_controller".into(),
        aim: Pose::IDENTITY,
        inputs: Vec::new(),
    }];
    let left = PointerBridge {
        sent: Some(Point::default()),
        ..Default::default()
    };
    let mut bridges = HashMap::from([((panel, Hand::Left), left)]);
    let input = pump(&mut kernel, &mut bridges);
    assert!(input.iter().any(|(_, h, _)| *h == Hand::Right));
    let left = bridges.get_mut(&(panel, Hand::Left)).unwrap();
    assert!(left.on_panel());
    assert!(matches!(
        left.translate(PointerEvent::Move { pos: Vec2::ZERO }, PointerMode::Mouse)
            .as_slice(),
        [PlatformInput::MouseMove(_)]
    ));
    left.translate(PointerEvent::Leave, PointerMode::Mouse);
    assert!(!left.on_panel());
}

#[test]
fn move_to_the_same_place_is_sent_once_until_leave() {
    let mut b = PointerBridge::default();
    let pos = Vec2::new(3.0, 4.0);
    let mut send = |ev| b.translate(ev, PointerMode::Mouse).len();
    assert_eq!(send(PointerEvent::Move { pos }), 1);
    assert_eq!(send(PointerEvent::Move { pos }), 0, "not sent while still");
    assert_eq!(send(PointerEvent::Down { pos }), 1);
    assert_eq!(send(PointerEvent::Move { pos }), 0);
    assert_eq!(send(PointerEvent::Leave), 1);
    assert_eq!(send(PointerEvent::Move { pos }), 1, "sent on re-entry");
}

#[test]
fn leave_releases_held_button() {
    let mut b = PointerBridge::default();
    b.translate(
        PointerEvent::Down {
            pos: Vec2::new(3.0, 4.0),
        },
        PointerMode::Mouse,
    );
    let [PlatformInput::MouseExited(e)] = &b.translate(PointerEvent::Leave, PointerMode::Mouse)[..]
    else {
        panic!()
    };
    assert_eq!(
        (e.position, e.pressed_button),
        (Point::new(px(3.0), px(4.0)), Some(MouseButton::Left))
    );
    assert!(!b.pressed);
}

/// 遠い・画素の細かいパネルでは、スワイプとみなす動き（`slop`）が大きくなり、手ブレ程度の動きはタップのまま。
#[test]
fn wider_slop_keeps_a_shaky_tap_a_tap() {
    let touch = PointerMode::Touch;
    let mut b = PointerBridge {
        slop: 40.0,
        ..Default::default()
    };
    b.translate(
        PointerEvent::Down {
            pos: Vec2::new(10.0, 10.0),
        },
        touch,
    );
    // 固定の下限（SWIPE_SLOP）は超えるが、このパネルの閾値の内。
    let moved = b.translate(
        PointerEvent::Move {
            pos: Vec2::new(40.0, 10.0),
        },
        touch,
    );
    assert!(moved.is_empty());
    let tap = b.translate(
        PointerEvent::Up {
            pos: Vec2::new(40.0, 10.0),
        },
        touch,
    );
    assert!(tap.iter().any(|i| matches!(i, PlatformInput::MouseDown(_))));
}

/// タッチ: 動かさずに離すとタップ（押して離す）、しきい値を超えて動かすとスワイプ（指に付いてくるスクロール）だけになる。
#[test]
fn touch_taps_or_swipes() {
    let pos = |x, y| Vec2::new(x, y);
    let kinds = |v: Vec<PlatformInput>| {
        v.iter()
            .map(|i| match i {
                PlatformInput::MouseMove(_) => "move",
                PlatformInput::MouseDown(_) => "down",
                PlatformInput::MouseUp(_) => "up",
                PlatformInput::ScrollWheel(_) => "scroll",
                _ => "other",
            })
            .collect::<Vec<_>>()
    };
    let touch = PointerMode::Touch;
    let mut b = PointerBridge::default();
    assert!(
        b.translate(
            PointerEvent::Down {
                pos: pos(10.0, 10.0)
            },
            touch
        )
        .is_empty()
    );
    assert!(
        b.translate(
            PointerEvent::Move {
                pos: pos(12.0, 12.0)
            },
            touch
        )
        .is_empty()
    );
    let tap = b.translate(
        PointerEvent::Up {
            pos: pos(12.0, 12.0),
        },
        touch,
    );
    assert_eq!(kinds(tap), ["move", "down", "up"]);

    b.translate(
        PointerEvent::Down {
            pos: pos(10.0, 10.0),
        },
        touch,
    );
    // 押している間に方式が変わっても、離すまではタッチのまま。
    let swipe = b.translate(
        PointerEvent::Move {
            pos: pos(10.0, 40.0),
        },
        PointerMode::Mouse,
    );
    let [PlatformInput::ScrollWheel(e)] = &swipe[..] else {
        panic!("{swipe:?}")
    };
    assert_eq!(e.delta.pixel_delta(px(1.0)), Point::new(px(0.0), px(30.0)));
    assert_eq!(e.touch_phase, TouchPhase::Started);
    assert_eq!(
        e.position,
        Point::new(px(10.0), px(10.0)),
        "target is where it was pressed"
    );
    assert_eq!(
        kinds(b.translate(
            PointerEvent::Up {
                pos: pos(10.0, 40.0)
            },
            touch
        )),
        ["scroll", "move"]
    );
    assert!(!b.pressed);
    // 離した後も慣性で同じ向きに滑り、だんだん止まる。
    let Some(PlatformInput::ScrollWheel(first)) = b.momentum() else {
        panic!("no momentum")
    };
    let dy = |e: &ScrollWheelEvent| f32::from(e.delta.pixel_delta(px(1.0)).y);
    assert!(dy(&first) > 0.0);
    let mut last = dy(&first);
    let mut frames = 0;
    while let Some(PlatformInput::ScrollWheel(e)) = b.momentum() {
        assert!(dy(&e) < last);
        last = dy(&e);
        frames += 1;
    }
    assert!(frames > 10, "does not stop immediately ({frames})");
    assert!(b.momentum().is_none());
}

#[test]
fn touch_long_press_becomes_mouse_press_for_drag() {
    let touch = PointerMode::Touch;
    let mut b = PointerBridge::default();
    let at = Vec2::new(10.0, 10.0);
    assert!(
        b.translate(PointerEvent::Down { pos: at }, touch)
            .is_empty()
    );
    assert!(
        b.translate(PointerEvent::Move { pos: at }, touch)
            .is_empty()
    );
    std::thread::sleep(LONG_PRESS);
    // 動かさずに長押しすると、押した所でマウスの押下になり、押したまま閾値を越えて動かして（ドラッグがすぐ始まる）元へ戻す。
    let held = b.translate(PointerEvent::Move { pos: at }, touch);
    let [
        PlatformInput::MouseMove(_),
        PlatformInput::MouseDown(_),
        PlatformInput::MouseMove(nudge),
        PlatformInput::MouseMove(back),
    ] = &held[..]
    else {
        panic!("{held:?}")
    };
    assert_eq!(nudge.pressed_button, Some(MouseButton::Left));
    assert!(
        f32::from(nudge.position.x) - 10.0 > 2.0,
        "exceeds the GPUI drag threshold"
    );
    assert_eq!(back.position, Point::new(px(10.0), px(10.0)));
    // 以降はマウスのドラッグ（押したままの Move）で、スクロールにはならない。
    let drag = b.translate(
        PointerEvent::Move {
            pos: Vec2::new(10.0, 60.0),
        },
        touch,
    );
    let [PlatformInput::MouseMove(m)] = &drag[..] else {
        panic!("{drag:?}")
    };
    assert_eq!(m.pressed_button, Some(MouseButton::Left));
    let up = b.translate(
        PointerEvent::Up {
            pos: Vec2::new(10.0, 60.0),
        },
        touch,
    );
    assert!(matches!(up[..], [PlatformInput::MouseUp(_)]), "{up:?}");
    assert!(b.momentum().is_none(), "drag has no momentum");
}

#[test]
fn starting_a_stream_clears_old_gpui_hover_and_scroll_momentum() {
    use kernel::desktop::{Frame, Status, Update, mock};
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    let panel = kernel
        .add_panel("stream", Pose::IDENTITY, Vec2::ONE, (1, 1))
        .unwrap();
    let (stream, control) = mock::stream();
    kernel.set_desktop_stream(panel, stream).unwrap();
    control.publish(Update {
        status: Status::Streaming,
        frame: Some(std::sync::Arc::new(Frame::new(1, 1, vec![255; 4]).unwrap())),
    });
    let bridge = PointerBridge {
        sent: Some(Point::default()),
        momentum: Some((Point::default(), Point::new(px(0.0), px(20.0)))),
        ..Default::default()
    };
    let mut bridges = HashMap::from([((panel, Hand::Right), bridge)]);
    let cleanup = pump(&mut kernel, &mut bridges);
    assert!(matches!(
        cleanup.as_slice(),
        [(_, _, PlatformInput::MouseExited(_))]
    ));
    assert!(pump(&mut kernel, &mut bridges).is_empty());
    assert!(bridges[&(panel, Hand::Right)].momentum.is_none());
}
