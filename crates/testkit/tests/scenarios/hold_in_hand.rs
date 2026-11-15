//! Holding a panel in a hand: the navigation bar's hand button opens a popover that picks the left or right hand. The panel
//! then rides on that controller, laid ahead of where its laser comes out, and stands up facing the user when the controller is
//! raised. While it is held, it can be grabbed (moved, turned, resized) only while the dashboard shows; if the dashboard goes
//! away mid-grab, the grab is undone (as with the wrist).

use std::f32::consts::FRAC_PI_2;

use gpui::TestAppContext;
use kernel::PanelCommand;
use testing::Testing;
use testkit::{Action, Device, Driver, Hand, Pose, Quat, Vec2, Vec3, VrKit};

/// Spawned Testing panel, its navigation bar and the hand popover opened from it.
const OWNER: usize = 1;
const BAR: usize = 2;
const MENU: usize = 3;

/// The left controller raised so that its laser points straight up.
fn raised_left() -> Pose {
    Pose::new(
        Vec3::new(-0.2, -0.2, -0.4),
        Quat::from_rotation_x(FRAC_PI_2),
    )
}

/// The right controller, in front of the panel held in the raised left hand.
fn in_front() -> Pose {
    Pose::new(Vec3::new(0.2, -0.3, 0.0), Quat::IDENTITY)
}

/// Spawns a Testing panel and holds it in the left hand through its navigation bar, with the left controller raised.
/// Returns the panel's size before it was held.
fn hold_in_left_hand(kit: &mut VrKit<Testing>) -> Vec2 {
    kit.click("spawn-panel").step();
    let size = kit.kernel.panel(OWNER).size;
    kit.panel(BAR).click("nav-hand").step();
    assert_eq!(kit.kernel.panel(MENU).parent.map(|p| p.panel), Some(OWNER));
    kit.panel(MENU).click("hand-left").step();
    assert!(kit.kernel.panel(MENU).closed, "choosing closes the popover");
    assert!(matches!(
        kit.kernel.panel(OWNER).attached,
        Some((Device::Controller(Hand::Left), _))
    ));
    kit.hand(Hand::Left)
        .move_hand(raised_left())
        .hand(Hand::Right);
    size
}

#[gpui::test]
fn nav_bar_puts_a_panel_in_the_left_hand(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::new(0));
    let before = hold_in_left_hand(&mut kit);
    // The big Testing panel shrinks to a size to hold, keeping its aspect ratio.
    let size = kit.kernel.panel(OWNER).size;
    assert!(size.x < before.x * 0.9, "{size} {before}");
    assert!((size.x / size.y - before.x / before.y).abs() < 1e-4);
    // Standing above the raised controller, upright, its top leaning toward the user and its face turned back to them.
    let hand = raised_left().position;
    let p = kit.kernel.panel(OWNER).pose;
    let (face, up) = (p.orientation * Vec3::Z, p.orientation * Vec3::Y);
    assert!(face.z > 0.6 && up.y > 0.6, "{face} {up}");
    assert!(up.z > 0.6, "leans toward the user: {up}");
    let rise = p.position.y - hand.y;
    assert!(rise > size.y / 4.0, "above the controller: {rise}");
    assert!((p.position.x - hand.x).abs() < 1e-4);
    // It follows the hand.
    let moved = Pose::new(Vec3::new(0.1, 0.0, 0.1), Quat::IDENTITY) * raised_left();
    kit.hand(Hand::Left).move_hand(moved).hand(Hand::Right);
    let q = kit.kernel.panel(OWNER).pose.position;
    assert!(
        q.abs_diff_eq(p.position + Vec3::new(0.1, 0.0, 0.1), 1e-4),
        "{q}"
    );
    // The other hand still operates it.
    kit.panel(OWNER).click("click-me");
    assert_eq!(kit.read(|t| t.clicks), 1);
    // Letting go leaves it where it is.
    kit.panel(BAR).click("nav-hand").step();
    let menu = MENU + 1;
    kit.panel(menu).click("hand-detach").step().step();
    assert_eq!(kit.kernel.panel(OWNER).attached, None);
    assert!(kit.kernel.panel(OWNER).pose.position.abs_diff_eq(q, 1e-4));
    assert!(!kit.kernel.panel(OWNER).locked);
}

