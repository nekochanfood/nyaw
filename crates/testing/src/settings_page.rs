//! 設定画面（`settings-test`）。左のサイドバーにカテゴリ、右に選んだカテゴリの設定。
//!
//! 見た目は ciel の設定（見出しの付いた角丸のカードに行を並べる）と GNOME の設定（行は題と補足、右に操作）に倣う。
//! 角丸は nyaw のもの（カードは `radius::LG`）。値はカーネルの設定（`Setting`）をそのまま読み書きし、設定ファイルにも残る。

use design::components::{self, Variant};
use design::{Theme, font_size, radius, spacing};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, Render, SharedString, Window, div,
    prelude::*, px, rgba,
};
use kernel::subsystems::input::HandTuning;
use kernel::{PanelId, PointerMode, Setting, ThemeMode};
use ui::PanelSpec;

use super::{change, heading, lang, language, push_setting, settings, stack_op};

/// 設定画面の大きさ（m）と解像度（px）。1px = 1mm。
pub const SETTINGS_SIZE: kernel::vr::Vec2 = kernel::vr::Vec2::new(1.024, 0.768);
pub const SETTINGS_PX: (u32, u32) = (1024, 768);
/// サイドバーの幅（px）。
const SIDEBAR_PX: f32 = 280.0;
/// 右に操作を置く行のスライダーの長さ（px）。
const SLIDER_PX: f32 = 280.0;
/// バージョンの札のアイコン（ロゴの代わり）の枠の大きさ（px）。
const LOGO_PX: f32 = 96.0;

/// 設定のカテゴリ: `debug_selector`・アイコン・文言のキー。
const CATEGORIES: [(&str, &design::Icon, &str); 4] = [
    (
        "settings-general",
        &design::icons::GEAR,
        "settings.category.general",
    ),
    (
        "settings-appearance",
        &design::icons::PALETTE,
        "settings.category.appearance",
    ),
    (
        "settings-controls",
        &design::icons::CURSOR_CLICK,
        "settings.category.controls",
    ),
    (
        "settings-about",
        &design::icons::INFO,
        "settings.category.about",
    ),
];
const GENERAL: usize = 0;
const APPEARANCE: usize = 1;
const CONTROLS: usize = 2;
const ABOUT: usize = 3;

/// 中身に積む画面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    /// カテゴリの設定。
    Category,
    /// 手の調整（操作のカテゴリから入る）。
    Hand,
}

/// 設定画面の起動仕様（ダッシュボードの設定ボタン。main が渡す）。`opened` は作られたパネルの ID を受け取る。
pub fn settings_spec(
    from: PanelId,
    opened: impl FnOnce(PanelId, &mut gpui::App) + 'static,
) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, cx| {
        opened(id, cx);
        Settings::new(id)
    })
    .sized(SETTINGS_SIZE, SETTINGS_PX)
    .titled("testing.open.settings")
}

/// 設定画面。中身は積み重ね（[`components::navigation`]）: 「手の調整」を押すと右から入り、`settings-back` か右から左へのスワイプで戻る。
#[derive(Default)]
pub struct Settings {
    /// 選んでいるカテゴリ（[`CATEGORIES`] の添字）。
    pub category: usize,
    pub pages: Vec<SettingsPage>,
}

impl Settings {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            category: GENERAL,
            pages: vec![SettingsPage::Category],
        }
    }
}

/// 見出し付きのカード: 上に小さな見出し、下に行を縦に並べ、行の間に区切り線を引く（角丸は `LG`。行の塗りは角で切る）。
fn group(theme: &Theme, title: Option<SharedString>, rows: Vec<AnyElement>) -> Div {
    let line = theme.neutral_border_subtle;
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::SM))
        .children(title.map(|t| {
            div()
                .px(px(spacing::LG))
                .text_size(px(font_size::SM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(theme.neutral_text_low))
                .child(t)
        }))
        .child(
            components::card(theme)
                .flex()
                .flex_col()
                .overflow_hidden()
                .children(rows.into_iter().enumerate().flat_map(move |(i, r)| {
                    let divider =
                        (i > 0).then(|| div().h(px(1.0)).bg(rgba(line)).into_any_element());
                    divider.into_iter().chain([r])
                })),
        )
}

