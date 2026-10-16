//! Navigation bar: summoned panels get a bar hung under them (title in the middle, close button at the right end),
//! and dialogs opt out.

use gpui::TestAppContext;
use testing::{DialogTest, Testing};
use testkit::{Driver, VrKit};

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
    assert!((b.position - o.position).dot(right).abs() < 1e-4, "centered");

    kit.panel(bar).click("nav-close").step();
    assert!(kit.kernel.panel(owner).closed);
    assert!(kit.kernel.panel(bar).closed);
    assert!(!kit.kernel.panel(0).closed);
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
