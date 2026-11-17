//! OS notifications on the VR path (mock runtime, whole frame loop): one toast panel flows from the HUD to the wrist
//! to the dashboard and never goes back; the HUD lands inside every headset's view; cards on the wrist and the
//! dashboard are clickable (the HUD is not).
//!
//! The toast's timing is real time (the HUD shows for 6s, fades), so this is its own binary rather than part of
//! `scenarios` (where every core runs at once and a slow step could outlast the HUD).
//! `cargo test -p testkit --test notifications`

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui::TestAppContext;
use kernel::subsystems::notifications::{self as notes, Notification, Notifications, Request};
use kernel::vr::{Fov, RawController, Vec3};
use kernel::{Device, Hand, NotificationAnchor, PanelCommand, PanelId, Pose, Setting, Vec2};
use testkit::{Action, Driver, VrKit};
use wrist::Wrist;

const PROFILE: &str = "valve/index_controller";

fn wrist(cx: &mut gpui::Context<Wrist>) -> Wrist {
    Wrist::new(
        0,
        |f, o| testing::spec(f, o),
        |f, o| settings::spec(f, o),
        cx,
    )
}

fn note() -> Notification {
    Notification {
        id: 7,
        app: notes::App {
            id: "Chat".into(),
            name: "Chat".into(),
            icon: None,
        },
        title: "kissa: are you joining tonight?".into(),
        body: "We start at nine.".into(),
        arrived: std::time::SystemTime::now(),
        url: None,
        actions: vec![notes::Action {
            label: "Reply".into(),
            url: None,
        }],
    }
}

/// The left hand hanging down (the watch is neither in front of the face nor turned toward it).
fn lowered() -> Pose {
    Pose::new(Vec3::new(-0.2, -0.7, -0.1), kernel::vr::Quat::IDENTITY)
}

/// The left hand raised so the watch is in front of the face and turned toward the head (looking at it).
fn raised() -> Pose {
    let at = Vec3::new(0.0, -0.25, -0.4);
    let watch = Pose::new(
        at,
        kernel::vr::Quat::from_rotation_arc(Vec3::Z, -at.normalize()),
    );
    watch * wrist::watch_offset(Hand::Left).inverse()
}

/// Lets motion finish: fades run on real time, glides on frames (each frame counts at most 1/30s, so a 300ms glide
/// needs about 10 frames even when frames are slow).
fn settle(kit: &mut VrKit<Wrist>) {
    let start = Instant::now();
    let mut steps = 0;
    while steps < 12 || start.elapsed() < Duration::from_millis(500) {
        kit.step();
        steps += 1;
    }
}

/// One test at a time: the HUD shows for 10s of real time, and parallel tests slow every frame down.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Presses the left hand's shortcut (X) twice. A full step can take longer than the double-press window in a debug
/// build, so the presses tick only the kernel (as in `dashboard.rs`).
fn press_x_twice(kit: &mut VrKit<Wrist>) {
    let name = kernel::subsystems::input::source(PROFILE, Action::Shortcut).unwrap();
    for held in [true, false, true, false] {
        kit.kernel.subsystems.backend.frame.controllers = vec![RawController {
            hand: Hand::Left,
            profile: PROFILE.into(),
            aim: raised(),
            inputs: if held {
                vec![(name.into(), 1.0)]
            } else {
                vec![]
            },
        }];
        kit.kernel.tick();
    }
    kit.step();
}

/// Where the notifications source receives requests (clicks: notification id, button index).
type Clicks = Receiver<Request>;

/// The wrist worn on the lowered left hand, a headset with `fov`, the HUD at `anchor`, and one notification that
/// arrives after start. Returns the toast's panel and where clicks are delivered.
fn arrive(
    cx: &mut TestAppContext,
    fov: Fov,
    anchor: NotificationAnchor,
) -> (VrKit<'_, Wrist>, PanelId, Clicks) {
    let mut kit = VrKit::new(cx, |_, cx| wrist(cx));
    kit.kernel.subsystems.backend.fov = Some(fov);
    kit.kernel.set(Setting::NotificationAnchor(anchor));
    let offset = wrist::watch_offset(Hand::Left);
    kit.kernel.apply(
        0,
        PanelCommand::Attach(Device::Controller(Hand::Left), offset),
    );
    kit.hand(Hand::Left).move_hand(lowered()).hand(Hand::Right);
    kit.step().step();
    let (notifications, clicks) = Notifications::fixed(vec![note()]);
    kit.kernel.set_notifications(notifications);
    settle(&mut kit);
    let toast = kit.read(|w| w.toast).expect("the wrist opens the toast");
    (kit, toast, clicks)
}

