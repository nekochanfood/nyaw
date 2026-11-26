//! ポインタ設定（入力カテゴリの `settings-pointer` から開く詳細画面）: カーソルおよびレーザービームの設定。
//! 上から調整モード（`setting-pointer-tuning`: シンプル / 詳細）、動作パラメータ（シンプルは `setting-steadiness` 1 本、
//! 詳細は `setting-smoothing` / `setting-release` / `setting-laser-length`）、カーソル（サイズ・ツールチップ）、
//! 音（タップ音・ホバー音）。すべてリアルタイムに反映される。

use design::components::{self, Button};
use design::{Theme, font_size, spacing};
use gpui::{App, Context, Div, SharedString, Window, div, prelude::*, px, rgba};
use kernel::overlay::{
    LASER_LENGTH_MIN, LASER_MAX, POINTER_RELEASE_DEG, POINTER_RELEASE_DEG_MAX,
    POINTER_RELEASE_DEG_MIN, POINTER_SMOOTHING, POINTER_SMOOTHING_MAX,
};
use kernel::{CURSOR_SCALE_MAX, CURSOR_SCALE_MIN, PointerTuning, SCALE_STEP, Setting, Settings};

use crate::{ROW_SLIDER_PX, change};

/// 動作スライダーの両端アイコン: 左端は即応性が高い（高速追従）、右端は手ブレ補正が強い（安定重視）。
const SPEED: [&design::Icon; 2] = [
    &design::icons::PERSON_SIMPLE_RUN,
    &design::icons::PERSON_SIMPLE_WALK,
];

/// シンプルモードのスライダー位置 `at`（0..=1）から手ブレ補正強度とリリース角度（度）を算出する。中央が既定値。
/// 左半分は強度を既定→0 へ対数変化（1 に近いほど感度変化が大きい）、右半分は角度を既定→上限へ広げる。
fn steadiness(at: f32) -> (f32, f32) {
    let at = at.clamp(0.0, 1.0);
    if at < 0.5 {
        (
            1.0 - (1.0 - POINTER_SMOOTHING).powf(at * 2.0),
            POINTER_RELEASE_DEG,
        )
    } else {
        let wide = POINTER_RELEASE_DEG_MAX / POINTER_RELEASE_DEG;
        (
            POINTER_SMOOTHING,
            POINTER_RELEASE_DEG * wide.powf(at * 2.0 - 1.0),
        )
    }
}

/// 現在の強度と角度からシンプルモードのスライダー位置を逆算する（[`steadiness`] の逆関数）。
/// 詳細モードで範囲外の組み合わせになった場合、強度が既定未満なら強度基準、それ以外は角度基準で決定する。
fn steadiness_at(smoothing: f32, release: f32) -> f32 {
    if smoothing < POINTER_SMOOTHING - 1e-4 {
        0.5 * (1.0 - smoothing.max(0.0)).ln() / (1.0 - POINTER_SMOOTHING).ln()
    } else {
        let wide = POINTER_RELEASE_DEG_MAX / POINTER_RELEASE_DEG;
        0.5 + 0.5 * ((release / POINTER_RELEASE_DEG).ln() / wide.ln()).clamp(0.0, 1.0)
    }
}

/// ページ本体（グループの並び）。上部に調整モード（シンプル / 詳細）の選択行。ポインタ方式の選択は提供しない
/// （マウスは廃止予定、全体はタッチ操作）。
pub(crate) fn page(
    s: &crate::Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<crate::Settings>,
) -> Vec<Div> {
    let now = ui::settings(cx);
    let tuning = s.select(
        theme,
        window,
        cx,
        "setting-pointer-tuning",
        &design::icons::SLIDERS_HORIZONTAL,
        i18n::t(cx, "settings.pointer_tuning"),
        now.pointer_tuning,
        vec![
            (
                "setting-tuning-simple",
                i18n::t(cx, "settings.simple"),
                PointerTuning::Simple,
            ),
            (
                "setting-tuning-detailed",
                i18n::t(cx, "settings.detailed"),
                PointerTuning::Detailed,
            ),
        ],
        Setting::PointerTuning,
    );
    body(theme, now, tuning, cx)
}

