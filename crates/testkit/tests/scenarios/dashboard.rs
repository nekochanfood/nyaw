use gpui::TestAppContext;
use kernel::{Device, DominantHand, Hand, Setting};
use testkit::{Button, Driver, NoVrKit};
use wrist::Wrist;

/// Presses the left hand's X button twice in a row (novr: the X key) and lets the result show up.
/// A double press counts only within `kernel::DOUBLE_PRESS` of real time, and a full step (which draws every
/// panel) can take longer than that in a debug build, so the presses tick only the kernel.
fn press_x_twice(kit: &mut NoVrKit<Wrist>) {
    for held in [true, false, true, false] {
        kit.kernel.subsystems.backend.set_button(Button::X, held);
        kit.kernel.tick();
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
    let hidden = kit
        .read(|w| w.dashboard)
        .expect("dashboard opened at startup");
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

/// The wrist sits on the wrist opposite the dominant hand and moves there by itself when the setting changes.
#[gpui::test]
fn wrist_follows_the_dominant_hand(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| wrist(cx));
    kit.step().step();
    for (dominant, wrist_on) in [
        (DominantHand::Left, Hand::Right),
        (DominantHand::Right, Hand::Left),
    ] {
        kit.kernel.set(Setting::DominantHand(dominant));
        // One frame hands the setting to the wrist, the next applies its attach command.
        kit.step().step();
        let watch = wrist::watch_offset(wrist_on);
        assert_eq!(
            kit.kernel.panel(0).attached,
            Some((Device::Controller(wrist_on), watch))
        );
    }
}
