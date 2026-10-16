use gpui::TestAppContext;
use testkit::{Button, Driver, NoVrKit};
use wrist::Wrist;

/// Presses the left hand's X button twice in a row (novr: the X key) and lets the summoned panel open.
fn press_x_twice(kit: &mut NoVrKit<Wrist>) {
    for _ in 0..2 {
        kit.press(Button::X).release(Button::X);
    }
    kit.step();
}

/// novr: pressing the X key twice opens the dashboard with its sound, and twice again closes it.
#[gpui::test]
fn x_key_twice_opens_and_closes_the_dashboard(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| Wrist::new(0, cx));
    press_x_twice(&mut kit);
    assert_eq!(kit.kernel.status(0).dashboard, 1);
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");
    assert!(!kit.kernel.panel(dashboard).closed);
    assert_eq!(kit.kernel.status(0).audio.playing, 1, "opening sound");

    press_x_twice(&mut kit);
    assert_eq!(kit.kernel.status(0).dashboard, 2);
    assert_eq!(kit.read(|w| w.dashboard), None);
    assert!(kit.kernel.panel(dashboard).closed);
}