/// 行: 左にアイコン・題・補足、右に操作（`suffix`）。
fn row(
    theme: &Theme,
    id: &'static str,
    glyph: &'static design::Icon,
    title: impl Into<SharedString>,
    hint: Option<SharedString>,
    suffix: impl IntoElement,
) -> AnyElement {
    div()
        .debug_selector(move || id.into())
        .min_h(px(components::BUTTON_HEIGHT + spacing::MD))
        .px(px(spacing::LG))
        .py(px(spacing::SM))
        .flex()
        .items_center()
        .gap(px(spacing::MD))
        .child(design::icon(glyph).size(px(font_size::LG)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().child(title.into()))
                .children(hint.map(|h| {
                    div()
                        .text_size(px(font_size::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(h)
                })),
        )
        .child(suffix)
        .into_any_element()
}

/// 選び方が少ない設定の操作: 選択肢のボタンを横に並べ、選んでいるものが光る。
fn choices(theme: &Theme, options: Vec<components::Button>) -> Div {
    let _ = theme;
    div().flex().gap(px(spacing::SM)).children(options)
}

/// ラベルと値だけの行（ciel のバージョン情報の行。値は等幅）。
fn info_row(theme: &Theme, id: &'static str, label: SharedString, value: String) -> AnyElement {
    div()
        .debug_selector(move || id.into())
        .min_h(px(components::BUTTON_HEIGHT))
        .px(px(spacing::LG))
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_color(rgba(theme.neutral_text_low)).child(label))
        .child(div().font_family("monospace").child(value))
        .into_any_element()
}

impl Settings {
    fn general(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<Div> {
        let s = settings(cx);
        let now = lang(s.language);
        let languages = choices(theme, design_langs(theme, now, cx));
        let modes = choices(
            theme,
            vec![
                mode_button(
                    theme,
                    "pointer-mouse",
                    i18n::t(cx, "settings.mouse"),
                    s.pointer_mode == PointerMode::Mouse,
                    PointerMode::Mouse,
                ),
                mode_button(
                    theme,
                    "pointer-touch",
                    i18n::t(cx, "settings.touch"),
                    s.pointer_mode == PointerMode::Touch,
                    PointerMode::Touch,
                ),
            ],
        );
        vec![
            group(
                theme,
                None,
                vec![row(
                    theme,
                    "setting-language",
                    &design::icons::GLOBE,
                    i18n::t(cx, "settings.language"),
                    Some(i18n::t(cx, "settings.language_hint")),
                    languages,
                )],
            ),
            group(
                theme,
                None,
                vec![row(
                    theme,
                    "setting-pointer-mode",
                    &design::icons::CURSOR_CLICK,
                    i18n::t(cx, "settings.pointer_mode"),
                    Some(i18n::t(cx, "settings.pointer_mode_hint")),
                    modes,
                )],
            ),
        ]
    }

    fn appearance(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<Div> {
        let current = settings(cx).theme;
        let button = |id: &'static str, key: &str, mode: ThemeMode| {
            components::button(theme, id, i18n::t(cx, key), Variant::Default)
                .selected(current == mode)
                .on_click(change(Setting::Theme(mode)))
        };
        vec![group(
            theme,
            None,
            vec![row(
                theme,
                "setting-theme",
                &design::icons::PALETTE,
                i18n::t(cx, "settings.theme"),
                Some(i18n::t(cx, "settings.theme_hint")),
                choices(
                    theme,
                    vec![
                        button("theme-dark", "theme.dark", ThemeMode::Dark),
                        button("theme-light", "theme.light", ThemeMode::Light),
                    ],
                ),
            )],
        )]
    }

    fn controls(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<Div> {
        let s = settings(cx);
        let (smooth_max, (short, long), (tight, loose)) = (
            kernel::overlay::POINTER_SMOOTHING_MAX,
            (
                kernel::overlay::LASER_LENGTH_MIN,
                kernel::overlay::LASER_MAX,
            ),
            (
                kernel::overlay::POINTER_RELEASE_DEG_MIN,
                kernel::overlay::POINTER_RELEASE_DEG_MAX,
            ),
        );
        let this = cx.entity();
        let hand_row = components::list_item(
            theme,
            "setting-hand",
            &design::icons::HAND_POINTING,
            i18n::t(cx, "settings.hand"),
        )
        .on_click(stack_op(
            &this,
            |v: &mut Settings| &mut v.pages,
            Some(SettingsPage::Hand),
        ));
        vec![
            group(
                theme,
                Some(i18n::t(cx, "settings.laser")),
                vec![
                    row(
                        theme,
                        "setting-smoothing",
                        &design::icons::WAVE_SINE,
                        i18n::t(cx, "settings.smoothing"),
                        Some(format!("{:.2}", s.pointer_smoothing).into()),
                        components::slider(
                            theme,
                            "smoothing-slider",
                            s.pointer_smoothing / smooth_max,
                            move |v, _, cx| {
                                push_setting(Setting::PointerSmoothing(v * smooth_max), cx)
                            },
                        )
                        .length(SLIDER_PX),
                    ),
                    row(
                        theme,
                        "setting-release",
                        &design::icons::ARROWS_OUT_LINE_HORIZONTAL,
                        i18n::t(cx, "settings.release"),
                        Some(format!("{:.1}°", s.pointer_release_deg).into()),
                        components::slider(
                            theme,
                            "release-slider",
                            (s.pointer_release_deg - tight) / (loose - tight),
                            move |v, _, cx| {
                                push_setting(
                                    Setting::PointerRelease(tight + v * (loose - tight)),
                                    cx,
                                )
                            },
                        )
                        .length(SLIDER_PX),
                    ),
                    row(
                        theme,
                        "setting-length",
                        &design::icons::RULER,
                        i18n::t(cx, "settings.length"),
                        Some(format!("{:.2} m", s.laser_length).into()),
                        components::slider(
                            theme,
                            "laser-length-slider",
                            (s.laser_length - short) / (long - short),
                            move |v, _, cx| {
                                push_setting(Setting::LaserLength(short + v * (long - short)), cx)
                            },
                        )
                        .length(SLIDER_PX),
                    ),
                    components::menu_item(
                        theme,
                        "laser-reset",
                        &design::icons::ARROW_COUNTER_CLOCKWISE,
                        i18n::t(cx, "laser.reset"),
                    )
                    .on_click(|_, _, cx| {
                        for s in [
                            Setting::PointerSmoothing(kernel::overlay::POINTER_SMOOTHING),
                            Setting::PointerRelease(kernel::overlay::POINTER_RELEASE_DEG),
                            Setting::LaserLength(kernel::overlay::LASER_LENGTH),
                        ] {
                            push_setting(s, cx);
                        }
                    })
                    .into_any_element(),
                ],
            ),
            group(theme, None, vec![hand_row.into_any_element()]),
        ]
    }

    /// 手の調整の画面の行（値と範囲から、題・スライダー・書き換えを 1 行にする）。
    fn hand(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<Div> {
        let now = settings(cx).hand;
        let item = |id: &'static str,
                    title: SharedString,
                    v: f32,
                    (lo, hi): (f32, f32),
                    set: fn(&mut HandTuning, f32)| {
            row(
                theme,
                id,
                &design::icons::HAND_POINTING,
                title,
                None,
                components::slider(theme, id, (v - lo) / (hi - lo), move |t, _, cx| {
                    let mut h = settings(cx).hand;
                    set(&mut h, lo + t * (hi - lo));
                    push_setting(Setting::Hand(h), cx)
                })
                .length(SLIDER_PX),
            )
        };
        let mm = |key: &str, v: f32| i18n::tf(cx, key, &[("mm", &format!("{v:.0}"))]);
        let deg = |key: &str, v: f32| i18n::tf(cx, key, &[("deg", &format!("{v:.0}"))]);
        let pct = |key: &str, v: f32| i18n::tf(cx, key, &[("pct", &format!("{:.0}", v * 100.0))]);
        vec![
            group(
                theme,
                None,
                vec![
                    item(
                        "tune-pinch",
                        mm("skeleton.tune_pinch", now.pinch_mm),
                        now.pinch_mm,
                        HandTuning::PINCH_MM,
                        |h, v| h.pinch_mm = v,
                    ),
                    item(
                        "tune-little",
                        mm("skeleton.tune_little", now.little_mm),
                        now.little_mm,
                        HandTuning::PINCH_MM,
                        |h, v| h.little_mm = v,
                    ),
                    item(
                        "tune-grip",
                        deg("skeleton.tune_grip", now.grip_deg),
                        now.grip_deg,
                        HandTuning::GRIP_DEG,
                        |h, v| h.grip_deg = v,
                    ),
                    item(
                        "tune-palm",
                        deg("skeleton.tune_palm", now.palm_deg),
                        now.palm_deg,
                        HandTuning::PALM_DEG,
                        |h, v| h.palm_deg = v,
                    ),
                    item(
                        "tune-smoothing",
                        pct("skeleton.tune_smoothing", now.smoothing),
                        now.smoothing,
                        HandTuning::UNIT,
                        |h, v| h.smoothing = v,
                    ),
                    item(
                        "tune-hold",
                        deg("skeleton.tune_hold", now.click_hold_deg),
                        now.click_hold_deg,
                        HandTuning::CLICK_HOLD_DEG,
                        |h, v| h.click_hold_deg = v,
                    ),
                    item(
                        "tune-click",
                        pct("skeleton.tune_click", now.click_steady),
                        now.click_steady,
                        HandTuning::UNIT,
                        |h, v| h.click_steady = v,
                    ),
                ],
            ),
            group(
                theme,
                None,
                vec![
                    components::menu_item(
                        theme,
                        "tune-reset",
                        &design::icons::ARROW_COUNTER_CLOCKWISE,
                        i18n::t(cx, "laser.reset"),
                    )
                    .on_click(change(Setting::Hand(HandTuning::default())))
                    .into_any_element(),
                ],
            ),
        ]
    }

    /// バージョン情報（ciel と同じ並び: ロゴ・名前・版の札、その下に情報の行のカード）。
    fn about(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<Div> {
        let version = env!("CARGO_PKG_VERSION");
        let card = components::card(theme)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(spacing::SM))
            .py(px(spacing::XL))
            .child(
                div()
                    .size(px(LOGO_PX))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::LG))
                    .bg(rgba(theme.control))
                    .child(design::icon(&design::icons::PAW_PRINT).size(px(LOGO_PX / 2.0))),
            )
            .child(
                div()
                    .debug_selector(|| "about-name".into())
                    .text_size(px(font_size::LG))
                    .font_weight(FontWeight::BOLD)
                    .child("nyaw"),
            )
            .child(
                div()
                    .debug_selector(|| "about-version".into())
                    .text_color(rgba(theme.neutral_text_low))
                    .child(version),
            );
        vec![
            card,
            group(
                theme,
                None,
                vec![
                    info_row(
                        theme,
                        "about-row-version",
                        i18n::t(cx, "settings.version"),
                        version.into(),
                    ),
                    info_row(
                        theme,
                        "about-row-license",
                        i18n::t(cx, "settings.license"),
                        env!("CARGO_PKG_LICENSE").into(),
                    ),
                    info_row(
                        theme,
                        "about-row-platform",
                        i18n::t(cx, "settings.platform"),
                        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                    ),
                ],
            ),
        ]
    }
}

fn mode_button(
    theme: &Theme,
    id: &'static str,
    label: SharedString,
    on: bool,
    mode: PointerMode,
) -> components::Button {
    components::button(theme, id, label, Variant::Default)
        .selected(on)
        .on_click(change(Setting::PointerMode(mode)))
}

/// 言語の選択肢（言語の名前はその言語自身で出す）。
fn design_langs(
    theme: &Theme,
    current: i18n::Lang,
    _cx: &mut Context<Settings>,
) -> Vec<components::Button> {
    i18n::Lang::ALL
        .into_iter()
        .map(|l| {
            let id = match l {
                i18n::Lang::En => "lang-en",
                i18n::Lang::Ja => "lang-ja",
            };
            components::button(theme, id, l.native_name(), Variant::Default)
                .selected(l == current)
                .on_click(change(Setting::Language(language(l))))
        })
        .collect()
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let categories = CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, &(id, glyph, key))| {
                components::sidebar_item(&theme, id, glyph, i18n::t(cx, key), i == self.category)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.category = i;
                        this.pages = vec![SettingsPage::Category];
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let (this, category) = (cx.entity(), self.category);
        let pages: fn(&mut Settings) -> &mut Vec<SettingsPage> = |v| &mut v.pages;
        let page = move |p: &SettingsPage, _: &mut Window, cx: &mut App| {
            let theme = Theme::current(cx);
            let (head, body) = this.update(cx, |s, cx| match p {
                SettingsPage::Category => {
                    let (_, _, title) = CATEGORIES[category];
                    let body = match category {
                        GENERAL => s.general(&theme, cx),
                        APPEARANCE => s.appearance(&theme, cx),
                        CONTROLS => s.controls(&theme, cx),
                        ABOUT => s.about(&theme, cx),
                        _ => unreachable!("categories are listed in CATEGORIES"),
                    };
                    (heading(i18n::t(cx, title), None), body)
                }
                SettingsPage::Hand => (
                    heading(
                        i18n::t(cx, "settings.hand"),
                        Some(
                            components::icon_button(
                                &theme,
                                "settings-back",
                                &design::icons::CARET_LEFT,
                                i18n::t(cx, "common.back"),
                                Variant::OnBackground,
                            )
                            .on_click(stack_op(&cx.entity(), pages, None))
                            .into_any_element(),
                        ),
                    ),
                    s.hand(&theme, cx),
                ),
            });
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                .child(head)
                .child(div().flex().flex_col().gap(px(spacing::LG)).children(body))
                .into_any_element()
        };
        let back = cx.entity();
        components::panel(&theme)
            .flex()
            .child(
                div()
                    .w(px(SIDEBAR_PX))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::MD))
                    .child(heading(i18n::t(cx, "settings.title"), None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(spacing::XS))
                            .children(categories),
                    ),
            )
            // サイドバーと中身の区切り。
            .child(
                div()
                    .debug_selector(|| "settings-separator".into())
                    .w(px(1.0))
                    .mx(px(spacing::MD))
                    .bg(rgba(theme.neutral_border_subtle)),
            )
            .child(div().flex_1().child(components::navigation(
                &theme,
                "settings-content",
                &self.pages,
                page,
                move |_, cx| {
                    back.update(cx, |v, cx| {
                        v.pages.pop();
                        cx.notify()
                    })
                },
            )))
    }
}
