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

/// 出ていく途中のアウトを終わらせ、次の場所へ移らせる（アウトは実時間で待つので、待った後にする）。
fn finish_out(view: &gpui::Entity<Toast>, cx: &mut VisualTestContext) {
    view.update(cx, |t, _| {
        if let Phase::Out(_) = t.phase {
            t.phase = Phase::Out(Instant::now() - Duration::from_secs(1));
        }
    });
    set(cx, |_| {});
}

fn hud_shows(c: &[PanelCommand]) -> bool {
    matches!(c[0], PanelCommand::Place(_))
        && matches!(c[1], PanelCommand::Attach(Device::Hmd, _))
        && c.contains(&PanelCommand::PassThrough(true))
        && fits(c)
        && c.last() == Some(&PanelCommand::Hide(false))
}

/// 時計を見ていなければ視界の端に出て、時計を見ると手首へ、ダッシュボードを開くとダッシュボードへ移る。どの移動も、
/// 今の場所で消えきって（前の中身のまま）から、次の場所へ瞬間的に移って現れる（目の前を横切らない）。
/// ダッシュボードを閉じると片付き、ダッシュボードで見たものは手首にも戻らない。
#[gpui::test]
fn the_toast_moves_out_and_in_from_the_hud_to_the_wrist_to_the_dashboard(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| c.notifications = Arc::from([note(1, None)]));
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Hud);
    assert!(hud_shows(&c), "{c:?}");
    assert!(cx.debug_bounds("toast-hud-1").is_some());
    assert!(cx.debug_bounds("toast-1").is_none(), "HUD は押せない");

    // 時計を見る: まず HUD で消える（中身は HUD のまま）。消えきってから手首で現れる。
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(take(cx, TOAST), [PanelCommand::Hide(true)]);
    assert_eq!(place(&view, cx), Place::Hud, "消える間は前の場所");
    assert!(
        cx.debug_bounds("toast-hud-1").is_some(),
        "消える間は前の中身"
    );
    finish_out(&view, cx);
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Wrist);
    let to_wrist = Parent {
        panel: WRIST,
        docked: true,
        slot: Some(WRIST_TILT),
        ..Default::default()
    };
    assert_eq!(c[0], PanelCommand::Parent(Some(to_wrist)), "{c:?}");
    assert!(c.contains(&PanelCommand::PassThrough(false)) && fits(&c));
    assert_eq!(c.last(), Some(&PanelCommand::Hide(false)));
    assert!(cx.debug_bounds("toast-1").is_some());

    // ダッシュボードを開く: 手首で消えて、手首から外れ、ダッシュボードの上で現れる。
    set(cx, |c| {
        c.dock = Some((BAR, 0.5));
        c.panels.entry(BAR).or_default();
    });
    assert_eq!(take(cx, TOAST), [PanelCommand::Hide(true)]);
    finish_out(&view, cx);
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
    assert_eq!(
        c[..2],
        [
            PanelCommand::Parent(None),
            PanelCommand::Parent(Some(to_bar))
        ]
    );
    assert_eq!(c.last(), Some(&PanelCommand::Hide(false)));

    // ダッシュボードを閉じる: 消えて外れる。ダッシュボードで見たものは手首に戻らない。
    set(cx, |c| c.panels.get_mut(&BAR).unwrap().status.hidden = true);
    assert_eq!(take(cx, TOAST), [PanelCommand::Hide(true)]);
    finish_out(&view, cx);
    assert_eq!(take(cx, TOAST), [PanelCommand::Parent(None)]);
    assert_eq!(place(&view, cx), Place::Hidden);
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(
        place(&view, cx),
        Place::Hidden,
        "ダッシュボードで見たものは手首に戻らない"
    );
    assert!(take(cx, TOAST).is_empty());
}

/// 消えきる前（アウトの途中）には次の場所へ移らない。
#[gpui::test]
fn the_toast_waits_for_its_out_animation(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| c.notifications = Arc::from([note(1, None)]));
    take(cx, TOAST);
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(take(cx, TOAST), [PanelCommand::Hide(true)]);
    for _ in 0..3 {
        set(cx, |_| {});
        assert!(take(cx, TOAST).is_empty(), "まだ消えている途中");
    }
    std::thread::sleep(Duration::from_millis(Motion::Exit.duration_ms()) + OUT_MARGIN);
    set(cx, |_| {});
    assert_eq!(place(&view, cx), Place::Wrist);
}

