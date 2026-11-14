//! 毎フレーム変わる画面（[`TestingAnim`]）。

use super::*;

/// ループアニメーションの周期（秒）。
const ANIM_PERIOD: f32 = 2.0;

/// 毎フレーム中身が変わる画面（`testing-anim`）: 描いた回数と、周期 [`ANIM_PERIOD`] で左右を往復し色の変わる四角。
/// 見えている間はフレームループが毎フレーム描き直す（[`ui::PanelState::live`]）。
pub struct TestingAnim {
    /// 描いた回数。
    pub frames: u64,
    start: Instant,
}

impl TestingAnim {
    pub fn new(panel: PanelId, cx: &mut App) -> Self {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .live = true;
        Self {
            frames: 0,
            start: Instant::now(),
        }
    }
}

impl Render for TestingAnim {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        self.frames += 1;
        let phase = self.start.elapsed().as_secs_f32() / ANIM_PERIOD % 1.0;
        // 0→1→0 と往復する。
        let x = 1.0 - (phase * 2.0 - 1.0).abs();
        components::card_panel(&theme)
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .debug_selector(|| "anim-frames".into())
                    .text_size(px(64.0))
                    .child(i18n::tf(cx, "testing.frame", &[("count", &self.frames)])),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(40.0))
                    .left(relative(0.05 + 0.7 * x))
                    .size(px(160.0))
                    .bg(hsla(phase, 0.8, 0.5, 1.0)),
            )
    }
}
