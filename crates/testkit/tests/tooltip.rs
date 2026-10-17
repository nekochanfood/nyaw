//! E2E for tooltips. Operates the TooltipTest screen with the VR laser: the hovered element's tooltip goes to the hand
//! pointing at it, and the side (above / below the cursor) follows where the hand appears from the head.
//! How it is drawn (slide, grace, resize) is checked in the beam unit tests.

use gpui::TestAppContext;
use kernel::vr::{Quat, Vec3};
use kernel::{Anchor, Hand, Pose};
use testing::TooltipTest;
use testkit::{Driver, VrKit};

fn tooltip(kit: &mut VrKit<TooltipTest>, hand: Hand) -> Option<String> {
    kit.panel_control(|c| c.tooltip.get(&hand).map(|t| t.to_string()))
}

fn anchor(kit: &mut VrKit<TooltipTest>, hand: Hand) -> Anchor {
    kit.panel_control(|c| c.anchor[&hand])
}

#[gpui::test]
fn hovered_element_tooltip_goes_to_the_pointing_hand(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| TooltipTest::default());
    kit.hover("tip-short");
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Save"));
    assert_eq!(
        tooltip(&mut kit, Hand::Left),
        None,
        "the other hand points at nothing"
    );
    // Moving straight to the neighbor swaps the text.
    kit.hover("tip-long");
    assert_eq!(
        tooltip(&mut kit, Hand::Right).as_deref(),
        Some("Open the settings of this panel")
    );
    // Icon-only buttons always carry one.
    kit.hover("tip-icon-gear");
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Settings"));
    kit.hover("no-tip");
    assert_eq!(tooltip(&mut kit, Hand::Right), None);
}

#[gpui::test]
fn two_hands_keep_different_tooltips_on_the_same_panel(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| TooltipTest::default());
    kit.hand(Hand::Right).hover("tip-short");
    kit.hand(Hand::Left).hover("tip-long");
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Save"));
    assert_eq!(
        tooltip(&mut kit, Hand::Left).as_deref(),
        Some("Open the settings of this panel")
    );
    kit.hand(Hand::Left).hover("tip-short");
    assert_eq!(tooltip(&mut kit, Hand::Left).as_deref(), Some("Save"));
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Save"));
    kit.hover("no-tip");
    assert_eq!(tooltip(&mut kit, Hand::Left), None);
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Save"));
}

#[gpui::test]
fn tooltip_side_follows_the_hand_seen_from_the_head(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| TooltipTest::default());
    // The tooltip stays on the side from which the laser approaches.
    kit.hover("tip-short");
    assert_eq!(anchor(&mut kit, Hand::Right), Anchor::Below);
    // Raise the hand above the head.
    kit.move_hand(Pose::new(Vec3::new(0.2, 0.8, 0.0), Quat::IDENTITY))
        .hover("tip-short");
    assert_eq!(anchor(&mut kit, Hand::Right), Anchor::Above);
    assert_eq!(tooltip(&mut kit, Hand::Right).as_deref(), Some("Save"));
}
