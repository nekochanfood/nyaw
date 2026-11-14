//! 今の LOD を数字と色で出す画面（[`TestingLod`]）。

use super::*;

/// LOD0〜3 の背景色: 緑・黄・橙・赤。
pub const LOD_COLORS: [u32; 4] = [0x30a050, 0xc0b030, 0xc07030, 0xc03030];

/// 今の LOD（距離による描画解像度の段階）を数字と背景色で大きく出す画面（`testing-lod`）。
pub struct TestingLod {
    panel: PanelId,
}

impl TestingLod {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
    }
}

impl Render for TestingLod {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let lod = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map_or(0, |c| c.status.lod);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_3()
            .bg(rgb(LOD_COLORS[(lod as usize).min(3)]))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .debug_selector(|| "lod-level".into())
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(320.0))
                    .child(format!("{lod}")),
            )
    }
}
