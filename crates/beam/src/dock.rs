//! はまる所の見本（[`Pointer::Dock`]）。はめられる画面を掴んでいる間、はまる所に画面の地と同じ大きさで出る半透明の面。
//! ここは絵だけを描き、置き場所・大きさ・濃さ・息づき・光り方はオーバーレイ層が決める（描き直すのは一行が変わったときだけ）。

use design::{Theme, font_size, icons, radius, spacing};
use gpui::{Context, Window, div, prelude::*, px, rgba};
use kernel::overlay::DOCK_HINT_PX;
use kernel::{DockHint, Hand, PanelId, Pointer, Pose, Vec2};
use ui::PanelSpec;

/// 窓の最初の大きさ（px）。掴んだ画面に合わせてオーバーレイ層が変える。
const GHOST_PX: (u32, u32) = (400, 300 + DOCK_HINT_PX);
/// はまる所の縁（破線）の太さと、面の濃さ（強調色に掛ける不透明度。0〜255）。
const BORDER_PX: f32 = 4.0;
const FILL_ALPHA: u32 = 0x33;
/// 真ん中の磁石の大きさ（px）と濃さ。
const MAGNET_PX: f32 = 96.0;
const MAGNET_OPACITY: f32 = 0.6;

/// `hand` の見本の起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, |panel, _, _| DockGhost {
        panel,
        hint: DockHint::Attach,
    })
    .sized(Vec2::splat(0.4), GHOST_PX)
    .pointer(hand, Pointer::Dock)
}

/// 上の帯に一行（引っ張って外す・近づけてくっつける）、その下にはまる所（強調色の半透明の面と破線の縁、真ん中に磁石）。
pub struct DockGhost {
    panel: PanelId,
    /// 最後に出した一行（離して薄れていく間も同じものを出す）。
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
            DockHint::Attach => (&icons::MAGNET, "dock.attach"),
        };
        let accent = theme.accent_solid;
        let band = div()
            .h(px(DOCK_HINT_PX as f32))
            .flex()
            .items_center()
            .justify_center()
            .child(
                // ツールチップと同じ地で、どこに重なっても読める。
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
        let slot = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::XL))
            .bg(rgba(accent & !0xff | FILL_ALPHA))
            .border(px(BORDER_PX))
            .border_dashed()
            .border_color(rgba(accent))
            .child(
                design::icon(&icons::MAGNET)
                    .size(px(MAGNET_PX))
                    .color(rgba(accent))
                    .opacity(MAGNET_OPACITY),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .font(design::font::font())
            .child(band)
            .child(slot)
    }
}
