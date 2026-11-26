use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use kernel::Request;
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

/// 蓄積されたシステムへの要求を取り出す。
fn take_requests(cx: &mut VisualTestContext) -> Vec<Request> {
    cx.update(|_, cx| std::mem::take(&mut cx.default_global::<PanelControl>().requests))
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

/// アウト中の遷移を強制完了させ、次の位置への遷移を発生させる（アウトは実時間で待機するため、待機後に呼び出す）。
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

/// 時計を注視していなければ HUD に表示され、注視すると手首へ、ダッシュボードを開くとダッシュボードへ遷移する。すべての遷移で、
/// 現在位置でフェードアウト完了（前のコンテンツのまま）後、次の位置へ瞬間移動してフェードインする（視界を横切らない）。
/// ダッシュボードを閉じると非表示になり、ダッシュボードで表示済みの通知は手首にも遷移しない。
#[gpui::test]
fn the_toast_moves_out_and_in_from_the_hud_to_the_wrist_to_the_dashboard(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| c.notifications = Arc::from([note(1, None)]));
    let c = take(cx, TOAST);
    assert_eq!(place(&view, cx), Place::Hud);
    assert!(hud_shows(&c), "{c:?}");
    assert!(cx.debug_bounds("toast-hud-1").is_some());
    assert!(
        cx.debug_bounds("toast-1").is_none(),
        "HUD は非インタラクティブ"
    );

    // 時計を注視: まず HUD でフェードアウト（コンテンツは HUD のまま）。完全消失後に手首でフェードイン。
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(take(cx, TOAST), [PanelCommand::Hide(true)]);
    assert_eq!(place(&view, cx), Place::Hud, "フェードアウト中は前の位置");
    assert!(
        cx.debug_bounds("toast-hud-1").is_some(),
        "フェードアウト中は前のコンテンツ"
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

    // ダッシュボードを開く: 手首でフェードアウトし、手首から切り離され、ダッシュボード上部でフェードインする。
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

    // ダッシュボードを閉じる: フェードアウトして切り離される。ダッシュボードで表示済みの通知は手首に遷移しない。
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
        "ダッシュボードで表示済みの通知は手首に遷移しない"
    );
    assert!(take(cx, TOAST).is_empty());
}

/// フェードアウト完了前（アウト遷移中）は次の位置へ遷移しない。
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
        assert!(take(cx, TOAST).is_empty(), "フェードアウト中");
    }
    std::thread::sleep(Duration::from_millis(Motion::Exit.duration_ms()) + OUT_MARGIN);
    set(cx, |_| {});
    assert_eq!(place(&view, cx), Place::Wrist);
}

/// カードは新しいものが上に積まれる（手首・ダッシュボード・HUD いずれでも）。
#[gpui::test]
fn the_newest_card_is_on_top(cx: &mut TestAppContext) {
    let (_view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None)])
    });
    let [old, new] = ["toast-hud-1", "toast-hud-2"].map(|s| cx.debug_bounds(s).unwrap());
    assert!(new.top() < old.top(), "HUD: {new:?} {old:?}");
}

/// 手首・ダッシュボードの通知は展開しない（ホバーしてもアクションボタンは表示しない）。クリックするとアプリには通知せず、
/// 通知センターに該当通知を展開して開くよう要求する。
#[gpui::test]
fn cards_jump_to_the_notification_center(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, Some("https://example.com/2"))]);
        c.panels.entry(WRIST).or_default().status.gazed = true;
    });
    take(cx, TOAST);
    let at = cx.debug_bounds("toast-1").unwrap().center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("toast-action-1-0").is_none(),
        "ホバーしても展開しない"
    );
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    let asked = cx.update(|_, cx| {
        let c = cx.global::<ui::notification::Center>();
        (c.asked, c.focus)
    });
    assert_eq!(asked, (1, Some(1)));
    let shown = view.read_with(cx, |t, _| t.shown.iter().map(|n| n.id).collect::<Vec<_>>());
    assert_eq!(
        shown,
        [2],
        "クリックした通知は閲覧済みとなり消える（他は残留する）"
    );
    assert!(
        !take_requests(cx)
            .iter()
            .any(|c| matches!(c, Request::Notification { .. })),
        "アプリへは通知しない"
    );
    let urls = cx.update(|window, cx| design::link::take(window, cx));
    assert!(urls.is_empty(), "URL も開かない（通知センターに委譲する）");
}

