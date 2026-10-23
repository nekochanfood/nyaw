use super::*;
use design::components::slide::{Entering, fit_bar, scroll_to_slot, slide_in, with_leaving};

/// カプセルの最大幅（px）。ボタンが増えてこれを超えそうなら、横にスクロールする。
const MAX_BAR_PX: f32 = 360.0;

/// ボタンの滑り込み・滑り出しと、それに合わせて広がる・閉じる幅を試す画面（ダッシュボードのバーと同じ部品）。
/// 上の操作で丸いボタンを足す・消す・全部出し直す。下のカプセルは中身の幅に収まる（バーと同じ）。
pub struct SlideTest {
    panel: PanelId,
    /// 並んでいるボタンの番号。
    items: Vec<u32>,
    /// 次に足すボタンの番号。
    next: u32,
    /// 前のフレームに並べていたもの（消えていくものを含む）。
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

    fn redraw(&self, cx: &mut Context<Self>) {
        cx.notify();
        cx.default_global::<PanelControl>()
            .panels
            .entry(self.panel)
            .or_default()
            .redraw = true;
    }

    fn key(n: u32) -> SharedString {
        format!("slide-item-{n}").into()
    }
}

impl Render for SlideTest {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let now = Instant::now();
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
        if let Some(ix) = self.enter.take_fresh() {
            scroll_to_slot(&self.scroll, ix);
        }
        if self.enter.finish() {
            components::keep_drawing(window, cx);
        }
        let act = |f: fn(&mut Self)| {
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                f(this);
                this.redraw(cx);
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
        // 動きの長さ（design::duration_ms）を実時間で待つ。
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

    /// 足したボタンは滑り込みながらカプセルの幅を広げ、消したボタンは滑り出ながら幅を閉じて、終われば並びから外れる。
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
            "足した直後はまだ広がっていない"
        );
        settle(cx);
        assert!(
            bar_width(cx) >= three + components::BUTTON_HEIGHT,
            "広がりきる"
        );

        click(cx, "slide-remove-last");
        assert!(
            cx.debug_bounds("slide-item-4").is_some(),
            "消える間は同じ場所に描き残す"
        );
        settle(cx);
        // 終わったフレームは幅 0 で描き残し、次のフレームで外れる。
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("slide-item-4").is_none(), "終われば外れる");
        assert!((bar_width(cx) - three).abs() <= 1.0, "幅が元に戻る");
    }

    /// 足している間、カプセルの幅は一度も縮まず、広がりきるまで増え続ける（隣が動いても幅が揺れない）。
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
            assert!(w >= last - 0.5, "縮んだ: {last} -> {w}");
            last = w;
        }
        assert!(last >= before + components::BUTTON_HEIGHT);
    }

    /// 最大幅を超えそうなら、カプセルはそこで頭打ちになり、横にスクロールできる。
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
            "頭打ち: {}",
            bar_width(cx)
        );
        assert_eq!(
            view.read_with(cx, |v, _| v.scroll.max_offset().y),
            px(0.0),
            "縦にはあふれない"
        );
        assert!(
            view.read_with(cx, |v, _| v.scroll.max_offset().x) > px(0.0),
            "あふれた分はスクロールできる"
        );
    }

    fn offset_x(view: &Entity<SlideTest>, cx: &mut VisualTestContext) -> f32 {
        -f32::from(view.read_with(cx, |v, _| v.scroll.offset().x))
    }

    /// 足したボタンの場所までスクロールする（末尾に足せば末尾へ、先頭に足せば先頭へ）。
    #[gpui::test]
    fn bar_scrolls_to_a_newly_added_button(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| SlideTest::new(1));
        cx.run_until_parked();
        settle(cx);
        for _ in 0..6 {
            click(cx, "slide-add");
        }
        settle(cx);
        settle(cx);
        let max = f32::from(view.read_with(cx, |v, _| v.scroll.max_offset().x));
        assert!(max > 0.0);
        let at = offset_x(&view, cx);
        assert!((at - max).abs() <= 1.0, "末尾へ: {at} / {max}");
        click(cx, "slide-add-first");
        settle(cx);
        settle(cx);
        let at = offset_x(&view, cx);
        assert!(at.abs() <= 1.0, "先頭へ（左の余白まで）: {at}");
    }
}
