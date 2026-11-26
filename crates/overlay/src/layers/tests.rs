use crate::test_util::*;
use crate::*;

fn add(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, key: &str, pose: Pose) -> PanelId {
    ov.add_panel(&mut vr.backend, key, pose, Vec2::splat(0.4), (40, 40))
        .unwrap()
}

fn sort(vr: &Subsystems<MockBackend>, key: &str) -> u32 {
    let o = vr.backend.overlays.iter().find(|o| o.key == key);
    o.unwrap().sort
}

/// 奥のパネルに付くものは、手前にある無関係なパネルより上に描かない。
#[test]
fn a_far_panels_nav_bar_stays_under_a_nearer_panel() {
    let (mut vr, mut ov) = setup();
    let far = add(&mut vr, &mut ov, "far", at(0.0, 0.0, -2.0));
    let bar = add(&mut vr, &mut ov, "bar", at(0.0, 0.0, -2.0));
    ov.set_nav_bar(&mut vr.backend, bar, far);
    ov.tick(&mut vr);
    assert!(sort(&vr, "far") < sort(&vr, "bar"));
    assert!(sort(&vr, "bar") < sort(&vr, "main"));
}

/// 同じ親に付くナビバーとポップオーバーは別の順になり、後から開くポップオーバーが上。
#[test]
fn a_popover_is_drawn_over_its_siblings_nav_bar() {
    let (mut vr, mut ov) = setup();
    let menu = add(&mut vr, &mut ov, "menu", at(0.0, 0.0, -0.97));
    let bar = add(&mut vr, &mut ov, "bar", at(0.0, -0.5, -1.0));
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    ov.set_parent(
        &mut vr.backend,
        menu,
        Some(Parent {
            panel: 0,
            docked: true,
            dismiss: true,
            ..Default::default()
        }),
    );
    ov.tick(&mut vr);
    assert!(sort(&vr, "main") < sort(&vr, "bar"));
    assert!(sort(&vr, "bar") < sort(&vr, "menu"));
}

/// 距離がわずかに行き来しても前後は入れ替わらず、はっきり離れたら入れ替わる。
#[test]
fn near_ties_keep_their_order() {
    let (mut vr, mut ov) = setup();
    let other = add(&mut vr, &mut ov, "other", at(0.0, 0.0, -1.005));
    let over_main = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, z: f32| {
        ov.set_pose(other, at(0.0, 0.0, z));
        ov.tick(vr);
        sort(vr, "other") > sort(vr, "main")
    };
    assert!(!over_main(&mut ov, &mut vr, -1.005));
    assert!(!over_main(&mut ov, &mut vr, -0.995));
    assert!(over_main(&mut ov, &mut vr, -0.95));
    assert!(over_main(&mut ov, &mut vr, -1.005));
    assert!(!over_main(&mut ov, &mut vr, -1.05));
}

/// 相手に重ねるものは、距離に関係なく相手のすぐ上にまとまる（子の子は子のすぐ上）。
#[test]
fn what_lies_on_a_panel_stays_right_above_it() {
    let (mut vr, mut ov) = setup();
    // 根元より奥にある子でも、根元より上。
    let dialog = add(&mut vr, &mut ov, "dialog", at(0.0, 0.0, -3.0));
    let bar = add(&mut vr, &mut ov, "bar", at(0.0, 0.0, -3.0));
    let near = add(&mut vr, &mut ov, "near", at(0.0, 0.0, -0.5));
    ov.set_parent(
        &mut vr.backend,
        dialog,
        Some(Parent {
            panel: 0,
            ..Default::default()
        }),
    );
    ov.set_nav_bar(&mut vr.backend, bar, dialog);
    ov.tick(&mut vr);
    let order = ["main", "dialog", "bar", "near"].map(|k| sort(&vr, k));
    assert!(order.is_sorted_by(|a, b| a < b), "{order:?}");
    assert_eq!(ov.panel(near).parent, None);
}

/// 同じ親の積む子は、後から開いた画面より上（画面の出入りで重なっても入れ替わらない）。
#[test]
fn a_stacked_child_is_drawn_over_later_screens_of_the_same_parent() {
    let (mut vr, mut ov) = setup();
    let toast = add(&mut vr, &mut ov, "toast", at(0.0, 1.0, -1.0));
    let screen = add(&mut vr, &mut ov, "screen", at(0.0, 0.5, -0.8));
    for (id, stack) in [(toast, true), (screen, false)] {
        ov.set_parent(
            &mut vr.backend,
            id,
            Some(Parent {
                panel: 0,
                docked: true,
                slot: Some(0.0),
                stack,
                ..Default::default()
            }),
        );
    }
    ov.tick(&mut vr);
    assert!(sort(&vr, "screen") < sort(&vr, "toast"));
}