/// 通知センターがダッシュボードにドッキングして表示されている間は、トーストは非表示になる（新規到着分を含め閲覧済みとなり、
/// 閉じても再表示されない）。
#[gpui::test]
fn the_toast_steps_aside_for_the_notification_center(cx: &mut TestAppContext) {
    const CENTER: PanelId = 9;
    let (view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.dock = Some((BAR, 0.5));
        c.panels.entry(BAR).or_default();
        c.notifications = Arc::from([note(1, None)]);
    });
    assert_eq!(place(&view, cx), Place::Dashboard);
    cx.update(|window, cx| {
        cx.default_global::<ui::notification::Center>().panel = Some(CENTER);
        cx.default_global::<PanelControl>()
            .panels
            .entry(CENTER)
            .or_default();
        ui::dock(CENTER, cx);
        window.refresh();
    });
    set(cx, |_| {});
    finish_out(&view, cx);
    assert_eq!(
        place(&view, cx),
        Place::Hidden,
        "通知センターに表示されるためトーストは非表示"
    );
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None)])
    });
    assert_eq!(
        place(&view, cx),
        Place::Hidden,
        "表示中に到着した通知も非表示"
    );
    set(cx, |c| c.panels.get_mut(&BAR).unwrap().status.hidden = true);
    set(cx, |c| {
        c.panels.entry(WRIST).or_default().status.gazed = true
    });
    assert_eq!(
        place(&view, cx),
        Place::Hidden,
        "通知センターで閲覧済みの通知は再表示されない"
    );
}

/// HUD コンテンツの外縁は、すべてのアンカー位置で両目の確実可視範囲の端に一致し、範囲外にはみ出さない（UI スケールが大きくても）。
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
            // アンカー側の外縁の角度（頭部正面基準）。パネルが範囲より大きい場合、中心は反対側に回るが、外縁位置は不変。
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
            // 頭部方向を向いている。
            let facing = pose.orientation * Vec3::Z;
            assert!(facing.angle_between(-p) < 1e-3, "{anchor:?}");
        }
    }
}

/// 表示上限を超過した「ほか n 件」は、最下部のカードに重ならない（手首・ダッシュボードの下寄せレイアウトでも、HUD でも）。
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

/// 手首の通知はマウスドラッグでも指のスライド（横スクロール）でも左右に非表示にできる。非表示はその場のみの効果で、
/// アクティベートとは扱わず、通知センター（クイック設定の一覧）からも削除しない。短いスワイプは元に戻る。
#[gpui::test]
fn cards_swipe_away(cx: &mut TestAppContext) {
    use gpui::{MouseButton, ScrollDelta, ScrollWheelEvent, TouchPhase, point};
    let (_view, cx) = cx.add_window_view(|_, cx| Toast::new(TOAST, WRIST, cx));
    set(cx, |c| {
        c.notifications = Arc::from([note(1, None), note(2, None), note(3, None)]);
        c.panels.entry(WRIST).or_default().status.gazed = true;
    });
    take(cx, TOAST);
    // リリース後: 横方向への退出・復帰アニメーションを待って描画し、退出の場合は高さの畳みアニメーションも待つ（完全に畳まれて非表示になる）。
    let settle = |cx: &mut VisualTestContext| {
        std::thread::sleep(Duration::from_millis(Motion::Return.duration_ms() + 50));
        set(cx, |_| {});
        std::thread::sleep(Duration::from_millis(
            ui::swipe::COLLAPSE.duration_ms() + 50,
        ));
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
    assert!(card(cx, 3).is_none(), "大きくドラッグすると非表示になる");
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
    assert!(card(cx, 2).is_some(), "短いスワイプは元に戻る");
    swipe(cx, 2, f32::from(width));
    assert!(card(cx, 2).is_none());
    assert!(card(cx, 1).is_some(), "他の通知は残留する");
    assert!(
        !take_requests(cx)
            .iter()
            .any(|c| matches!(c, Request::Notification { .. })),
        "スワイプはアクティベートとして扱わない"
    );
    let urls = cx.update(|window, cx| design::link::take(window, cx));
    assert!(urls.is_empty());
    let left = cx.update(|_, cx| cx.global::<PanelControl>().notifications.len());
    assert_eq!(left, 3, "通知一覧からは削除しない");
}