/// カードは新しいものが上から積まれる（手首・ダッシュボードでも、HUD でも）。
#[gpui::test]
fn the_newest_card_is_on_top(cx: &mut TestAppContext) {
    let (_view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None)])
    });
    let [old, new] = ["toast-hud-1", "toast-hud-2"].map(|s| cx.debug_bounds(s).unwrap());
    assert!(new.top() < old.top(), "HUD: {new:?} {old:?}");
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
    finish_out(&view, cx);
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

/// 出しきれない分の「ほか n 件」は、一番下のカードに重ならない（手首・ダッシュボードの下に寄せる並べ方でも、HUD でも）。
#[gpui::test]
fn the_more_line_does_not_overlap_the_cards(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    let notes: Vec<_> = (1..=5).map(|id| note(id, None)).collect();
    set(cx, |c| c.notifications = Arc::from(notes));
    // HUD: 新しい 3 件（5・4・3）の下に「ほか 2 件」。
    let (last, more) = (
        cx.debug_bounds("toast-hud-3").unwrap(),
        cx.debug_bounds("toast-more").unwrap(),
    );
    assert!(more.top() >= last.bottom(), "HUD: {last:?} {more:?}");
    // 手首: 下に寄せて並べる。
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    finish_out(&view, cx);
    let (last, more) = (
        cx.debug_bounds("toast-3").unwrap(),
        cx.debug_bounds("toast-more").unwrap(),
    );
    assert!(
        more.top() >= last.bottom() + px(spacing::SM) - px(1.0),
        "手首: {last:?} {more:?}"
    );
}

/// 手首の通知はマウスのドラッグでも指のスライド（横のスクロール）でも左右へ片付く。片付けるのはその場だけで、押した
/// ことにはせず、通知の一覧（クイック設定）からも消さない。短いスワイプは元へ戻る。
#[gpui::test]
fn cards_swipe_away(cx: &mut TestAppContext) {
    use gpui::{MouseButton, ScrollDelta, ScrollWheelEvent, TouchPhase, point};
    let (_view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None), note(3, None)]);
        c.panels.entry(WRIST).or_default().status.gazed = true;
    });
    take(cx, TOAST);
    let settle = |cx: &mut VisualTestContext| {
        std::thread::sleep(Duration::from_millis(Motion::Return.duration_ms() + 50));
        set(cx, |_| {});
        set(cx, |_| {});
    };
    let card = |cx: &mut VisualTestContext, id: u32| cx.debug_bounds(format!("toast-{id}").leak());
    // マウス: 押して左へ大きくドラッグし、離す。
    let at = card(cx, 3).unwrap().center();
    let width = card(cx, 3).unwrap().size.width;
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    for x in [0.2, 0.5, 0.8] {
        cx.simulate_mouse_move(
            at - point(width * x, px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
    }
    cx.simulate_mouse_up(
        at - point(width * 0.8, px(0.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    settle(cx);
    assert!(card(cx, 3).is_none(), "大きく動かすと片付く");
    // 指: 横のスクロールで右へ少しだけ動かすと戻る。大きく動かすと片付く。
    let swipe = |cx: &mut VisualTestContext, id: u32, by: f32| {
        let at = card(cx, id).unwrap().center();
        for (phase, dx) in [
            (TouchPhase::Started, 4.0),
            (TouchPhase::Moved, by),
            (TouchPhase::Ended, 0.0),
        ] {
            cx.simulate_event(ScrollWheelEvent {
                position: at,
                delta: ScrollDelta::Pixels(point(px(dx), px(0.0))),
                modifiers: Modifiers::none(),
                touch_phase: phase,
            });
        }
        settle(cx);
    };
    swipe(cx, 2, 20.0);
    assert!(card(cx, 2).is_some(), "短いスワイプは戻る");
    swipe(cx, 2, f32::from(width));
    assert!(card(cx, 2).is_none());
    assert!(card(cx, 1).is_some(), "ほかの通知は残る");
    assert!(
        !take(cx, TOAST)
            .iter()
            .any(|c| matches!(c, PanelCommand::Notification { .. })),
        "スワイプは押したことにしない"
    );
    let urls = cx.update(|window, cx| design::link::take(window, cx));
    assert!(urls.is_empty());
    let left = cx.update(|_, cx| cx.global::<PanelControl>().notifications.len());
    assert_eq!(left, 3, "一覧からは消さない");
}
