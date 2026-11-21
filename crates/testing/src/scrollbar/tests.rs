use super::*;
use crate::test_util::*;

/// オーバーフローしたリストにスクロールバーが表示され、サムをドラッグするとリストがスクロールする。
#[gpui::test]
fn scrollbar_appears_and_drags(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Testing::default());
    // リスト（30 行 × 24px）がオーバーフローする高さに設定する。
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    // 初回レイアウトで初めてオーバーフローが検出されるため、再描画後に表示される（本番ではフレームループが再描画する）。
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let thumb = cx.debug_bounds("scroll-bars-v-thumb").expect("サムが表示される");
    let from = thumb.center();
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + gpui::point(px(0.0), px(40.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        from + gpui::point(px(0.0), px(40.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    let y = view.read_with(cx, |t, _| t.list.offset().y);
    assert!(y < px(0.0), "ドラッグ分だけリストがスクロールする: {y:?}");
}

/// タッチスライド（長押しなし）でサム上からドラッグすると、サムが移動してコンテンツがスクロールし、慣性スクロールは発生しない。
#[gpui::test]
fn scrollbar_follows_a_touch_slide_without_long_press(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let at = cx.debug_bounds("scroll-bars-v-thumb").unwrap().center();
    let wheel = |phase| ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(30.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: phase,
    };
    cx.simulate_event(wheel(gpui::TouchPhase::Started));
    let y = view.read_with(cx, |t, _| t.list.offset().y);
    assert!(y < px(-30.0), "サムが 30px 移動するとコンテンツはそれ以上スクロールする: {y:?}");
    cx.simulate_event(wheel(gpui::TouchPhase::Ended));
    assert_eq!(
        view.read_with(cx, |t, _| t.list.offset().y),
        y,
        "慣性スクロールは発生しない"
    );
}

/// 先頭到達後にさらにドラッグするとコンテンツが引き伸ばされ、時間経過で元に戻る（アニメーション有効時のみ）。
#[gpui::test]
fn content_is_pulled_at_the_edge_and_settles(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.update(|_, cx| cx.set_global(design::Motion::default()));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let area = cx.debug_bounds("scrollbar-v").unwrap();
    let top =
        |cx: &mut gpui::VisualTestContext| cx.debug_bounds("scrollbar-nested").unwrap().origin.y;
    let rest = top(cx);
    cx.simulate_event(ScrollWheelEvent {
        position: gpui::point(area.center().x, area.bottom() - px(20.0)),
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let pulled = top(cx);
    assert!(pulled > rest, "先頭を超えてドラッグするとコンテンツが下方向に引かれる");
    // ドラッグを継続すると上限なく引き伸ばせる（復元力はリリース時にのみ作用する）。
    for _ in 0..20 {
        cx.simulate_event(ScrollWheelEvent {
            position: gpui::point(area.center().x, area.bottom() - px(20.0)),
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(
        top(cx) > pulled + px(100.0),
        "ドラッグ量に応じて伸びる: {:?}",
        top(cx)
    );
    // リリースすると復元が開始される。
    cx.simulate_event(ScrollWheelEvent {
        position: gpui::point(area.center().x, area.bottom() - px(20.0)),
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(0.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Ended,
    });
    std::thread::sleep(std::time::Duration::from_millis(900));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), rest, "リリース後に元の位置に復帰する");
}

/// 末尾でも上限なく引き伸ばせ、タッチ中は停止しても（リリースするまで）復元しない。
#[gpui::test]
fn pull_at_the_end_is_unlimited_and_held_while_touching(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.update(|_, cx| cx.set_global(design::Motion::default()));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    // 末尾までスクロールしておく。
    view.read_with(cx, |t, _| {
        let m = t.vertical.max_offset();
        t.vertical.set_offset(gpui::point(px(0.0), -m.y));
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let area = cx.debug_bounds("scrollbar-v").unwrap();
    let at = gpui::point(area.center().x, area.bottom() - px(20.0));
    let top =
        |cx: &mut gpui::VisualTestContext| cx.debug_bounds("scrollbar-nested").unwrap().origin.y;
    let rest = top(cx);
    let wheel = |dy: f32, phase| ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(dy))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: phase,
    };
    cx.simulate_event(wheel(-40.0, gpui::TouchPhase::Started));
    for _ in 0..20 {
        cx.simulate_event(wheel(-40.0, gpui::TouchPhase::Moved));
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(
        top(cx) < rest - px(150.0),
        "末尾でも上限なく引き伸ばせる: {:?}",
        top(cx)
    );
    // タッチを停止しても、リリースイベントが来るまで復元しない。
    let held = top(cx);
    std::thread::sleep(std::time::Duration::from_millis(700));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), held, "タッチ中は停止しても復元しない");
    cx.simulate_event(wheel(0.0, gpui::TouchPhase::Ended));
    std::thread::sleep(std::time::Duration::from_millis(900));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), rest, "リリース後に元の位置に復帰する");
}

/// ネストされた内側を引っ張っても、外側のバーは反応しない。
#[gpui::test]
fn pulling_the_inner_does_not_squash_the_outer_bar(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.update(|_, cx| cx.set_global(design::Motion::default()));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let outer = cx.debug_bounds("bars-v-v-thumb").unwrap().size.height;
    let inner = cx.debug_bounds("bars-inner-v-thumb").unwrap().size.height;
    let at = cx.debug_bounds("scrollbar-inner").unwrap().center();
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(
        cx.debug_bounds("bars-v-v-thumb").unwrap().size.height,
        outer
    );
    assert!(
        cx.debug_bounds("bars-inner-v-thumb").unwrap().size.height < inner,
        "内側のサムは縮小する"
    );
}

/// ネストされたスクロール要素上でホイールを操作すると、内側のみがスクロールし外側は不動。
#[gpui::test]
fn nested_scroll_moves_only_the_innermost(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let at = cx.debug_bounds("scrollbar-inner").unwrap().center();
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(-30.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let (inner, outer) = view.read_with(cx, |t, _| (t.inner.offset().y, t.vertical.offset().y));
    assert!(inner < px(0.0), "内側がスクロールする: {inner:?}");
    assert_eq!(outer, px(0.0), "外側は不動");
}

/// スクロールバーのテスト画面では 3 つの領域それぞれにバーが表示される（縦横領域では両方向）。
#[gpui::test]
fn scrollbar_screen_shows_bars_for_overflowing_axes(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    for name in [
        "bars-v-v-thumb",
        "bars-h-h-thumb",
        "bars-both-v-thumb",
        "bars-both-h-thumb",
    ] {
        assert!(cx.debug_bounds(name).is_some(), "{name} が表示される");
    }
    assert!(
        cx.debug_bounds("bars-v-h-thumb").is_none(),
        "縦方向のみの領域に横方向のバーは表示されない"
    );
}
