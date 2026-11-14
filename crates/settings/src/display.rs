//! 表示（`settings-display`）: ダッシュボードの置き場所・距離 LOD の品質・ミラーリングの取得の上限 FPS。どれも 1〜2 行なので
//! 詳しい画面には分けず、見出し付きのグループで並べる。

use design::Theme;
use design::components;
use gpui::{App, Context, Div, SharedString, Window};
use kernel::{LodQuality, Setting};

use crate::{ROW_SLIDER_PX, Settings};

/// ミラーリングの FPS の選択肢。
const MIRROR_FRAME_RATES: [(&str, u32); 5] = [
    ("setting-fps-30", 30),
    ("setting-fps-60", 60),
    ("setting-fps-90", 90),
    ("setting-fps-120", 120),
    ("setting-fps-144", 144),
];

pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let now = ui::settings(cx);
    // ダッシュボード: 距離と高さ（頭の高さからの相対）。決めるとこの画面ごと動くので、離したときに決める。
    let (distance, height) = (
        i18n::t(cx, "settings.dashboard_distance"),
        i18n::t(cx, "settings.dashboard_height"),
    );
    let dashboard = [
        placement_slider(
            theme,
            window,
            cx,
            (
                "setting-dashboard-distance",
                "setting-dashboard-distance-slider",
            ),
            &design::icons::PERSPECTIVE,
            distance,
            now.dashboard_distance,
            (
                kernel::DASHBOARD_DISTANCE_MIN,
                kernel::DASHBOARD_DISTANCE_MAX,
            ),
            |d| format!("{d:.2} m"),
            Setting::DashboardDistance,
        ),
        placement_slider(
            theme,
            window,
            cx,
            (
                "setting-dashboard-height",
                "setting-dashboard-height-slider",
            ),
            &design::icons::ARROWS_VERTICAL,
            height,
            now.dashboard_height,
            (kernel::DASHBOARD_HEIGHT_MIN, kernel::DASHBOARD_HEIGHT_MAX),
            |h| format!("{h:+.2} m"),
            Setting::DashboardHeight,
        ),
    ];
    // グラフィックス: 距離 LOD の品質。
    let lod = s.select(
        theme,
        window,
        cx,
        "setting-lod-quality",
        &design::icons::IMAGE,
        i18n::t(cx, "settings.lod_quality"),
        now.lod_quality,
        [
            ("setting-lod-off", "settings.lod.off", LodQuality::Off),
            ("setting-lod-low", "settings.lod.low", LodQuality::Low),
            ("setting-lod-mid", "settings.lod.mid", LodQuality::Mid),
            ("setting-lod-high", "settings.lod.high", LodQuality::High),
        ]
        .map(|(id, key, q)| (id, i18n::t(cx, key), q))
        .to_vec(),
        Setting::LodQuality,
    );
    // ミラーリング: 取得の上限 FPS。
    let mirror = s.select(
        theme,
        window,
        cx,
        "setting-mirror-fps",
        &design::icons::GAUGE,
        i18n::t(cx, "settings.mirror_frame_rate"),
        now.mirror_frame_rate,
        MIRROR_FRAME_RATES
            .map(|(id, n)| (id, SharedString::from(format!("{n} FPS")), n))
            .to_vec(),
        Setting::MirrorFrameRate,
    );
    let group = |key: &str, rows: Vec<components::Button>| {
        components::group(theme, i18n::t(cx, key), components::list(theme, rows))
    };
    vec![
        group("settings.category.dashboard", dashboard.into()),
        group("settings.category.graphics", vec![lod]),
        group("settings.category.mirroring", vec![mirror]),
    ]
}

/// 決めると自分の置き場所が変わる長さ（m）のスライダーの行（`ids` は行とスライダー）。動かしている間は値だけを見せて
/// 離したときに `set` で決める（`ui::prefs` の UI スケールと同じ）。`min..=max` を `kernel::DASHBOARD_STEP` に吸い付かせる。
#[allow(clippy::too_many_arguments)]
fn placement_slider(
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    (id, slider_id): (&'static str, &'static str),
    glyph: &'static design::Icon,
    label: SharedString,
    now: f32,
    (min, max): (f32, f32),
    show: fn(f32) -> String,
    set: fn(f32) -> Setting,
) -> components::Button {
    let step = kernel::DASHBOARD_STEP;
    // 動かしている途中の値と、離したか（離したら決めたことにし、設定が追いつくまで値はそのまま見せる）。
    let key = SharedString::from(format!("{id}-draft"));
    let draft = window.use_keyed_state(key, cx, |_, _| None::<(f32, bool)>);
    let pending = (*draft.read(cx)).filter(|&(to, done)| !(done && (to - now).abs() < step / 2.0));
    if pending.is_none() && draft.read(cx).is_some() {
        draft.update(cx, |d, _| *d = None);
    }
    let value = pending.map_or(now, |(to, _)| to);
    let (moving, done) = (draft.clone(), draft);
    let slider = components::slider(
        theme,
        slider_id,
        (value - min) / (max - min),
        move |at, _, cx| {
            let to = kernel::config::snap(min + at * (max - min), min, max, step, now);
            moving.update(cx, |d, cx| {
                if *d != Some((to, false)) {
                    *d = Some((to, false));
                    cx.notify();
                }
            })
        },
    )
    .on_release(move |_, cx| {
        let to = done.update(cx, |d, cx| {
            cx.notify();
            d.as_mut().map(|(to, released)| {
                *released = true;
                *to
            })
        });
        if let Some(to) = to {
            ui::set(set(to), cx)
        }
    })
    .length(ROW_SLIDER_PX);
    components::slider_item(theme, id, glyph, label, show(value), slider)
}
