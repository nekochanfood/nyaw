use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use kernel::subsystems::notifications::{Action, App};
use std::time::SystemTime;

const TOAST: PanelId = 0;
const WRIST: PanelId = 5;
const BAR: PanelId = 7;

fn note(id: u32, url: Option<&str>) -> Arc<Notification> {
    Arc::new(Notification {
        id,
        app: App {
            id: "Chat".into(),
            name: "Chat".into(),
            icon: None,
        },
        title: format!("Title {id}"),
        body: "Body".into(),
        arrived: SystemTime::now(),
        url: url.map(Into::into),
        actions: vec![
            Action {
                label: "Mute".into(),
                url: None,
            },
            Action {
                label: "Site".into(),
                url: Some("https://example.com/".into()),
            },
        ],
    })
}

fn set(cx: &mut VisualTestContext, f: impl FnOnce(&mut PanelControl)) {
    cx.update(|window, cx| {
        f(cx.default_global::<PanelControl>());
        window.refresh();
    });
    cx.run_until_parked();
}

fn take(cx: &mut VisualTestContext, id: PanelId) -> Vec<PanelCommand> {
    cx.update(|_, cx| {
        let state = cx.default_global::<PanelControl>().panels.entry(id);
        std::mem::take(&mut state.or_default().commands)
    })
}

fn place(view: &gpui::Entity<Toast>, cx: &mut VisualTestContext) -> Place {
    view.read_with(cx, |t, _| t.place())
}

fn fits(commands: &[PanelCommand]) -> bool {
    commands
        .iter()
        .any(|c| matches!(c, PanelCommand::FitHeight(_)))
}

/// 時計を見ていなければ視界の端に出て、時計を見ると手首へ、ダッシュボードを開くとダッシュボードへ、見えたまま動く
/// （隠さない）。ダッシュボードを閉じると片付き、ダッシュボードで見たものは手首にも戻らない。
#[gpui::test]
fn the_toast_flows_from_the_hud_to_the_wrist_to_the_dashboard(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| c.notifications = Arc::from([note(1, None)]));
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Hud);
    assert!(
        matches!(c[0], PanelCommand::Place(_))
            && matches!(c[1], PanelCommand::Attach(Device::Hmd, _)),
        "{c:?}"
    );
    assert!(c.contains(&PanelCommand::PassThrough(true)) && fits(&c));
    assert_eq!(c.last(), Some(&PanelCommand::Hide(false)));
    assert!(cx.debug_bounds("toast-hud-1").is_some());
    assert!(cx.debug_bounds("toast-1").is_none(), "HUD は押せない");

    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Wrist);
    assert!(
        matches!(
            c[0],
            PanelCommand::Parent(Some(Parent {
                panel: WRIST,
                slot: Some(_),
                ..
            }))
        ),
        "{c:?}"
    );
    assert!(c.contains(&PanelCommand::PassThrough(false)) && fits(&c));
    assert!(
        !c.iter().any(|c| matches!(c, PanelCommand::Hide(_))),
        "見えたまま動く"
    );
    assert!(cx.debug_bounds("toast-1").is_some());

    set(cx, |c| {
        c.dock = Some((BAR, 0.5));
        c.panels.entry(BAR).or_default();
    });
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Dashboard);
    let to_bar = Parent {
        panel: BAR,
        carry: true,
        docked: true,
        slot: Some(0.5),
        stack: true,
        ..Default::default()
    };
    assert_eq!(c[0], PanelCommand::Parent(Some(to_bar)));
    assert!(
        !c.iter().any(|c| matches!(c, PanelCommand::Hide(_))),
        "見えたまま動く"
    );

    set(cx, |c| c.panels.get_mut(&BAR).unwrap().status.hidden = true);
    let c = take(cx, TOAST);
    assert_eq!(c, [PanelCommand::Hide(true), PanelCommand::Parent(None)]);
    assert_eq!(place(&view, cx), Place::Hidden);
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(
        place(&view, cx),
        Place::Hidden,
        "ダッシュボードで見たものは手首に戻らない"
    );
}

