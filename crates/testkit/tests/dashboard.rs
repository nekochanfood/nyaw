use gpui::TestAppContext;
use testkit::{Button, Driver, NoVrKit};
use wrist::Wrist;

/// Presses the left hand's X button twice in a row (novr: the X key) and lets the result show up.
fn press_x_twice(kit: &mut NoVrKit<Wrist>) {
    for _ in 0..2 {
        kit.press(Button::X).release(Button::X);
    }
    kit.step();
}

fn wrist(cx: &mut gpui::Context<Wrist>) -> Wrist {
    Wrist::new(
        0,
        |f, o| testing::spec(f, o),
        |f, o| testing::settings_spec(f, o),
        cx,
    )
}

/// novr: the dashboard opens hidden at startup. Pressing the X key twice shows it with its sound; twice again
/// hides it (it stays open, so it comes back as it was), and twice more shows it again.
#[gpui::test]
fn x_key_twice_opens_hides_and_shows_the_dashboard(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| wrist(cx));
    // One frame opens it, the next applies its first commands.
    kit.step().step();
    let hidden = kit.read(|w| w.dashboard).expect("dashboard opened at startup");
    assert!(kit.kernel.status(hidden).hidden);
    press_x_twice(&mut kit);
    assert_eq!(kit.kernel.status(0).double_press, 1);
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");
    assert!(!kit.kernel.panel(dashboard).closed);
    // The new panel's commands run on the next frame.
    kit.step();
    assert_eq!(
        kit.kernel.status(dashboard).audio.playing,
        1,
        "opening sound from the dashboard"
    );
    assert_eq!(kit.kernel.status(0).audio.playing, 0, "not from the wrist");

    press_x_twice(&mut kit);
    kit.step();
    assert_eq!(kit.kernel.status(0).double_press, 2);
    assert_eq!(kit.read(|w| w.dashboard), Some(dashboard));
    assert!(!kit.kernel.panel(dashboard).closed, "hidden, not closed");
    assert!(kit.kernel.status(dashboard).hidden);
    assert_eq!(kit.kernel.status(0).audio.playing, 0, "not from the wrist");

    press_x_twice(&mut kit);
    kit.step();
    assert!(!kit.kernel.status(dashboard).hidden);
    assert_eq!(kit.read(|w| w.dashboard), Some(dashboard), "not reopened");
}