/// `tuning` 行の下に配置する本体（動作・カーソル・音の各グループ）。
fn body(theme: &Theme, now: Settings, tuning: Button, cx: &App) -> Vec<Div> {
    let t = |key: &str| i18n::t(cx, key);
    let mut groups = vec![components::list(theme, [tuning])];
    groups.extend(match now.pointer_tuning {
        PointerTuning::Simple => vec![simple(theme, now, cx)],
        PointerTuning::Detailed => detailed(theme, now, cx),
    });

    // カーソルサイズは `SCALE_STEP` にスナップし、ステップ境界を越えた場合のみ設定を更新する。
    let (min, max, scale) = (CURSOR_SCALE_MIN, CURSOR_SCALE_MAX, now.cursor_scale);
    let size = components::slider_item(
        theme,
        "setting-cursor-size",
        &design::icons::CURSOR,
        t("settings.cursor_size"),
        format!("{:.0}%", scale * 100.0),
        components::slider(
            theme,
            "setting-cursor-size-slider",
            (scale - min) / (max - min),
            move |at, _, cx| {
                let to = kernel::config::clamp_scale(min + at * (max - min), min, max);
                if (to - scale).abs() > SCALE_STEP / 2.0 {
                    ui::set(Setting::CursorScale(to), cx)
                }
            },
        )
        .length(ROW_SLIDER_PX),
    );
    let switch = |id: &'static str,
                  glyph: &'static design::Icon,
                  key: &str,
                  on: bool,
                  set: fn(bool) -> Setting| {
        components::switch_item(theme, id, glyph, t(key), on).on_click(change(set(!on)))
    };
    let tooltips = switch(
        "setting-tooltips",
        &design::icons::CHAT_TEXT,
        "settings.tooltips",
        now.tooltips,
        Setting::Tooltips,
    );
    groups.push(components::group(
        theme,
        t("settings.cursor"),
        components::list(theme, [size, tooltips]),
    ));
    let sounds = [
        switch(
            "setting-tap-sound",
            &design::icons::HAND_TAP,
            "settings.tap_sound",
            now.tap_sound,
            Setting::TapSound,
        ),
        switch(
            "setting-hover-sound",
            &design::icons::HAND_POINTING,
            "settings.hover_sound",
            now.hover_sound,
            Setting::HoverSound,
        ),
    ];
    groups.push(components::group(
        theme,
        t("settings.sounds"),
        components::list(theme, sounds),
    ));
    groups
}

/// シンプルモード: 1 本のスライダーで手ブレ補正強度とリリース角度を連動して調整する。
fn simple(theme: &Theme, now: Settings, cx: &App) -> Div {
    let at = steadiness_at(now.pointer_smoothing, now.pointer_release_deg);
    components::group(
        theme,
        i18n::t(cx, "settings.smoothing"),
        setting_slider(
            theme,
            i18n::t(cx, "settings.steadiness"),
            components::slider(theme, "setting-steadiness", at, |at, _, cx| {
                let (smoothing, release) = steadiness(at);
                ui::set(Setting::PointerSmoothing(smoothing), cx);
                ui::set(Setting::PointerRelease(release), cx);
            }),
            Some(SPEED),
        ),
    )
}

