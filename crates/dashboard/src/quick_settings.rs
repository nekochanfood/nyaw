//! クイック設定: 時刻・日付と設定ボタン、テーマの切り替え、デバイスの電池のカード。

use super::*;

/// これより少ない残量は危険の色にする。
const LOW_BATTERY: f32 = 0.2;

impl Screen {
    pub(crate) fn quick_settings(
        &self,
        theme: &Theme,
        time_width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = chrono::Local::now();
        let ctl = cx.global::<PanelControl>();
        let mut batteries = ctl.batteries.clone();
        let mode = ctl.settings.theme;
        // HMD・左・右・トラッカーの順に並べる（ランタイムが返す順は決まらない）。
        batteries.sort_by_key(|b| match &b.device {
            Device::Hmd => (0, String::new()),
            Device::Controller(Hand::Left) => (1, String::new()),
            Device::Controller(Hand::Right) => (2, String::new()),
            Device::Tracker(id) => (3, id.clone()),
        });
        let mut trackers = 0;
        let cards: Vec<_> = batteries
            .iter()
            .map(|b| {
                let (id, name) = match &b.device {
                    Device::Hmd => ("battery-hmd".into(), i18n::t(cx, "dashboard.hmd")),
                    Device::Controller(Hand::Left) => (
                        "battery-left".into(),
                        i18n::t(cx, "dashboard.left_controller"),
                    ),
                    Device::Controller(Hand::Right) => (
                        "battery-right".into(),
                        i18n::t(cx, "dashboard.right_controller"),
                    ),
                    Device::Tracker(_) => {
                        trackers += 1;
                        (
                            format!("battery-tracker-{trackers}"),
                            i18n::tf(cx, "dashboard.tracker", &[("n", &trackers)]),
                        )
                    }
                };
                battery_card(theme, id, b, name, cx)
            })
            .collect();
        let dashboard = self.dashboard.clone();
        let theme_label = match mode {
            kernel::ThemeMode::Dark => i18n::t(cx, "theme.dark"),
            kernel::ThemeMode::Light => i18n::t(cx, "theme.light"),
        };
        // トラッカーが多いと収まらないので、中身は縦にスクロールする。
        components::panel(theme).child(
            div()
                .id("quick-settings")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                // Android のクイック設定のように、上に時刻・日付と設定へのボタン。
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(spacing::SM))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_baseline()
                                .gap(px(spacing::MD))
                                .child(
                                    div()
                                        .debug_selector(|| "quick-time".into())
                                        .text_size(px(QUICK_TIME_PX))
                                        .child(fixed_clock(time_width)),
                                )
                                .child(
                                    div()
                                        .text_size(px(font_size::MD))
                                        .text_color(rgba(theme.neutral_text_low))
                                        .child(i18n::date(cx, now)),
                                ),
                        )
                        .child(
                            components::icon_button(
                                theme,
                                "quick-settings-open",
                                &icons::GEAR,
                                i18n::t(cx, "dashboard.settings"),
                                Variant::OnBackground,
                            )
                            // バーの設定ボタンと同じ（設定画面がこの画面と入れ替わりにはまる）。
                            .on_click(move |_, _, cx| {
                                let _ = dashboard.update(cx, |d, cx| d.toggle(Page::Settings, cx));
                            }),
                        ),
                )
                .child(
                    grid().child(
                        components::tile(theme, "quick-theme")
                            .h(tile_height())
                            .min_w_0()
                            .child(tile_icon(match mode {
                                kernel::ThemeMode::Dark => &icons::MOON,
                                kernel::ThemeMode::Light => &icons::SUN,
                            }))
                            .child(tile_label(format!(
                                "{} · {theme_label}",
                                i18n::t(cx, "dashboard.theme")
                            )))
                            .on_click(move |_, _, cx| ui::set(Setting::Theme(mode.toggled()), cx)),
                    ),
                )
                .child(section(theme, i18n::t(cx, "dashboard.batteries")))
                .child(if cards.is_empty() {
                    div()
                        .px(px(spacing::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(i18n::t(cx, "dashboard.no_batteries"))
                } else {
                    grid().children(cards)
                }),
        )
    }
}

/// 電池 1 つのカード: デバイスのアイコンと名前、残量（数字と棒）。充電中はアイコンの形と文字でも示す。
fn battery_card(
    theme: &Theme,
    id: String,
    b: &Battery,
    name: gpui::SharedString,
    cx: &App,
) -> gpui::AnyElement {
    let low = b.level < LOW_BATTERY && !b.charging;
    let fill = if low {
        theme.danger_solid
    } else {
        theme.accent_solid
    };
    let device = match b.device {
        Device::Hmd => &icons::VIRTUAL_REALITY,
        Device::Controller(_) => &icons::GAME_CONTROLLER,
        Device::Tracker(_) => &icons::CROSSHAIR,
    };
    components::card(theme)
        .debug_selector(move || id)
        .h(tile_height())
        .min_w_0()
        .p(px(spacing::MD))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .child(design::icon(device).size(px(font_size::LG)))
                .child(tile_label(name).text_left()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .child(design::icon(battery_glyph(b)).size(px(font_size::LG)))
                .child(
                    div()
                        .text_size(px(font_size::LG))
                        .font_weight(FontWeight::BOLD)
                        .child(percent(b.level)),
                )
                .when(b.charging, |d| {
                    d.child(
                        div()
                            .text_size(px(font_size::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::t(cx, "dashboard.charging")),
                    )
                }),
        )
        .child(
            div()
                .h(px(spacing::SM))
                .rounded(px(design::radius::FULL))
                .bg(rgba(theme.groove))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(b.level))
                        .rounded(px(design::radius::FULL))
                        .bg(rgba(fill)),
                ),
        )
        .into_any_element()
}

/// 残量に合った電池のアイコン（充電中は稲妻）。
pub(crate) fn battery_glyph(b: &Battery) -> &'static design::Icon {
    match b.level {
        _ if b.charging => &icons::BATTERY_CHARGING,
        l if l >= 0.9 => &icons::BATTERY_FULL,
        l if l >= 0.6 => &icons::BATTERY_HIGH,
        l if l >= 0.35 => &icons::BATTERY_MEDIUM,
        l if l >= 0.1 => &icons::BATTERY_LOW,
        _ => &icons::BATTERY_EMPTY,
    }
}

fn percent(level: f32) -> String {
    format!("{}%", (level * 100.0).round() as u32)
}

#[cfg(test)]
mod tests;