/// The angles (left/right, up/down; radians from straight ahead) of the HUD's corners as the head sees them.
fn hud_corners(kit: &mut VrKit<Wrist>, toast: PanelId) -> Vec<(f32, f32)> {
    let b = kit.panel(toast).bounds("toast-hud-7");
    let (l, t, r, bo) = (
        f32::from(b.left()),
        f32::from(b.top()),
        f32::from(b.right()),
        f32::from(b.bottom()),
    );
    let panel = kit.kernel.panel(toast).clone();
    kit.panel(0);
    [(l, t), (r, t), (l, bo), (r, bo)]
        .into_iter()
        .map(|(x, y)| {
            let q = panel.pixel_to_world(Vec2::new(x, y));
            (q.x.atan2(-q.z), q.y.atan2(-q.z))
        })
        .collect()
}

/// The HUD toast is attached to the head, lets the laser through, and its visible pill is inside both eyes' view of
/// the headset, inside the safe area, and touches the safe area's edge on the anchored side.
fn hud_lands_inside(cx: &mut TestAppContext, fov: Fov, anchor: NotificationAnchor) {
    let _serial = serial();
    let (mut kit, toast, _) = arrive(cx, fov, anchor);
    let status = kit.kernel.status(toast);
    assert_eq!(status.attached, Some(Device::Hmd));
    assert!(!status.hidden && kit.kernel.panel(toast).pass_through);
    let safe = fov.safe();
    // Facing the eye turns the outer edge out by about 1°, within the safe area's margin.
    let slack = 1.5_f32.to_radians();
    let corners = hud_corners(&mut kit, toast);
    for &(h, v) in &corners {
        assert!(
            h > -fov.left && h < fov.right && v > -fov.down && v < fov.up,
            "{h} {v}"
        );
        assert!(
            h > -safe.left - slack && h < safe.right + slack,
            "{anchor:?}: {h}"
        );
        assert!(
            v > -safe.down - slack && v < safe.up + slack,
            "{anchor:?}: {v}"
        );
    }
    let (sx, sy) = anchor.side();
    let (h, v): (Vec<f32>, Vec<f32>) = corners.into_iter().unzip();
    let reach = |s: f32, a: &[f32], neg: f32, pos: f32| {
        let edge = if s > 0.0 {
            a.iter().copied().fold(f32::MIN, f32::max) - pos
        } else {
            -neg - a.iter().copied().fold(f32::MAX, f32::min)
        };
        assert!(edge.abs() < slack, "{anchor:?}: {edge} from the edge");
    };
    reach(sx, &h, safe.left, safe.right);
    reach(sy, &v, safe.down, safe.up);
}

fn deg(d: f32) -> f32 {
    d.to_radians()
}

/// A narrow headset (both eyes overlap 35° to each side).
#[gpui::test]
fn the_hud_lands_inside_a_narrow_headset(cx: &mut TestAppContext) {
    hud_lands_inside(
        cx,
        Fov::even(deg(35.0), deg(35.0)),
        NotificationAnchor::TopRight,
    );
}

/// A Quest-like headset (asymmetric: more down than up).
#[gpui::test]
fn the_hud_lands_inside_a_quest_like_headset(cx: &mut TestAppContext) {
    let fov = Fov {
        left: deg(42.0),
        right: deg(41.0),
        up: deg(44.0),
        down: deg(50.0),
    };
    hud_lands_inside(cx, fov, NotificationAnchor::BottomLeft);
}

/// An Index-like headset.
#[gpui::test]
fn the_hud_lands_inside_an_index_like_headset(cx: &mut TestAppContext) {
    hud_lands_inside(
        cx,
        Fov::even(deg(50.0), deg(52.0)),
        NotificationAnchor::TopLeft,
    );
}

/// A wide headset: the toast still stays where the eyes can read it (capped), not at the blurry edge.
#[gpui::test]
fn the_hud_lands_inside_a_wide_headset(cx: &mut TestAppContext) {
    hud_lands_inside(
        cx,
        Fov::even(deg(75.0), deg(55.0)),
        NotificationAnchor::BottomRight,
    );
}

