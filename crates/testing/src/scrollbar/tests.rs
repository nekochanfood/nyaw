use super::*;
use crate::test_util::*;

/// あふれた一覧にはスクロールバーが出て、棒を掴んで引くと一覧が動く。
#[gpui::test]
fn scrollbar_appears_and_drags(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Testing::default());
    // 一覧（30 行 × 24px）があふれる高さにする。
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    // 初回のレイアウトで初めてあふれが分かるので、描き直されてから出る（本番はフレームループが描き直す）。
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let thumb = cx.debug_bounds("scroll-bars-v-thumb").expect("棒が出る");
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
    assert!(y < px(0.0), "引いた分だけ一覧が進む: {y:?}");
}

/// タッチのスライド（長押しなし）で棒の上から動かすと、棒が動いて中身が進み、中身は惰性で流れない。
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
    assert!(y < px(-30.0), "棒が 30px 動くと中身はそれ以上進む: {y:?}");
    cx.simulate_event(wheel(gpui::TouchPhase::Ended));
    assert_eq!(
        view.read_with(cx, |t, _| t.list.offset().y),
        y,
        "惰性では動かない"
    );
}

/// 先頭に着いたままさらに引くと中身が引っ張られ、時間が経つと戻る（動きが有効なときだけ）。
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
    assert!(pulled > rest, "先頭を越えて引くと中身が下へ引かれる");
    // 引き続けると上限なく引っ張れる（戻る力は離したときだけ）。
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
        "引くほど伸びる: {:?}",
        top(cx)
    );
    // 指を離すと戻り始める。
    cx.simulate_event(ScrollWheelEvent {
        position: gpui::point(area.center().x, area.bottom() - px(20.0)),
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(0.0))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Ended,
    });
    std::thread::sleep(std::time::Duration::from_millis(900));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), rest, "離したら戻る");
}

/// 末尾でも上限なく引っ張れ、タッチは手を止めても（離すまで）戻らない。
#[gpui::test]
fn pull_at_the_end_is_unlimited_and_held_while_touching(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
    cx.update(|_, cx| cx.set_global(design::Motion::default()));
    cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    // 一番下までスクロールしておく。
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
        "末尾でも上限なく引っ張れる: {:?}",
        top(cx)
    );
    // 手を止めても、離す合図が来るまで戻らない。
    let held = top(cx);
    std::thread::sleep(std::time::Duration::from_millis(700));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), held, "タッチ中は手を止めても戻らない");
    cx.simulate_event(wheel(0.0, gpui::TouchPhase::Ended));
    std::thread::sleep(std::time::Duration::from_millis(900));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(top(cx), rest, "離したら戻る");
}

/// 入れ子の内側を引っ張っても、外側の棒は反応しない。
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
        "内側の棒は縮む"
    );
}

/// 入れ子のスクロール要素の上でホイールを回すと、内側だけが動いて外側は動かない。
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
    assert!(inner < px(0.0), "内側が進む: {inner:?}");
    assert_eq!(outer, px(0.0), "外側は動かない");
}

/// スクロールバーの試験画面は 3 つの領域それぞれに棒が出る（縦横の領域は両方出る）。
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
        assert!(cx.debug_bounds(name).is_some(), "{name} が出る");
    }
    assert!(
        cx.debug_bounds("bars-v-h-thumb").is_none(),
        "縦だけの領域に横の棒は出ない"
    );
}