/// 手首から HUD のように戻る向きへは動かさず、隠して消えきってから出し直す。
#[gpui::test]
fn going_back_hides_first(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None)]);
        c.panels.entry(WRIST).or_default().status.gazed = true;
    });
    assert_eq!(place(&view, cx), Place::Wrist);
    take(cx, TOAST);
    // 時計から目を離して片付いた後に、新しい通知が届く。
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = false
    });
    view.update(cx, |t, _| t.flow = Flow::default());
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None)])
    });
    assert_eq!(
        take(cx, TOAST),
        [PanelCommand::Hide(true), PanelCommand::Parent(None)]
    );
    assert_eq!(place(&view, cx), Place::Hidden);
    view.update(cx, |t, _| t.hidden_at = Some(Instant::now() - REAPPEAR));
    set(cx, |_| {});
    assert_eq!(place(&view, cx), Place::Hud);
    assert_eq!(take(cx, TOAST).last(), Some(&PanelCommand::Hide(false)));
}

/// 手首・ダッシュボードの通知は押すとアプリへ届き、URL のものは確かめてから開く。ホバーするとボタンが出る。
/// 押した通知はもう出さない。
#[gpui::test]
fn cards_open_the_app_or_the_url(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, Some("https://example.com/2"))]);
        c.panels.entry(WRIST).or_default().status.gazed = true;
    });
    take(cx, TOAST);
    assert!(
        cx.debug_bounds("toast-action-1-0").is_none(),
        "ホバーするまでボタンは出さない"
    );
    let at = cx.debug_bounds("toast-1").unwrap().center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("toast-action-1-0").is_some());
    let mute = cx.debug_bounds("toast-action-1-0").unwrap().center();
    cx.simulate_click(mute, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        take(cx, TOAST),
        [PanelCommand::Notification {
            id: 1,
            action: Some(0)
        }],
        "ボタンだけが押される"
    );
    assert!(cx.debug_bounds("toast-1").is_none(), "押した通知は出さない");
    let at = cx.debug_bounds("toast-2").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(
        !take(cx, TOAST)
            .iter()
            .any(|c| matches!(c, PanelCommand::Notification { .. })),
        "URL はアプリへは届けない"
    );
    let urls = cx.update(|window, cx| design::link::take(window, cx));
    assert_eq!(urls, ["https://example.com/2"]);
    assert_eq!(place(&view, cx), Place::Hidden);
}

/// HUD の中身の外側の端は、どの端でも両目に確実に映る範囲の端に来て、その外へは出ない（UI スケールが大きくても）。
#[test]
fn the_hud_stays_inside_the_safe_area() {
    let safe = Fov::NARROW.safe();
    for scale in [kernel::UI_SCALE_MIN, 1.0, kernel::UI_SCALE_MAX] {
        let half =
            size(HUD_PX_PER_M) * scale / 2.0 - Vec2::splat(PANEL_MARGIN / HUD_PX_PER_M * scale);
        for anchor in NotificationAnchor::ALL {
            let pose = hud_pose(anchor, safe, scale);
            let (sx, sy) = anchor.side();
            let p = pose.position;
            assert!(
                (p.length() - HUD_DISTANCE).abs() < HUD_DISTANCE * 0.15,
                "{anchor:?}"
            );
            // 寄せた側の外側の端の角度（頭の正面から）。パネルが範囲より大きければ中心は反対側へ回るが、端は動かない。
            let check = |s: f32, c: f32, half: f32, neg: f32, pos: f32| {
                if s == 0.0 {
                    assert_eq!(c, 0.0, "{anchor:?}");
                } else {
                    let limit = if s > 0.0 { pos } else { neg };
                    let edge = ((s * c + half) / -p.z).atan();
                    assert!((edge - limit).abs() < 1e-3, "{anchor:?} {scale}");
                }
            };
            check(sx, p.x, half.x, safe.left, safe.right);
            check(sy, p.y, half.y, safe.down, safe.up);
            // 頭の方を向いている。
            let facing = pose.orientation * Vec3::Z;
            assert!(facing.angle_between(-p) < 1e-3, "{anchor:?}");
        }
    }
}
