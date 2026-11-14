//! レーザーの手ブレ抑えを試す画面（[`SmoothingTest`]）。

use super::*;

/// レーザーを調整する画面（`smoothing-test`）。`smoothing-slider` で手ブレ抑えの強さを、`laser-length-slider` で
/// 線の見える長さを変えるとその場で効き、設定ファイルの `pointer_smoothing` / `laser_length` にも残る
/// （`Setting::PointerSmoothing` / `LaserLength`）。下の小さな的を狙って、止めたときの揺れと、振ったときの遅れを見比べる。
pub struct SmoothingTest {}

impl SmoothingTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

/// 狙う的の数と大きさ（px）。小さいほど揺れが分かる。
const AIM_TARGETS: usize = 8;

const AIM_PX: f32 = 32.0;

impl Render for SmoothingTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let max = kernel::overlay::POINTER_SMOOTHING_MAX;
        // フレームループが書き戻す前（ビューだけのテスト）は既定値。
        let current = cx.try_global::<PanelControl>().map(|c| c.settings);
        let now = current.map_or(kernel::overlay::POINTER_SMOOTHING, |s| s.pointer_smoothing);
        let length = current.map_or(kernel::overlay::LASER_LENGTH, |s| s.laser_length);
        let (short, long) = (
            kernel::overlay::LASER_LENGTH_MIN,
            kernel::overlay::LASER_MAX,
        );
        let release = current.map_or(kernel::overlay::POINTER_RELEASE_DEG, |s| {
            s.pointer_release_deg
        });
        let (tight, loose) = (
            kernel::overlay::POINTER_RELEASE_DEG_MIN,
            kernel::overlay::POINTER_RELEASE_DEG_MAX,
        );
        let preset = |id: &'static str, label: SharedString, v: f32| {
            button(&theme, id, label, (now - v).abs() < 1e-3)
                .on_click(change(Setting::PointerSmoothing(v)))
        };
        let length_preset = |id: &'static str, label: SharedString, v: f32| {
            button(&theme, id, label, (length - v).abs() < 1e-3)
                .on_click(change(Setting::LaserLength(v)))
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(
                div()
                    .debug_selector(|| "smoothing-value".into())
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(
                        cx,
                        "laser.strength",
                        &[("now", &format!("{now:.2}")), ("max", &format!("{max:.2}"))],
                    )),
            )
            .child(
                components::slider(&theme, "smoothing-slider", now / max, move |v, _, cx| {
                    ui::set(Setting::PointerSmoothing(v * max), cx)
                })
                .length(560.0),
            )
            .child(
                row()
                    .child(preset("smoothing-off", i18n::t(cx, "laser.off"), 0.0))
                    .child(preset(
                        "smoothing-default",
                        i18n::t(cx, "common.default"),
                        kernel::overlay::POINTER_SMOOTHING,
                    ))
                    .child(preset("smoothing-strong", i18n::t(cx, "laser.light"), 0.95)),
            )
            .child(
                div()
                    .debug_selector(|| "release-value".into())
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(
                        cx,
                        "laser.release",
                        &[
                            ("release", &format!("{release:.1}")),
                            ("tight", &format!("{tight:.1}")),
                            ("loose", &format!("{loose:.0}")),
                        ],
                    )),
            )
            .child(
                components::slider(
                    &theme,
                    "release-slider",
                    (release - tight) / (loose - tight),
                    move |v, _, cx| {
                        ui::set(Setting::PointerRelease(tight + v * (loose - tight)), cx)
                    },
                )
                .length(560.0),
            )
            .child(
                div()
                    .debug_selector(|| "laser-length-value".into())
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(
                        cx,
                        "laser.length",
                        &[
                            ("length", &format!("{length:.2}")),
                            ("short", &format!("{short:.2}")),
                            ("long", &format!("{long:.2}")),
                        ],
                    )),
            )
            .child(
                components::slider(
                    &theme,
                    "laser-length-slider",
                    (length - short) / (long - short),
                    move |v, _, cx| ui::set(Setting::LaserLength(short + v * (long - short)), cx),
                )
                .length(560.0),
            )
            .child(
                row()
                    .child(length_preset(
                        "laser-short",
                        i18n::t(cx, "laser.short"),
                        0.15,
                    ))
                    .child(length_preset(
                        "laser-medium",
                        i18n::t(cx, "common.default"),
                        kernel::overlay::LASER_LENGTH,
                    ))
                    .child(length_preset("laser-full", i18n::t(cx, "laser.full"), long)),
            )
            .child(
                // 3 つとも既定値へ戻す（設定ファイルにも既定値が書き戻される）。
                row().child(
                    button(&theme, "laser-reset", i18n::t(cx, "laser.reset"), false).on_click(
                        move |_, _, cx| {
                            for s in [
                                Setting::PointerSmoothing(kernel::overlay::POINTER_SMOOTHING),
                                Setting::PointerRelease(kernel::overlay::POINTER_RELEASE_DEG),
                                Setting::LaserLength(kernel::overlay::LASER_LENGTH),
                            ] {
                                ui::set(s, cx);
                            }
                        },
                    ),
                ),
            )
            .child(
                // 狙う的。指している間だけ強調色になる。
                row().children((0..AIM_TARGETS).map(|i| {
                    div()
                        .id(("aim", i))
                        .size(px(AIM_PX))
                        .rounded(px(radius::FULL))
                        .bg(rgba(theme.control))
                        .hover(|s| s.bg(rgba(theme.accent_solid)))
                })),
            )
    }
}
