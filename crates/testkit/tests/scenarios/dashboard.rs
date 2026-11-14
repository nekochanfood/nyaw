use gpui::TestAppContext;
use kernel::vr::Vec3;
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
        |f, o| settings::spec(f, o),
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

/// The wrist can be grabbed only while the dashboard shows (the kit builds the view without `wrist::spec`, so
/// its startup lock and reach are not here; the wrist locks itself once it sees the dashboard hidden).
#[gpui::test]
fn the_wrist_is_grabbable_only_while_the_dashboard_shows(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| wrist(cx));
    kit.step().step();
    assert!(kit.kernel.panel(0).locked);
    press_x_twice(&mut kit);
    // One frame hands the shown dashboard to the wrist, the next applies its unlock.
    kit.step().step();
    assert!(!kit.kernel.panel(0).locked);
    press_x_twice(&mut kit);
    kit.step().step();
    assert!(kit.kernel.panel(0).locked);
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

/// The dashboard shows at the distance and height set in Settings (the height is relative to the head). While it
/// shows, a change moves it there at once; while hidden, it takes effect the next time it shows.
#[gpui::test]
fn dashboard_shows_at_the_set_distance_and_height(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| wrist(cx));
    kit.step().step();
    press_x_twice(&mut kit);
    kit.step();
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");
    let at = |kit: &NoVrKit<Wrist>| kit.kernel.panel(dashboard).pose;
    let home = at(&kit);

    // 0.2m up: it moves straight up, facing the same way.
    kit.kernel.set(Setting::DashboardHeight(0.2));
    kit.step().step();
    let up = at(&kit);
    let moved = up.position - home.position;
    assert!(moved.abs_diff_eq(Vec3::Y * 0.2, 1e-4), "{moved}");
    assert!(up.orientation.abs_diff_eq(home.orientation, 1e-5));

    // 0.5m farther: it moves level, away from the head.
    kit.kernel.set(Setting::DashboardDistance(1.6));
    kit.step().step();
    let far = at(&kit);
    let head = kit.kernel.hmd().position;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let moved = far.position - up.position;
    assert!(
        moved.y.abs() < 1e-4 && (flat(moved).length() - 0.5).abs() < 1e-3,
        "{moved}"
    );
    assert!(flat(far.position - head).length() > flat(up.position - head).length());

    // Hidden, a change leaves it where it is; it shows at the new place next time.
    press_x_twice(&mut kit);
    kit.step();
    assert!(kit.kernel.status(dashboard).hidden);
    kit.kernel.set(Setting::DashboardHeight(0.0));
    kit.kernel
        .set(Setting::DashboardDistance(kernel::DASHBOARD_DISTANCE));
    kit.step().step();
    assert!(
        at(&kit).position.abs_diff_eq(far.position, 1e-5),
        "not while hidden"
    );
    press_x_twice(&mut kit);
    kit.step();
    assert!(!kit.kernel.status(dashboard).hidden);
    assert!(at(&kit).position.abs_diff_eq(home.position, 1e-4));
}
