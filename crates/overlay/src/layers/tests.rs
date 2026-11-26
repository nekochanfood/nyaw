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

/// 親の面から `z` 手前を要望するポップオーバー。親（ID 0。z = −1）の中央に重ねる。
fn popover(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, key: &str, z: f32) -> PanelId {
    let id = add(vr, ov, key, at(0.0, 0.0, -1.0 + z));
    ov.set_animated(&mut vr.backend, id, false);
    ov.set_parent(&mut vr.backend, id, menu_of_main());
    id
}

fn menu_of_main() -> Option<Parent> {
    Some(Parent {
        panel: 0,
        docked: true,
        dismiss: true,
        ..Default::default()
    })
}

/// 親の面から手前に出ている距離。
fn front(ov: &Overlay, id: PanelId) -> f32 {
    ov.panel(id).pose.position.z + 1.0
}

/// 重なる 2 枚が同じ距離を要望すると後の方が 1 段手前になり、下が閉じれば要望に戻る。
#[test]
fn equal_requests_are_split_and_given_back() {
    let (mut vr, mut ov) = setup();
    let first = popover(&mut vr, &mut ov, "first", 0.002);
    let second = popover(&mut vr, &mut ov, "second", 0.002);
    ov.tick(&mut vr);
    ov.tick(&mut vr);
    assert!((front(&ov, first) - 0.002).abs() < 1e-6);
    assert!((front(&ov, second) - 0.002 - LIFT_STEP).abs() < 1e-6);
    ov.set_animated(&mut vr.backend, first, false);
    ov.close(&mut vr.backend, first);
    ov.tick(&mut vr);
    ov.tick(&mut vr);
    assert!((front(&ov, second) - 0.002).abs() < 1e-6);
}

/// 空いていれば要望どおり。重ならないもの・ずっと奥を要望したものは動かさない。
#[test]
fn requests_are_kept_when_nothing_is_in_the_way() {
    let (mut vr, mut ov) = setup();
    let high = popover(&mut vr, &mut ov, "high", 0.03);
    let behind = popover(&mut vr, &mut ov, "behind", -0.3);
    let aside = add(&mut vr, &mut ov, "aside", at(2.0, 0.0, -1.0));
    ov.set_parent(&mut vr.backend, aside, menu_of_main());
    ov.tick(&mut vr);
    assert!((front(&ov, high) - 0.03).abs() < 1e-6);
    assert!((front(&ov, behind) + 0.3).abs() < 1e-6);
    assert!(front(&ov, aside).abs() < 1e-6);
}

/// ずらした量は要望に混ざらない: 付け直しや掴んで離すのを繰り返しても、下が消えれば元の距離に戻る。
#[test]
fn the_granted_depth_never_becomes_the_request() {
    for carry in [true, false] {
        let (mut vr, mut ov) = setup();
        ov.set_animated(&mut vr.backend, 0, false);
        let under = popover(&mut vr, &mut ov, "under", 0.002);
        let over = popover(&mut vr, &mut ov, "over", 0.002);
        // ダイアログは掴めて、外を押しても閉じない。
        let parent = Parent {
            panel: 0,
            carry,
            ..Default::default()
        };
        ov.set_parent(&mut vr.backend, under, Some(parent));
        ov.set_parent(&mut vr.backend, over, Some(parent));
        ov.tick(&mut vr);
        assert!((front(&ov, over) - 0.002 - LIFT_STEP).abs() < 1e-6);
        for _ in 0..5 {
            ov.set_parent(&mut vr.backend, over, Some(parent));
            for grip in [1.0, 1.0, 0.0] {
                controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", grip)]);
                ov.tick(&mut vr);
                assert_eq!(ov.grabbed(over), grip > 0.0, "carry = {carry}");
            }
        }
        vr.backend.frame.controllers.clear();
        ov.close(&mut vr.backend, under);
        settle(&mut vr, &mut ov);
        let gap = ov.panel(over).pose.position.z - ov.panel(0).pose.position.z;
        assert!((gap - 0.002).abs() < 1e-5, "carry = {carry}: {gap}");
    }
}
