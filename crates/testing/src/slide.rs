use super::*;
use design::components::slide::{Entering, fit_bar, slide_in, with_leaving};

/// カプセルの最大幅（px）。ボタンの増加でこれを超える場合、水平スクロールに切り替わる。
const MAX_BAR_PX: f32 = 360.0;

/// ボタンのスライドイン・スライドアウトと、それに連動するカプセル幅の拡縮を検証する画面（ダッシュボードのバーと共通の部品）。
/// 上段の操作で円形ボタンの追加・削除・全体リセットを行う。下段のカプセルはコンテンツ幅に追従する（バーと同一動作）。
pub struct SlideTest {
    panel: PanelId,
    /// 表示中のボタンの番号。
    items: Vec<u32>,
    /// 次に追加するボタンの番号。
    next: u32,
    /// 前フレームで表示していたアイテム（スライドアウト中のものを含む）。
    prev: Vec<u32>,
    enter: Entering,
    scroll: ScrollHandle,
}

impl SlideTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            items: (1..=3).collect(),
            next: 4,
            prev: Vec::new(),
            enter: Entering::new(false),
            scroll: ScrollHandle::new(),
        }
    }

    fn key(n: u32) -> SharedString {
        format!("slide-item-{n}").into()
    }
}

impl Render for SlideTest {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let now = design::drive::now();
        self.enter.begin(false, now);
        let shown = with_leaving(
            self.items.clone(),
            &self.prev,
            |n| Self::key(*n),
            &mut self.enter,
            now,
        );
        self.prev = shown.iter().map(|(n, _)| *n).collect();
        let buttons: Vec<_> = shown
            .iter()
            .enumerate()
            .map(|(i, (n, leaving))| {
                let key = Self::key(*n);
                let t = if *leaving {
                    self.enter.progress_out(&key, now)
                } else {
                    self.enter.progress(&key, i, now)
                };
                slide_in(
                    t,
                    components::BUTTON_HEIGHT,
                    components::round_button(&theme, key, format!("#{n}"), Variant::Default)
                        .child(n.to_string()),
                )
            })
            .collect();
        let scrolling = self.enter.follow_scroll(&self.scroll, now);
        if self.enter.finish() || scrolling {
            components::keep_drawing(window, cx);
        }
        let act = |f: fn(&mut Self)| {
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                f(this);
                ui::notify(this.panel, cx);
            })
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(
                row()
                    .child(
                        button(&theme, "slide-add", i18n::t(cx, "testing.slide.add"), false)
                            .on_click(act(|t| {
                                t.items.push(t.next);
                                t.next += 1;
                            })),
                    )
                    .child(
                        button(
                            &theme,
                            "slide-add-first",
                            i18n::t(cx, "testing.slide.add_first"),
                            false,
                        )
                        .on_click(act(|t| {
                            t.items.insert(0, t.next);
                            t.next += 1;
                        })),
                    )
                    .child(
                        button(
                            &theme,
                            "slide-remove-first",
                            i18n::t(cx, "testing.slide.remove_first"),
                            false,
                        )
                        .on_click(act(|t| {
                            if !t.items.is_empty() {
                                t.items.remove(0);
                            }
                        })),
                    )
                    .child(
                        button(
                            &theme,
                            "slide-remove-last",
                            i18n::t(cx, "testing.slide.remove_last"),
                            false,
                        )
                        .on_click(act(|t| {
                            t.items.pop();
                        })),
                    )
                    .child(
                        button(
                            &theme,
                            "slide-replay",
                            i18n::t(cx, "testing.slide.replay"),
                            false,
                        )
                        .on_click(act(|t| t.enter.replay())),
                    ),
            )
            .child(
                div().flex().justify_center().child(
                    fit_bar(
                        &theme,
                        "slide-scroll",
                        &self.scroll,
                        self.enter.peak(),
                        Some(MAX_BAR_PX),
                        buttons,
                    )
                    .debug_selector(|| "slide-bar".into()),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::time::Duration;

    fn settle(cx: &mut VisualTestContext) {
        // アニメーション時間（design::duration_ms）を実時間で待機する。
        std::thread::sleep(Duration::from_millis(700));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, id: &'static str) {
        let at = cx.debug_bounds(id).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
    }

    fn bar_width(cx: &mut VisualTestContext) -> f32 {
        f32::from(cx.debug_bounds("slide-bar").unwrap().size.width)
    }

    /// 追加ボタンはスライドインしつつカプセル幅を拡大し、削除ボタンはスライドアウトしつつ幅を縮小して、完了後に配列から除去される。
    #[gpui::test]
    fn bar_widens_and_narrows_with_the_sliding_buttons(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| SlideTest::new(1));
        cx.run_until_parked();
        settle(cx);
        let three = bar_width(cx);

        click(cx, "slide-add");
        assert!(cx.debug_bounds("slide-item-4").is_some());
        assert!(
            bar_width(cx) < three + components::BUTTON_HEIGHT / 2.0,
            "追加直後はまだ拡大していない"
        );
        settle(cx);
        assert!(
            bar_width(cx) >= three + components::BUTTON_HEIGHT,
            "拡大が完了する"
        );

        click(cx, "slide-remove-last");
        assert!(
            cx.debug_bounds("slide-item-4").is_some(),
            "スライドアウト中は同じ位置に描画を維持する"
        );
        settle(cx);
        // 完了フレームは幅 0 で残存描画され、次フレームで除去される。
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("slide-item-4").is_none(), "完了後に除去される");
        assert!((bar_width(cx) - three).abs() <= 1.0, "幅が元に復帰する");
    }

    /// 追加中のカプセル幅は一度も縮小せず、拡大完了まで単調増加する（隣接要素の移動で幅がぶれない）。
    #[gpui::test]
    fn bar_width_grows_steadily_when_a_button_is_added(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| SlideTest::new(1));
        cx.run_until_parked();
        settle(cx);
        let before = bar_width(cx);
        click(cx, "slide-add");
        let mut last = before;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(20));
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            let w = bar_width(cx);
            assert!(w >= last - 0.5, "縮小した: {last} -> {w}");
            last = w;
        }
        assert!(last >= before + components::BUTTON_HEIGHT);
    }

    /// 最大幅を超える場合、カプセルは最大幅で固定され、水平スクロールが有効になる。
    #[gpui::test]
    fn bar_stops_at_the_max_width_and_scrolls(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| SlideTest::new(1));
        cx.run_until_parked();
        settle(cx);
        assert_eq!(view.read_with(cx, |v, _| v.scroll.max_offset().x), px(0.0));
        for _ in 0..6 {
            click(cx, "slide-add");
        }
        settle(cx);
        assert!(
            bar_width(cx) <= MAX_BAR_PX + 1.0,
            "最大幅で固定: {}",
            bar_width(cx)
        );
        assert_eq!(
            view.read_with(cx, |v, _| v.scroll.max_offset().y),
            px(0.0),
            "垂直方向にはオーバーフローしない"
        );
        assert!(
            view.read_with(cx, |v, _| v.scroll.max_offset().x) > px(0.0),
            "オーバーフロー分はスクロール可能"
        );
    }

    /// 20ms 間隔で `n` フレーム描画する（スクロール等、フレーム単位で進行するアニメーションを進める）。
    fn frames(cx: &mut VisualTestContext, n: usize) {
        for _ in 0..n {
            std::thread::sleep(Duration::from_millis(20));
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }
    }

    fn offset_x(view: &Entity<SlideTest>, cx: &mut VisualTestContext) -> f32 {
        -f32::from(view.read_with(cx, |v, _| v.scroll.offset().x))
    }

    /// 追加したボタンの位置までスクロールする（末尾に追加すれば末尾へ、先頭に追加すれば先頭へ）。
    #[gpui::test]
    fn bar_scrolls_to_a_newly_added_button(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| SlideTest::new(1));
        cx.run_until_parked();
        settle(cx);
        for _ in 0..6 {
            click(cx, "slide-add");
        }
        frames(cx, 60);
        let max = f32::from(view.read_with(cx, |v, _| v.scroll.max_offset().x));
        assert!(max > 0.0);
        let at = offset_x(&view, cx);
        assert!((at - max).abs() <= 1.0, "末尾までスクロール: {at} / {max}");
        click(cx, "slide-add-first");
        // 即座には到達せず、時間をかけてスクロールする。
        let start = offset_x(&view, cx);
        frames(cx, 3);
        let early = offset_x(&view, cx);
        assert!(early > 1.0 && early < start, "スクロール途中: {start} -> {early}");
        frames(cx, 40);
        let at = offset_x(&view, cx);
        assert!(at.abs() <= 1.0, "先頭までスクロール（左マージンまで）: {at}");
    }
}
