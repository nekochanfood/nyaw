//! Skeletal input: the hand skeleton the runtime reports reaches the skeleton test screen, and a pinch between the thumb and each finger lights up.

use gpui::TestAppContext;
use kernel::vr::{FingerTracking, HandSkeleton, JOINT_COUNT, TIPS};
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG, SkeletonTest};
use testkit::{Driver, Hand, Pose, Vec3, VrKit};

/// A right hand whose fingertips are spread 4 cm apart along X, with fingertip `pinched` (an index into [`TIPS`]) on the thumb tip.
fn hand(pinched: Option<usize>) -> HandSkeleton {
    let mut joints = [Pose::IDENTITY; JOINT_COUNT];
    for (j, joint) in joints.iter_mut().enumerate() {
        joint.position = Vec3::new(j as f32 * 0.01, 1.0, -0.5);
    }
    for (i, &tip) in TIPS.iter().enumerate() {
        joints[tip].position = Vec3::new(i as f32 * 0.04, 1.0, -0.5);
    }
    if let Some(i) = pinched {
        joints[TIPS[i]].position = joints[TIPS[0]].position + Vec3::new(0.0, 0.005, 0.0);
    }
    HandSkeleton {
        hand: Hand::Right,
        joints,
        tracking: FingerTracking::Full,
    }
}

#[gpui::test]
fn pinch_lights_only_the_touching_finger(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| SkeletonTest::new(0, cx));
    kit.kernel.subsystems.backend.frame.hands = vec![hand(None)];
    kit.step().step();
    for id in [
        "pinch-right-index",
        "pinch-right-middle",
        "pinch-left-index",
    ] {
        assert_eq!(kit.color_at(id), BUTTON_BG, "{id}");
    }

    kit.kernel.subsystems.backend.frame.hands = vec![hand(Some(2))];
    kit.step().step();
    assert_eq!(kit.color_at("pinch-right-middle"), BUTTON_ACTIVE_BG);
    for id in [
        "pinch-right-index",
        "pinch-right-ring",
        "pinch-right-little",
        "pinch-left-middle",
    ] {
        assert_eq!(kit.color_at(id), BUTTON_BG, "{id}");
    }

    // The bare hand is driven by its skeleton: pinching thumb and index clicks, which the diagnostics show.
    kit.kernel.subsystems.backend.frame.hands = vec![hand(Some(1))];
    kit.step().step();
    for id in [
        "input-source-right",
        "hand-click-right",
        "pinch-right-index",
    ] {
        assert_eq!(kit.color_at(id), BUTTON_ACTIVE_BG, "{id}");
    }
    assert_eq!(kit.color_at("input-source-left"), BUTTON_BG);

    // Losing the skeleton turns the pinch off.
    kit.kernel.subsystems.backend.frame.hands.clear();
    kit.step().step();
    assert_eq!(kit.color_at("pinch-right-middle"), BUTTON_BG);
}
