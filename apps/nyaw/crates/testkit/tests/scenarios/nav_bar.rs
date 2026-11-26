//! Navigation bar: summoned panels get a bar hung under them (title in the middle, close button at the right end),
//! and dialogs opt out.

use gpui::TestAppContext;
use testing::{DialogTest, Testing};
use testkit::{Driver, Pose, Quat, Vec2, Vec3, VrKit};

#[gpui::test]
fn summoned_panel_gets_a_bar_that_closes_it(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::new(0));
    kit.click("spawn-panel").step();
    let (owner, bar) = (1, 2);
    assert_eq!(kit.kernel.panel(bar).nav_bar_of, Some(owner));
    // Hung under the owner, centered left to right.
    let (o, b) = (kit.kernel.panel(owner).pose, kit.kernel.panel(bar).pose);
    let up = o.orientation * testkit::Vec3::Y;
    assert!((b.position - o.position).dot(up) < 0.0, "below the owner");
    let right = o.orientation * testkit::Vec3::X;
    assert!(
        (b.position - o.position).dot(right).abs() < 1e-4,
        "centered"
    );

    kit.panel(bar).click("nav-close").step();
    assert!(kit.kernel.panel(owner).closed);
    assert!(kit.kernel.panel(bar).closed);
    assert!(!kit.kernel.panel(0).closed);
}

/// A bar nobody points at fades away and gives its runtime overlay back, but the app keeps the bar itself (panel and
/// view): pointing at the owner brings it back on a new overlay, and its buttons still work.
#[gpui::test]
fn idle_bar_gives_its_overlay_back_and_returns_when_the_owner_is_pointed(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::new(0));
    kit.click("spawn-panel").step();
    let (owner, bar) = (1, 2);
    let overlay = kit.kernel.panel(bar).overlay_id();
    // Point at nothing (straight up), and wait until the line has lingered and then faded out (real time).
    let up = Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::Y);
    kit.move_hand(Pose::new(Vec3::ZERO, up));
    for wait in [1.6, 1.1] {
        std::thread::sleep(std::time::Duration::from_secs_f32(wait));
        kit.step();
    }
    assert!(
        kit.kernel.subsystems.backend.overlays[overlay.0 as usize].destroyed,
        "the runtime overlay is given back"
    );
    assert!(!kit.kernel.panel(bar).closed, "the bar itself is kept");

    let res = kit.kernel.panel(owner).resolution;
    kit.panel(owner)
        .point_at(Vec2::new(res.0 as f32, res.1 as f32) / 2.0);
    let again = kit.kernel.panel(bar).overlay_id();
    assert_ne!(again, overlay, "comes back on a new overlay");
    assert!(!kit.kernel.subsystems.backend.overlays[again.0 as usize].destroyed);

    kit.panel(bar).click("nav-close").step();
    assert!(kit.kernel.panel(owner).closed);
}

#[gpui::test]
fn dialogs_have_no_bar(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| DialogTest::new(0));
    kit.click("open-dialog").step().step();
    assert_eq!(
        kit.kernel.subsystems.backend.overlays.len(),
        2,
        "only the dialog was opened"
    );
}
