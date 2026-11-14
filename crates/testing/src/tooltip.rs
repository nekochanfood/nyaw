//! ツールチップを試す画面（[`TooltipTest`]）。

use super::*;

/// ツールチップを試す画面（`tooltip-test`）。
/// - `tip-short` / `tip-long` は隣り合っていて、移ると猶予（0.125 秒）の間に文字が変わり、箱の幅が伸び縮みする。
/// - `tip-far` は離れていて、届くまでに猶予が過ぎると一度消えてから出直す。`no-tip` には説明が無い（指すと猶予の後に消える）。
/// - アイコンだけのボタン（`tip-icon-*`）には必ず説明が付く（`components::icon_button`）。
/// - `tip-overflow` の説明は箱（640px）に収まらず、止まる → 一周流れる、を繰り返す。
/// - `tip-status` に、手ごとの今の文字と向き（上下）を出す。手を頭より上に掲げて見上げると下に、下から指すと上に出る。
#[derive(Default)]
pub struct TooltipTest {}

impl TooltipTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for TooltipTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let ctl = cx.try_global::<PanelControl>();
        let status = [Hand::Left, Hand::Right]
            .map(|h| {
                let text = ctl.and_then(|c| c.tooltip.get(&h));
                let anchor = ctl
                    .and_then(|c| c.anchor.get(&h))
                    .copied()
                    .unwrap_or_default();
                format!("{h:?}: {} ({anchor:?})", text.map_or("-", |t| t.as_ref()))
            })
            .join(" / ");
        let icon = |id, glyph, tip: SharedString| {
            components::icon_button(&theme, id, glyph, tip, Variant::Default)
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .justify_between()
            .p_3()
            .child(row().child(div().debug_selector(|| "tip-status".into()).child(status)))
            .child(
                row()
                    .child(
                        button(&theme, "tip-short", i18n::t(cx, "tooltip.short"), false)
                            .tooltip(i18n::t(cx, "tooltip.save")),
                    )
                    .child(
                        button(&theme, "tip-long", i18n::t(cx, "tooltip.long"), false)
                            .tooltip(i18n::t(cx, "tooltip.panel_settings")),
                    )
                    .child(button(&theme, "no-tip", i18n::t(cx, "tooltip.none"), false))
                    .child(
                        button(
                            &theme,
                            "tip-overflow",
                            i18n::t(cx, "tooltip.overflow"),
                            false,
                        )
                        .tooltip(i18n::t(cx, "tooltip.overflow_text")),
                    ),
            )
            .child(
                row()
                    .child(icon(
                        "tip-icon-gear",
                        &design::icons::GEAR,
                        i18n::t(cx, "tooltip.settings"),
                    ))
                    .child(icon(
                        "tip-icon-heart",
                        &design::icons::HEART,
                        i18n::t(cx, "tooltip.favorite"),
                    ))
                    .child(icon(
                        "tip-icon-search",
                        &design::icons::MAGNIFYING_GLASS,
                        i18n::t(cx, "tooltip.search"),
                    )),
            )
            .child(
                row().justify_end().child(
                    button(&theme, "tip-far", i18n::t(cx, "tooltip.far"), false)
                        .tooltip(i18n::t(cx, "tooltip.far_away")),
                ),
            )
    }
}
