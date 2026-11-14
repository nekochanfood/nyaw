//! 設定の「ポインタ」（`settings-pointer`）: カーソルとビームの設定。上から動きの調整のしかた（`setting-pointer-tuning`。
//! シンプル / 詳細）、動き（シンプルは 1 本の `setting-steadiness`、詳細は `setting-smoothing` / `setting-release` /
//! `setting-laser-length`）、カーソル（大きさ・ツールチップ）、音（タップ音・ホバー音）。どれも動かすとすぐ効く。

use design::Theme;
use design::components::{self, Button};
use gpui::{App, Div, SharedString};
use kernel::overlay::{
    LASER_LENGTH_MIN, LASER_MAX, POINTER_RELEASE_DEG, POINTER_RELEASE_DEG_MAX,
    POINTER_RELEASE_DEG_MIN, POINTER_SMOOTHING, POINTER_SMOOTHING_MAX,
};
use kernel::{CURSOR_SCALE_MAX, CURSOR_SCALE_MIN, PointerTuning, SCALE_STEP, Setting, Settings};

use crate::{ROW_SLIDER_PX, change, setting_slider};

/// シンプルのスライダーの位置 `at`（0..=1）の、手ブレ抑えの強さと緩める角度（度）。真ん中が既定値。
/// 左半分は強さを既定から 0 へ（1 − 強さを対数で。1 に近いほど効き目が大きく変わる）、右半分は角度を既定から上限へ広げる。
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

/// 今の強さと角度の、シンプルのスライダーの位置（[`steadiness`] の逆）。詳細で外れた組は、強さが既定より弱ければ強さで、
/// そうでなければ角度で決める。
fn steadiness_at(smoothing: f32, release: f32) -> f32 {
    if smoothing < POINTER_SMOOTHING - 1e-4 {
        0.5 * (1.0 - smoothing.max(0.0)).ln() / (1.0 - POINTER_SMOOTHING).ln()
    } else {
        let wide = POINTER_RELEASE_DEG_MAX / POINTER_RELEASE_DEG;
        0.5 + 0.5 * ((release / POINTER_RELEASE_DEG).ln() / wide.ln()).clamp(0.0, 1.0)
    }
}

/// 中身（グループの並び）。`tuning` は調整のしかたを選ぶ行（設定画面の選択肢）。
pub(crate) fn page(theme: &Theme, now: Settings, tuning: Button, cx: &App) -> Vec<Div> {
    let t = |key: &str| i18n::t(cx, key);
    let mut groups = vec![components::list(theme, [tuning])];
    groups.extend(match now.pointer_tuning {
        PointerTuning::Simple => vec![simple(theme, now, cx)],
        PointerTuning::Detailed => detailed(theme, now, cx),
    });

    // カーソルの大きさは刻み（`SCALE_STEP`）に吸い付く。刻みが変わったときだけ設定を変える。
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

/// シンプル: 1 本のスライダーで手ブレ抑えの強さと緩める角度をまとめて動かす。
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
        ),
    )
}

/// 詳細: 手ブレ抑えの強さ・緩める角度・レーザーの長さ（線が消えていく長さ）をそれぞれ（Laser の画面と同じ範囲）。
fn detailed(theme: &Theme, now: Settings, cx: &App) -> Vec<Div> {
    let max = POINTER_SMOOTHING_MAX;
    let (tight, loose) = (POINTER_RELEASE_DEG_MIN, POINTER_RELEASE_DEG_MAX);
    let (short, long) = (LASER_LENGTH_MIN, LASER_MAX);
    let row = |title: &str, value: SharedString, slider| {
        components::group(
            theme,
            i18n::t(cx, title),
            setting_slider(theme, value, slider),
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
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真ん中が既定値、左端は抑えない、右端は角度が上限。位置と組は行き来でき、強さは左から右へ強まり、角度は広がる。
    #[test]
    fn steadiness_is_default_in_the_middle_and_round_trips() {
        assert_eq!(steadiness(0.5), (POINTER_SMOOTHING, POINTER_RELEASE_DEG));
        assert_eq!(steadiness(0.0), (0.0, POINTER_RELEASE_DEG));
        let (s, r) = steadiness(1.0);
        assert_eq!(s, POINTER_SMOOTHING);
        assert!((r - POINTER_RELEASE_DEG_MAX).abs() < 1e-3);
        assert_eq!(steadiness(-1.0), steadiness(0.0), "範囲の外は端");
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
        // 詳細で道から外れた組も、スライダーの範囲に収まる。
        assert_eq!(
            steadiness_at(POINTER_SMOOTHING, POINTER_RELEASE_DEG_MIN),
            0.5
        );
        assert_eq!(steadiness_at(POINTER_SMOOTHING_MAX, 999.0), 1.0);
        assert_eq!(steadiness_at(-1.0, 30.0), 0.0);
    }
}
