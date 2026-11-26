//! ドッキングスロットのゴースト表示（[`Pointer::Dock`]）。ドッキング可能なパネルを掴んでいる間、スロットにパネル背景より少し大きい角枠（[`crate::bracket`]）とヒントテキストを描く。
//! ドッキング成立時は強調色、解除時は無彩色で状態を視覚的に区別する。
//! 描画のみを担い、配置・寸法・フェード制御はオーバーレイ層が行う（再描画はヒントテキスト変更時のみ）。

use design::{Theme, font_size, icons, radius, spacing};
use gpui::{Context, Window, div, prelude::*, px, rgba};
use kernel::overlay::DOCK_HINT_PX;
use kernel::{DockHint, Hand, PanelId, Pointer, Pose, Vec2};
use ui::PanelSpec;

use crate::bracket::{self, Corner};

/// ウィンドウの初期サイズ（px）。掴んだパネルに合わせてオーバーレイ層が変更する。
const GHOST_PX: (u32, u32) = (400, 300 + DOCK_HINT_PX);

/// `hand` のドッキングゴーストの起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, |panel, _, _| DockGhost {
        panel,
        hint: DockHint::Attach,
    })
    .sized(Vec2::splat(0.4), GHOST_PX)
    .pointer(hand, Pointer::Dock)
}

/// 上部にヒントテキスト帯（Pull / Release / Attach）、下部に角枠のみのスロット表示（中央は透過）。
pub struct DockGhost {
    panel: PanelId,
    /// 最後に表示したヒント（フェードアウト中も同じものを維持する）。
    hint: DockHint,
}

impl Render for DockGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hint) = cx
            .try_global::<ui::PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .and_then(|s| s.status.dock_hint)
        {
            self.hint = hint;
        }
        let theme = Theme::current(cx);
        let (glyph, key) = match self.hint {
            DockHint::Pull => (&icons::ARROW_SQUARE_OUT, "dock.pull"),
            DockHint::Release => (&icons::LINK, "dock.release"),
            DockHint::Attach => (&icons::MAGNET, "dock.attach"),
        };
        let line = bracket::color(&theme, self.hint.docks());
        let band = div()
            .h(px(DOCK_HINT_PX as f32))
            .flex()
            .items_center()
            .justify_center()
            .child(
                // ツールチップと同じ背景色で、どの面に重なっても可読性を確保する。アイコンは文字色と統一。
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .px(px(spacing::LG))
                    .py(px(spacing::SM))
                    .rounded(px(radius::FULL))
                    .bg(rgba(
                        theme.tooltip & !0xff | design::components::TOOLTIP_ALPHA,
                    ))
                    .text_color(rgba(theme.on_tooltip))
                    .text_size(px(font_size::LG))
                    .whitespace_nowrap()
                    .child(
                        design::icon(glyph)
                            .size(px(font_size::LG))
                            .color(rgba(theme.on_tooltip)),
                    )
                    .child(i18n::t(cx, key)),
            );
        let slot = bracket::frame(&Corner::ALL, line).flex_1().w_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font(design::font::font())
            .child(band)
            .child(slot)
    }
}
