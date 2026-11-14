use super::*;
use crate::test_util::*;

/// 最小化された窓の画面には「戻す」ボタンが出て、押すと同じ窓を映し直す（映し直しが最小化を戻す）。
#[gpui::test]
fn mirror_offers_restore_when_its_window_is_minimized(cx: &mut TestAppContext) {
    let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
    let (_, cx) = cx.add_window_view(|_, cx| Mirror::new(4, Some(source), None, None, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(4)
            .or_default()
            .status
            .desktop = Some(Status::Minimized)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("mirror-retry").is_none());
    assert!(cx.debug_bounds("mirror-restore").is_some());
    take_commands(cx, 4);
    click(cx, "mirror-restore");
    assert_eq!(
        take_commands(cx, 4),
        [PanelCommand::Stream(
            Some(kernel::desktop::mock::source("Editor", SourceKind::Window)),
            DEFAULT_FRAME_RATE
        )]
    );
}

/// 配信が終わってもパネルに再試行を出す。
#[gpui::test]
fn mirror_offers_retry_when_its_window_closes(cx: &mut TestAppContext) {
    let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
    let (_, cx) = cx.add_window_view(|_, cx| Mirror::new(4, Some(source), None, None, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(4)
            .or_default()
            .status
            .desktop = Some(Status::Closed)
    });
    cx.run_until_parked();
    assert!(take_commands(cx, 4).is_empty());
    assert!(cx.debug_bounds("mirror-retry").is_some());
    click(cx, "mirror-retry");
    assert_eq!(
        take_commands(cx, 4),
        [PanelCommand::Stream(
            Some(kernel::desktop::mock::source("Editor", SourceKind::Window)),
            DEFAULT_FRAME_RATE
        )]
    );
    cx.update(|_, cx| ui::redraw(4, cx));
    cx.run_until_parked();
    assert!(take_commands(cx, 4).is_empty());
}

#[gpui::test]
fn mirror_offers_retry_when_capture_fails(cx: &mut TestAppContext) {
    let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
    let (_, cx) = cx.add_window_view(|_, cx| Mirror::new(4, Some(source.clone()), None, None, cx));
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(4)
            .or_default()
            .status
            .desktop = Some(Status::Failed(kernel::desktop::Error(
            "capture stopped".into(),
        )))
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("mirror-retry").is_some());
    click(cx, "mirror-retry");
    assert_eq!(
        take_commands(cx, 4),
        [PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE)]
    );
}

#[gpui::test]
fn reconnecting_updates_the_mirrored_window_and_stream(cx: &mut TestAppContext) {
    let old = kernel::desktop::mock::source("Adobe Photoshop", SourceKind::Window);
    let next = kernel::desktop::mock::source("Untitled - Adobe Photoshop", SourceKind::Window);
    let (mirror, cx) = cx.add_window_view(|_, cx| {
        cx.set_global(Mirrors(vec![(old.clone(), 4, false)]));
        Mirror::new(4, Some(old), None, None, cx)
    });
    mirror.update(cx, |mirror, cx| mirror.restart(next.clone(), cx));
    assert_eq!(
        take_commands(cx, 4),
        [PanelCommand::Stream(Some(next.clone()), DEFAULT_FRAME_RATE)]
    );
    assert_eq!(
        cx.update(|_, cx| cx.global::<Mirrors>().0[0].0.name.clone()),
        next.name
    );
}

/// OS でウィンドウが最小化されても、ダッシュボード側ではしまわない（互いに連動しない）。
#[gpui::test]
fn os_minimizing_a_window_does_not_stash_it(cx: &mut TestAppContext) {
    let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
    let bar = cx
        .add_window(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        })
        .root(cx)
        .unwrap();
    let spec = mirror_spec(source.clone(), None, Some(bar.downgrade()), 2, false, false);
    let (_mirror, cx) = cx.add_window_view(|window, cx| {
        Waiter(spec.build(4, window, cx).downcast::<Mirror>().unwrap())
    });
    take_commands(cx, 4);
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(4)
            .or_default()
            .status
            .desktop = Some(Status::Minimized);
    });
    cx.run_until_parked();
    assert!(!take_commands(cx, 4).contains(&PanelCommand::Hide(true)));
    assert!(!bar.read_with(cx, |d, _| d.streams[0].stashed));
}