/// Not looking at the watch, a notification shows at the edge of the view (not clickable). Raising the watch moves
/// the same panel onto it (out where it was, then in on the watch), opening the dashboard moves it on top of the
/// dashboard. Clicking it there opens the notification center with that notification expanded, the toast steps aside,
/// and the notification's buttons in the center reach the app. After closing the dashboard it never comes back, not even
/// to the watch.
#[gpui::test]
fn a_notification_flows_from_the_hud_to_the_wrist_to_the_dashboard(cx: &mut TestAppContext) {
    let _serial = serial();
    let (mut kit, toast, clicks) = arrive(cx, Fov::NARROW, NotificationAnchor::Top);
    assert_eq!(kit.kernel.status(toast).attached, Some(Device::Hmd));
    assert!(
        kit.panel(toast).try_bounds("toast-7").is_none(),
        "the HUD has no card to click"
    );
    kit.panel(0);
    let hud = kit.kernel.panel(toast).pose.position;

    // Raise the watch: the toast first disappears where it is (on the HUD), and only then appears on the watch. It is
    // never seen anywhere in between (moving across the view would block it).
    kit.hand(Hand::Left).move_hand(raised()).hand(Hand::Right);
    assert!(kit.kernel.status(0).gazed, "looking at the watch");
    let start = Instant::now();
    while kit.kernel.status(toast).parent != Some(0) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "moves to the watch"
        );
        let at = kit.kernel.panel(toast).pose.position;
        assert!(
            kit.kernel.status(toast).hidden || at.distance(hud) < 1e-3,
            "seen only on the HUD before it moves: {at} vs {hud}"
        );
        kit.step();
    }
    settle(&mut kit);
    let (watch, card) = (kit.kernel.panel(0).clone(), kit.kernel.panel(toast).clone());
    assert_eq!(kit.kernel.status(toast).parent, Some(0));
    assert!(!card.pass_through && !kit.kernel.status(toast).hidden);
    let above = watch.pose.inverse().transform_point(card.pose.position);
    assert!(above.y > 0.0, "stands on top of the watch: {above}");
    assert!(card.pose.position.distance(watch.pose.position) < watch.size.y + card.size.y);
    assert!(
        kit.panel(toast).try_bounds("toast-7").is_some(),
        "a card on the wrist"
    );
    kit.panel(0);

    // Open the dashboard: the toast moves on top of it.
    press_x_twice(&mut kit);
    settle(&mut kit);
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");
    assert!(!kit.kernel.status(dashboard).hidden);
    assert_eq!(kit.kernel.status(toast).parent, Some(dashboard));
    let (bar, card) = (
        kit.kernel.panel(dashboard).clone(),
        kit.kernel.panel(toast).clone(),
    );
    let above = bar.pose.inverse().transform_point(card.pose.position);
    assert!(
        above.y > bar.size.y / 2.0,
        "stands on top of the dashboard: {above}"
    );

    // The card shows no buttons (details live in the notification center). Clicking it opens the center with this
    // notification expanded, and the toast steps aside (the center lists it). The kit's watch is a full-size panel, so
    // lower it out of the right hand's way first (the dashboard keeps the toast).
    kit.hand(Hand::Left).move_hand(lowered()).hand(Hand::Right);
    assert_eq!(kit.kernel.status(toast).parent, Some(dashboard));
    kit.panel(toast).hover("toast-7");
    kit.step();
    assert!(kit.try_bounds("toast-action-7-0").is_none(), "no buttons");
    kit.click("toast-7");
    settle(&mut kit);
    let center = kit
        .find("notification-action-7-0")
        .expect("the center opens with the notification expanded");
    assert_eq!(kit.kernel.status(center).parent, Some(dashboard));
    assert!(kit.kernel.status(toast).hidden, "the toast steps aside");
    // Its button reaches the app (with the button's index).
    kit.panel(center).click("notification-action-7-0");
    assert_eq!(clicks.try_recv(), Ok(Request::Activate(7, Some(0))));

    // Close the dashboard and look at the watch again: it was seen on the dashboard, so it never comes back.
    kit.panel(0);
    press_x_twice(&mut kit);
    settle(&mut kit);
    assert!(kit.kernel.status(toast).hidden);
}

/// Clicking a card on the watch while the dashboard is hidden shows the dashboard with the notification center docked
/// in it (not floating beside it), and the clicked card goes away (it is read).
#[gpui::test]
fn a_card_on_the_wrist_opens_the_notification_center_in_the_dashboard(cx: &mut TestAppContext) {
    let _serial = serial();
    let (mut kit, toast, _) = arrive(cx, Fov::NARROW, NotificationAnchor::Top);
    kit.hand(Hand::Left).move_hand(raised()).hand(Hand::Right);
    let start = Instant::now();
    while kit.kernel.status(toast).parent != Some(0) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "moves to the watch"
        );
        kit.step();
    }
    settle(&mut kit);
    let dashboard = kit.read(|w| w.dashboard).expect("opened hidden at startup");
    assert!(kit.kernel.status(dashboard).hidden);
    kit.panel(toast).click("toast-7");
    settle(&mut kit);
    assert!(!kit.kernel.status(dashboard).hidden, "the dashboard shows");
    let center = kit
        .find("notification-action-7-0")
        .expect("the center opens with the notification expanded");
    assert_eq!(
        kit.kernel.status(center).parent,
        Some(dashboard),
        "docked in the dashboard"
    );
    assert!(
        kit.kernel.status(toast).hidden,
        "the clicked card goes away"
    );
}