/// 詳細モード: 手ブレ補正強度・リリース角度・レーザー長（線が減衰する距離）を個別に設定する（Laser 画面と同一範囲）。
fn detailed(theme: &Theme, now: Settings, cx: &App) -> Vec<Div> {
    let max = POINTER_SMOOTHING_MAX;
    let (tight, loose) = (POINTER_RELEASE_DEG_MIN, POINTER_RELEASE_DEG_MAX);
    let (short, long) = (LASER_LENGTH_MIN, LASER_MAX);
    let row = |title: &str, value: SharedString, slider, ends| {
        components::group(
            theme,
            i18n::t(cx, title),
            setting_slider(theme, value, slider, ends),
        )
    };
    vec![
        row(
            "settings.smoothing",
            i18n::tf(
                cx,
                "laser.strength",
                &[
                    ("now", &format!("{:.2}", now.pointer_smoothing)),
                    ("max", &format!("{max:.2}")),
                ],
            ),
            components::slider(
                theme,
                "setting-smoothing",
                now.pointer_smoothing / max,
                move |v, _, cx| ui::set(Setting::PointerSmoothing(v * max), cx),
            ),
            Some(SPEED),
        ),
        row(
            "settings.release",
            i18n::tf(
                cx,
                "laser.release",
                &[
                    ("release", &format!("{:.1}", now.pointer_release_deg)),
                    ("tight", &format!("{tight:.1}")),
                    ("loose", &format!("{loose:.0}")),
                ],
            ),
            components::slider(
                theme,
                "setting-release",
                (now.pointer_release_deg - tight) / (loose - tight),
                move |v, _, cx| ui::set(Setting::PointerRelease(tight + v * (loose - tight)), cx),
            ),
            Some(SPEED),
        ),
        row(
            "settings.laser_length",
            i18n::tf(
                cx,
                "laser.length",
                &[
                    ("length", &format!("{:.2}", now.laser_length)),
                    ("short", &format!("{short:.2}")),
                    ("long", &format!("{long:.2}")),
                ],
            ),
            components::slider(
                theme,
                "setting-laser-length",
                (now.laser_length - short) / (long - short),
                move |v, _, cx| ui::set(Setting::LaserLength(short + v * (long - short)), cx),
            ),
            None,
        ),
    ]
}

