//! The layer test screen (the testing screen's `layer-test`): every kind of sample it opens ends up in the expected
//! draw order, overlapping samples are pushed forward, and the screen can clear them all.

use gpui::TestAppContext;
use testing::{LayerKind, LayerTest};
use testkit::{Driver, VrKit};

/// Draw order of a panel in VR.
fn order(kit: &VrKit<'_, LayerTest>, panel: usize) -> u32 {
    kit.kernel.panel(panel).draw_order()
}

/// How far a panel stands in front of the test screen (panel 0), in meters.
fn front(kit: &VrKit<'_, LayerTest>, panel: usize) -> f32 {
    (kit.kernel.panel(0).pose.inverse() * kit.kernel.panel(panel).pose)
        .position
        .z
}

/// Clicks a button on the test screen and returns the sample it opened.
fn open(kit: &mut VrKit<'_, LayerTest>, button: &'static str) -> usize {
    let before = kit.read(|t| t.samples.len());
    kit.panel(0).click(button).step().step();
    kit.panel(0);
    let samples = kit.read(|t| t.samples.clone());
    assert_eq!(samples.len(), before + 1, "{button}");
    samples[before].1
}

#[gpui::test]
fn overlapping_over_samples_step_forward_and_come_back(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| LayerTest::new(0));
    let first = open(&mut kit, "layer-over");
    let second = open(&mut kit, "layer-over");
    assert!(order(&kit, 0) < order(&kit, first));
    assert!(order(&kit, first) < order(&kit, second));
    assert!((front(&kit, first) - 0.001).abs() < 1e-4);
    assert!((front(&kit, second) - 0.002).abs() < 1e-4);
    // The sample shows the values the layer step granted.
    kit.panel(second).step();
    kit.bounds("layer-readout");

    // Closing the lower one gives the upper one its requested depth back.
    kit.panel(first).click("layer-close").step().step().settle();
    assert!(kit.kernel.panel(first).closed);
    assert!((front(&kit, second) - 0.001).abs() < 1e-4);
}

#[gpui::test]
fn every_kind_is_drawn_in_layer_order(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| LayerTest::new(0));
    // The far panel opens its navigation bar as well; neither may be drawn over the nearer test screen.
    let far = open(&mut kit, "layer-far");
    let far_bar = kit.kernel.nav_bar(far).expect("the far panel has a bar");
    assert!(order(&kit, far) < order(&kit, far_bar));
    assert!(order(&kit, far_bar) < order(&kit, 0));

    let over = open(&mut kit, "layer-over");
    let hud = open(&mut kit, "layer-hud");
    let popover = open(&mut kit, "layer-popover");
    assert!(order(&kit, 0) < order(&kit, hud));
    assert!(order(&kit, hud) < order(&kit, popover));
    assert!(order(&kit, popover) < order(&kit, over));

    // The first press outside a popover only closes it.
    kit.panel(0).click("layer-dialog").step().step().settle();
    assert!(kit.kernel.panel(popover).closed);

    // A dialog blocks the screen, so it is opened last.
    let dialog = open(&mut kit, "layer-dialog");
    assert!(order(&kit, hud) < order(&kit, dialog));
    assert!(order(&kit, dialog) < order(&kit, over));
    let kinds = kit.read(|t| t.samples.iter().map(|s| s.0).collect::<Vec<_>>());
    assert_eq!(kinds.last(), Some(&LayerKind::Dialog));

    // With the dialog closed, "close all" clears the rest, including the HUD that cannot be clicked.
    kit.panel(dialog)
        .click("layer-close")
        .step()
        .step()
        .settle();
    kit.panel(0).click("layer-clear").step().step().settle();
    for panel in [far, far_bar, over, hud, popover, dialog] {
        assert!(kit.kernel.panel(panel).closed, "{panel}");
    }
}