#[gpui::test]
fn a_held_panel_is_adjustable_only_while_the_dashboard_shows(cx: &mut TestAppContext) {
    // Panel 0 stands in for the dashboard: screens dock to it while it shows.
    let mut kit = VrKit::new(cx, |_, cx| {
        cx.default_global::<ui::PanelControl>().dock = Some((0, 0.0));
        Testing::new(0)
    });
    hold_in_left_hand(&mut kit);
    let offset = |kit: &VrKit<Testing>| kit.kernel.panel(OWNER).attached.clone().unwrap().1;
    let start = offset(&kit);
    assert!(!kit.kernel.panel(OWNER).locked);

    // With the dashboard hidden, the held panel is locked and does not move when grabbed.
    kit.kernel.apply(0, PanelCommand::Hide(true));
    kit.step().step();
    assert!(kit.kernel.panel(OWNER).locked);
    kit.panel(OWNER).drag_panel(Vec3::new(0.2, 0.1, -0.6));
    assert_eq!(offset(&kit), start);

    // With it shown again, the other hand moves the panel, and it stays there in the left hand.
    // The drag above carried the right hand along; bring it back in front of the panel first.
    kit.kernel.apply(0, PanelCommand::Hide(false));
    kit.step().step();
    assert!(!kit.kernel.panel(OWNER).locked);
    kit.move_hand(in_front())
        .drag_panel(Vec3::new(0.2, 0.1, -0.6));
    let moved = offset(&kit);
    assert!(!moved.position.abs_diff_eq(start.position, 1e-3));
    kit.hand(Hand::Left)
        .move_hand(Pose::IDENTITY)
        .hand(Hand::Right);
    assert!(
        kit.kernel
            .panel(OWNER)
            .pose
            .position
            .abs_diff_eq(moved.position, 1e-4)
    );
    kit.hand(Hand::Left)
        .move_hand(raised_left())
        .hand(Hand::Right);
    let (pose, size) = (kit.kernel.panel(OWNER).pose, kit.kernel.panel(OWNER).size);

    // Grab it again and resize it (trigger held, hand pushed forward), then the dashboard goes away mid-grab: the grab is
    // undone, and the panel is back in the hand where it was, at its old size.
    // The drags above carried the right hand along; bring it back in front of the panel first.
    let res = kit.kernel.panel(OWNER).resolution;
    kit.move_hand(in_front())
        .point_at(Vec2::new(res.0 as f32, res.1 as f32) / 2.0)
        .press(Action::Grab)
        .press(Action::Select);
    for _ in 0..10 {
        let aim = kit
            .kernel
            .controllers()
            .iter()
            .find(|c| c.hand == Hand::Right)
            .unwrap()
            .aim;
        kit.move_hand(Pose::new(
            aim.position + aim.orientation * Vec3::NEG_Z * 0.02,
            aim.orientation,
        ));
    }
    assert!(kit.kernel.panel(OWNER).size.x > size.x * 1.1, "resized");
    kit.kernel.apply(0, PanelCommand::Hide(true));
    kit.step().step();
    assert!(kit.kernel.panel(OWNER).locked);
    assert_eq!(kit.kernel.panel(OWNER).size, size);
    assert_eq!(offset(&kit), moved);
    let p = kit.kernel.panel(OWNER).pose;
    assert!(
        p.position.abs_diff_eq(pose.position, 1e-4),
        "{p:?} {pose:?}"
    );
    kit.release(Action::Select).release(Action::Grab);
    assert_eq!(offset(&kit), moved);
}