/// スライダーカード: スライダーをカード全幅に配置し、現在値の説明をカード下部に表示する。
/// `ends` 指定時はスライダー両端に意味を示すアイコンを配置する。
fn setting_slider(
    theme: &Theme,
    value: SharedString,
    slider: components::Slider,
    ends: Option<[&'static design::Icon; 2]>,
) -> Div {
    let end = |glyph| {
        design::icon(glyph)
            .size(px(font_size::LG))
            .color(rgba(theme.neutral_text_low))
    };
    let (left, right) = match ends {
        Some([l, r]) => (Some(end(l)), Some(end(r))),
        None => (None, None),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::SM))
        .child(
            components::card(theme)
                .p(px(spacing::LG))
                .flex()
                .items_center()
                .gap(px(spacing::MD))
                .children(left)
                .child(div().flex_1().child(slider.fill()))
                .children(right),
        )
        .child(
            div()
                .px(px(spacing::LG))
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(value),
        )
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext};
    use ui::PanelControl;

    use super::*;
    use crate::{Category, PX, Page};

    /// 中央が既定値、左端は補正なし、右端は角度上限。位置と値の組は往復変換可能。強度は左→右で増加、角度は拡大する。
    #[test]
    fn steadiness_is_default_in_the_middle_and_round_trips() {
        assert_eq!(steadiness(0.5), (POINTER_SMOOTHING, POINTER_RELEASE_DEG));
        assert_eq!(steadiness(0.0), (0.0, POINTER_RELEASE_DEG));
        let (s, r) = steadiness(1.0);
        assert_eq!(s, POINTER_SMOOTHING);
        assert!((r - POINTER_RELEASE_DEG_MAX).abs() < 1e-3);
        assert_eq!(steadiness(-1.0), steadiness(0.0), "範囲外は端にクランプ");
        assert_eq!(steadiness(9.0), steadiness(1.0));
        let mut last = steadiness(0.0);
        for i in 0..=20 {
            let at = i as f32 / 20.0;
            let (s, r) = steadiness(at);
            assert!((steadiness_at(s, r) - at).abs() < 1e-3, "{at}");
            assert!(s >= last.0 && r >= last.1, "{at}");
            assert!((0.0..=POINTER_SMOOTHING_MAX).contains(&s));
            assert!((POINTER_RELEASE_DEG_MIN..=POINTER_RELEASE_DEG_MAX).contains(&r));
            last = (s, r);
        }
        // 詳細モードで範囲外の組み合わせも、スライダーの有効範囲内に収まる。
        assert_eq!(
            steadiness_at(POINTER_SMOOTHING, POINTER_RELEASE_DEG_MIN),
            0.5
        );
        assert_eq!(steadiness_at(POINTER_SMOOTHING_MAX, 999.0), 1.0);
        assert_eq!(steadiness_at(-1.0, 30.0), 0.0);
    }

    /// ポインタ動作スライダーはカード全幅（両端アイコン分の均等余白あり）に配置され、ハンドルは値に対応する位置にあり、
    /// 右端まで操作可能。
    #[gpui::test]
    fn pointer_sliders_span_the_card(cx: &mut TestAppContext) {
        use kernel::PointerTuning;
        let (_, cx) = cx.add_window_view(|_, cx| {
            let settings = crate::Settings {
                category: Category::Input,
                pages: vec![Page::Pointer],
                ..crate::Settings::new(0)
            };
            ui::PanelRoot::new(cx.new(|_| settings).into())
        });
        use kernel::overlay::{
            POINTER_RELEASE_DEG, POINTER_RELEASE_DEG_MAX, POINTER_SMOOTHING, POINTER_SMOOTHING_MAX,
        };
        for width in [PX.0, 800] {
            cx.simulate_resize(gpui::size(px(width as f32), px(PX.1 as f32)));
            // (調整モード, スライダーとハンドルの ID, 手ブレ補正強度, スライダー位置)。シンプルの既定値は中央。
            for (tuning, (id, handle), smoothing, value) in [
                (
                    PointerTuning::Simple,
                    ("setting-steadiness", "setting-steadiness-handle"),
                    POINTER_SMOOTHING,
                    0.5,
                ),
                (
                    PointerTuning::Detailed,
                    ("setting-smoothing", "setting-smoothing-handle"),
                    POINTER_SMOOTHING_MAX / 4.0,
                    0.25,
                ),
            ] {
                cx.update(|window, cx| {
                    let c = cx.default_global::<PanelControl>();
                    c.settings.pointer_tuning = tuning;
                    (c.settings.pointer_smoothing, c.settings.pointer_release_deg) =
                        (smoothing, POINTER_RELEASE_DEG);
                    window.refresh();
                });
                cx.run_until_parked();
                let row = cx.debug_bounds("setting-cursor-size").unwrap();
                let slider = cx.debug_bounds(id).unwrap();
                let handle = cx.debug_bounds(handle).unwrap();
                let at = format!("{width}px {id}: {slider:?} in {row:?}");
                let (l, r) = (slider.left() - row.left(), row.right() - slider.right());
                assert!((l - r).abs() < px(1.0) && r < px(80.0), "{at}");
                let travel = slider.size.width - handle.size.width;
                let expect = slider.left() + handle.size.width / 2.0 + travel * value;
                assert!(
                    (handle.center().x - expect).abs() < px(1.0),
                    "{at}: {handle:?}"
                );

                let from = handle.center();
                let to = gpui::point(slider.right() + px(40.0), from.y);
                cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
                cx.simulate_mouse_move(to, gpui::MouseButton::Left, Modifiers::none());
                cx.simulate_mouse_up(to, gpui::MouseButton::Left, Modifiers::none());
                let changes = cx.update(|_, cx| {
                    std::mem::take(&mut cx.default_global::<PanelControl>().setting_changes)
                });
                let at_end = |c: &Setting| match (tuning, *c) {
                    (PointerTuning::Simple, Setting::PointerRelease(v)) => {
                        (v - POINTER_RELEASE_DEG_MAX).abs() < 1e-3
                    }
                    (PointerTuning::Detailed, Setting::PointerSmoothing(v)) => {
                        (v - POINTER_SMOOTHING_MAX).abs() < 1e-6
                    }
                    _ => false,
                };
                assert!(changes.iter().any(at_end), "{at}: {changes:?}");
            }
        }
    }
}
