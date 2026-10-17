//! Layer 5 画面: Testing パネル。UI 層（`ui`）の上に載る画面の1つで、一連の機能を1画面で試せるテスト用の画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック数・HMD / 手 / トラッカーへの取り付け・取り外し・ロック・メニューボタンと掴みの表示
//! - GPUI: 切り替え・押している間の見た目・ドラッグ＆ドロップ・スクロールする一覧・フォーカスと文字入力・ポップアップメニュー
//! - ポインタの方式: [`PointerTest`] でマウス・タッチのエミュレートを切り替える
//! - レーザー: [`SmoothingTest`] でレーザーの手ブレ抑えの強さと線の長さを変え、小さな的で揺れと遅れを見る
//! - ツールチップ: [`TooltipTest`] で、カーソルに付いて出る説明（向き・猶予・文字の差し替え・幅の伸び縮み）を見る
//! - 積み重ね: [`NavTest`] で、右から入る画面（push 遷移）と、押したまま右から左へ動かして戻るスワイプバック（設定画面の中身と同じ部品）
//! - 親子: [`DialogTest`] がダイアログ（[`Dialog`]）を子として開く。開いている間の親は暗くなり、子を掴むと親も付いてくる
//! - 描画: [`TestingAnim`]（毎フレーム変わる）・[`TestingLod`]（今の LOD を大きく出す）・[`TestingFont`]（フォントの見本）・[`TestingScrollbar`]（縦・横・両方のスクロールバー）
//! - 音: [`AudioTest`] で、パネルのスピーカーから立体的に鳴る音（同時再生・立体化のオプトアウト・モノラル / ステレオ）を聞く
//! - 負荷: [`StressTest`] で上限いっぱいまでパネルを出し、閉じる・ちりばめる・並べる
//!
//! 要素はすべて `debug_selector` で名前を付けてある（名前 = `id`）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

use design::components::{self, Variant};
use design::{Mode, Theme, font_size, radius, spacing};
use gpui::{
    App, ClickEvent, Context, Div, FocusHandle, FontWeight, KeyDownEvent, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, SharedString, Stateful, WeakEntity, Window, anchored, deferred,
    div, hsla, prelude::*, px, relative, rgb, rgba,
};
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{Language, PanelCommand, PanelId, PointerMode};
use ui::{PanelControl, PanelSpec};

mod emoji;
pub use emoji::EmojiTest;
mod keyboard;
pub use keyboard::KeyboardTest;
mod desktop;
pub use desktop::DesktopTest;
mod audio;
pub use audio::AudioTest;

/// 「HMD に取り付け」で使う相対姿勢: 顔の正面 1m。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」で使う相対姿勢: 0.3m 前（手首の時計のような配置）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// 色は `design` のダークテーマの役割から引く（`color_at` は 0xRRGGBB なのでアルファを落とす）。
const THEME: Theme = Theme::of(Mode::Dark);
const fn rgb24(rgba: u32) -> u32 {
    rgba >> 8
}
/// カード 1 枚のパネルで使う地の色。
pub const PANEL_BG: u32 = rgb24(THEME.card);
/// Testing パネルのカードの外側にある地の色。
pub const PANEL_GROUND_BG: u32 = rgb24(THEME.background);
/// パネルの縁の影の余白。地はこの内側から始まる。
pub use design::components::PANEL_MARGIN;
pub const BUTTON_BG: u32 = rgb24(THEME.control);
/// レーザーが当たっている間のボタン色。VR 上でどこを指しているかの視覚的フィードバック。
pub const BUTTON_HOVER_BG: u32 = rgb24(THEME.control_hover);
/// 有効になっている切り替えボタン・表示や、押している間のボタンの色。
pub const BUTTON_ACTIVE_BG: u32 = rgb24(THEME.control_active);
/// ドラッグ中の値を落とせる場所の上にいるときの色。
pub const DROP_TARGET_BG: u32 = 0x30a050;
/// フォーカスのある入力欄の色。
pub const FIELD_FOCUS_BG: u32 = rgb24(THEME.accent_bg_active);
/// ダイアログの背景色（親より明るくして重なりが見えるように）。
pub const DIALOG_BG: u32 = rgb24(THEME.card);
/// 画面の地（`components::panel`）。設定画面のサイドバーの、選んでいない行も同じ色（背景が無いように見える）。
pub const BACKGROUND: u32 = rgb24(THEME.background);
/// 設定画面のサイドバーで選んでいる行の色。
pub const SIDEBAR_SELECTED_BG: u32 = rgb24(THEME.accent_bg_active);
/// Testing パネルの大きさ（m）と解像度（px）。ボタンが高い（最小ヒット領域）ので、既定の 0.8m × 0.6m では入りきらない。
pub const TESTING_SIZE: Vec2 = Vec2::new(1.2, 1.0);
pub const TESTING_PX: (u32, u32) = (1200, 1000);
/// スクロールする一覧の行数と1行の高さ。
pub const LIST_ROWS: usize = 30;
const ROW_PX: f32 = 24.0;
/// ピクセル単位のスクロール（トラックパッド等）を行に換算するときの1行の高さ。
const SCROLL_LINE_PX: f32 = 20.0;

#[derive(Default)]
pub struct Testing {
    panel: PanelId,
    pub clicks: usize,
    /// 受け取ったスクロール量の合計（行。下向きが負）。
    pub scrolled: f32,
    pub toggled: bool,
    /// ドロップ先に落とされた回数。
    pub dropped: usize,
    /// 入力欄に打たれた文字。
    pub text: String,
    pub menu_open: bool,
    /// ポップアップメニューで選んだ項目。
    pub picked: Option<&'static str>,
    /// スクロールする一覧の状態（`offset().y` が負 = 下へスクロール済み）。
    pub list: ScrollHandle,
    focus: Option<FocusHandle>,
}

impl Testing {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }
}

/// ドラッグ＆ドロップで運ぶ値。
struct Chip;

/// ドラッグ中にポインタに付いてくる見た目。
struct ChipPreview;

impl Render for ChipPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_4().bg(rgb(BUTTON_ACTIVE_BG))
    }
}

/// 押せるボタン（`design` の部品）。`active` は切り替えボタンが有効なとき。
fn button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> components::Button {
    themed_button(theme, id, label, active)
}

fn themed_button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> components::Button {
    components::button(theme, id, label.into(), Variant::Default).selected(active)
}

/// ボタンを押したらパネルへの操作要求を積む。
fn command(
    panel: PanelId,
    c: PanelCommand,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .commands
            .push(c.clone())
    }
}

fn spawn(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| Testing::new(id))
                .sized(TESTING_SIZE, TESTING_PX)
                .titled("testing.open.testing"),
        )
    }
}

fn spawn_dialog_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| DialogTest::new(id)).titled("testing.open.dialog"),
        )
    }
}

fn spawn_pointer_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| PointerTest::new(id)).titled("testing.open.pointer"),
        )
    }
}

fn spawn_transparent(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| TestingTransparent::new(id))
                .titled("testing.open.transparent"),
        )
    }
}

/// 隣に `build` の画面を開く。`title` はナビゲーションバーのタイトル（i18n のキー）。
fn spawn_view<V: Render>(
    panel: PanelId,
    title: &'static str,
    build: fn(PanelId, &mut App) -> V,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, move |id, _, cx| build(id, cx)).titled(title))
    }
}

/// design のカタログを隣に開く（Close はこのパネルを閉じる）。
fn spawn_catalog(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, cx| {
                // 動きの見本を毎フレーム描く。
                cx.default_global::<PanelControl>()
                    .panels
                    .entry(id)
                    .or_default()
                    .live = true;
                design::catalog::Catalog::default()
            })
            .titled("testing.open.catalog"),
        )
    }
}

/// カーネルの言語に対応する i18n の言語。
fn lang(l: Language) -> i18n::Lang {
    match l {
        Language::En => i18n::Lang::En,
        Language::Ja => i18n::Lang::Ja,
    }
}

/// 言語の切り替えボタンが次に選ぶ言語（i18n の対応言語を順に回る）。
fn next_language(l: Language) -> Language {
    language(lang(l).next())
}

fn language(l: i18n::Lang) -> Language {
    match l {
        i18n::Lang::En => Language::En,
        i18n::Lang::Ja => Language::Ja,
    }
}

fn attach_offset(panel: PanelId, base: Pose) -> Pose {
    Pose::new(
        base.position + Vec3::new(1.2 * panel as f32, 0.0, 0.0),
        base.orientation,
    )
}

fn row() -> Div {
    div().flex().flex_wrap().gap_2().items_center()
}

/// 見出し付きのカード。中身は縦に並べる。
fn section(theme: &Theme, title: impl Into<SharedString>) -> Div {
    components::card(theme)
        .p(px(spacing::LG))
        .flex()
        .flex_col()
        .gap(px(spacing::LG))
        .child(
            div()
                .text_size(px(font_size::MD))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        )
}

/// 小見出しと、その下の中身。
fn labeled(theme: &Theme, label: impl Into<SharedString>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::XS))
        .child(
            div()
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(label.into()),
        )
        .child(body)
}

/// 押せない状態の表示。ボタンと見分けられるよう低く、先頭に点を置く（点は入っている間だけ強調色）。
/// 面の色は入っている間 `control-active`、それ以外 `control`（テストは中心の色を見る）。
fn indicator(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(36.0))
        .px(px(spacing::MD))
        .flex()
        .items_center()
        .gap(px(spacing::SM))
        .rounded(px(radius::FULL))
        .bg(rgba(if on {
            theme.control_active
        } else {
            theme.control
        }))
        .child(
            div()
                .size(px(10.0))
                .rounded(px(radius::FULL))
                .bg(rgba(if on {
                    theme.accent_solid
                } else {
                    theme.neutral_text_low
                })),
        )
        .child(label.into())
}

impl Render for Testing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.clone())
            .unwrap_or_default();
        let on = |d: &Device| st.attached.as_ref() == Some(d);
        let profiling = cx.try_global::<PanelControl>().is_some_and(|c| c.profiling);
        let tracker = st.trackers.first().map(|id| Device::Tracker(id.clone()));
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let theme = Theme::current(cx);
        // この画面のボタンは現在のテーマで描く（上のモジュール関数 `button` を隠す）。
        macro_rules! button {
            ($($a:expr),+ $(,)?) => {
                themed_button(&theme, $($a),+)
            };
        }

        let theme_label = i18n::tf(
            cx,
            "testing.panel.theme",
            &[(
                "theme",
                &i18n::t(
                    cx,
                    match st.theme {
                        kernel::ThemeMode::Dark => "theme.dark",
                        kernel::ThemeMode::Light => "theme.light",
                    },
                ),
            )],
        );
        let language_label = i18n::tf(
            cx,
            "testing.panel.language",
            &[("language", &lang(st.language).native_name())],
        );
        // このパネル自身の操作（表示・テーマ・言語・ロック・計測）。
        let panel_row = row()
            .child(
                button!(
                    "animate-panel",
                    i18n::t(cx, "testing.panel.animation"),
                    st.animated
                )
                .on_click(command(self.panel, PanelCommand::Animated(!st.animated))),
            )
            .child(
                button!("toggle-theme", theme_label, false)
                    .on_click(command(self.panel, PanelCommand::Theme(st.theme.toggled()))),
            )
            .child(
                // 言語の名前はその言語自身で出す（どの言語で表示していても読める）。
                button!("toggle-language", language_label, false).on_click(command(
                    self.panel,
                    PanelCommand::Language(next_language(st.language)),
                )),
            )
            .child(
                button!("lock", i18n::t(cx, "testing.panel.lock"), st.locked)
                    .on_click(command(self.panel, PanelCommand::Lock(!st.locked))),
            )
            .child(
                button!(
                    "toggle-profiler",
                    i18n::t(cx, "testing.panel.profiler"),
                    profiling
                )
                .on_click(|_, _, cx| {
                    let c = cx.default_global::<PanelControl>();
                    c.profiling = !c.profiling;
                }),
            );
        // 取り付け先（今どこに付いているかで光る）。
        let attach_row = row()
            .child(
                button!(
                    "attach-hmd",
                    i18n::t(cx, "testing.attach.hmd"),
                    on(&Device::Hmd)
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(Device::Hmd, attach_offset(self.panel, HMD_OFFSET)),
                )),
            )
            .child(
                button!(
                    "attach-left",
                    i18n::t(cx, "testing.attach.left_hand"),
                    on(&Device::Controller(Hand::Left)),
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(
                        Device::Controller(Hand::Left),
                        attach_offset(self.panel, HAND_OFFSET),
                    ),
                )),
            )
            .child(
                button!(
                    "attach-right",
                    i18n::t(cx, "testing.attach.right_hand"),
                    on(&Device::Controller(Hand::Right)),
                )
                .on_click(command(
                    self.panel,
                    PanelCommand::Attach(
                        Device::Controller(Hand::Right),
                        attach_offset(self.panel, HAND_OFFSET),
                    ),
                )),
            )
            .child({
                // 見えている最初のトラッカーに取り付ける。見えていなければ何もしない。
                let b = button!(
                    "attach-tracker",
                    match &tracker {
                        Some(Device::Tracker(id)) => {
                            i18n::tf(cx, "testing.attach.tracker", &[("id", id)])
                        }
                        _ => i18n::t(cx, "testing.attach.tracker_none"),
                    },
                    tracker.as_ref().is_some_and(on),
                );
                match tracker {
                    Some(d) => b.on_click(command(
                        self.panel,
                        PanelCommand::Attach(d, attach_offset(self.panel, HAND_OFFSET)),
                    )),
                    None => b,
                }
            })
            .child(
                button!("detach", i18n::t(cx, "testing.attach.detach"), false)
                    .on_click(command(self.panel, PanelCommand::Detach)),
            );
        // コントローラの入力の表示（押せない）。見出しの行に置く。
        let input_row = row()
            .child(indicator(
                &theme,
                "menu-held",
                i18n::t(cx, "testing.menu_button"),
                st.menu,
            ))
            .child(indicator(
                &theme,
                "grabbed",
                i18n::t(cx, "testing.grabbing"),
                st.grabbed,
            ));

        // 隣に開く画面。
        let open_row =
            row()
                .child(
                    button!("spawn-panel", i18n::t(cx, "testing.open.testing"), false)
                        .on_click(spawn(self.panel)),
                )
                .child(
                    button!(
                        "spawn-transparent",
                        i18n::t(cx, "testing.open.transparent"),
                        false
                    )
                    .on_click(spawn_transparent(self.panel)),
                )
                .child(
                    button!("spawn-anim", i18n::t(cx, "testing.open.moving"), false).on_click(
                        spawn_view(self.panel, "testing.open.moving", TestingAnim::new),
                    ),
                )
                .child(
                    button!("spawn-lod", i18n::t(cx, "testing.open.lod"), false).on_click(
                        spawn_view(self.panel, "testing.open.lod", |id, _| TestingLod::new(id)),
                    ),
                )
                .child(
                    button!("spawn-font", i18n::t(cx, "testing.open.font"), false).on_click(
                        spawn_view(self.panel, "testing.open.font", |id, _| {
                            TestingFont::new(id)
                        }),
                    ),
                )
                .child(
                    button!("emoji-test", i18n::t(cx, "testing.open.emoji"), false).on_click(
                        spawn_view(self.panel, "testing.open.emoji", |id, _| EmojiTest::new(id)),
                    ),
                )
                .child(
                    button!("keyboard-test", i18n::t(cx, "keyboard.title"), false).on_click(
                        move |_, _, cx| {
                            cx.default_global::<PanelControl>()
                                .spawns
                                .push(KeyboardTest::spec());
                        },
                    ),
                )
                .child(
                    button!(
                        "spawn-scrollbar",
                        i18n::t(cx, "testing.open.scrollbar"),
                        false
                    )
                    .on_click(spawn_view(
                        self.panel,
                        "testing.open.scrollbar",
                        |id, _| TestingScrollbar::new(id),
                    )),
                )
                .child(
                    button!("pointer-test", i18n::t(cx, "testing.open.pointer"), false)
                        .on_click(spawn_pointer_test(self.panel)),
                )
                .child(
                    button!("desktop-test", i18n::t(cx, "desktop.title"), false).on_click(
                        spawn_view(self.panel, "desktop.title", |id, _| DesktopTest::new(id)),
                    ),
                )
                .child(
                    button!("audio-test", i18n::t(cx, "audio.title"), false).on_click(
                        spawn_view(self.panel, "audio.title", |id, _| AudioTest::new(id)),
                    ),
                )
                .child(
                    button!("smoothing-test", i18n::t(cx, "testing.open.laser"), false).on_click(
                        spawn_view(self.panel, "testing.open.laser", |id, _| {
                            SmoothingTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("tooltip-test", i18n::t(cx, "testing.open.tooltip"), false).on_click(
                        spawn_view(self.panel, "testing.open.tooltip", |id, _| {
                            TooltipTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("dialog-test", i18n::t(cx, "testing.open.dialog"), false)
                        .on_click(spawn_dialog_test(self.panel)),
                )
                .child(
                    button!("settings-test", i18n::t(cx, "testing.open.settings"), false).on_click(
                        {
                            let panel = self.panel;
                            move |_, _, cx| {
                                cx.default_global::<PanelControl>().spawns.push(
                                    PanelSpec::spawn(panel, |id, _, _| Settings::new(id))
                                        .sized(SETTINGS_SIZE, SETTINGS_PX)
                                        .titled("testing.open.settings"),
                                )
                            }
                        },
                    ),
                )
                .child(
                    button!("nav-test", i18n::t(cx, "testing.open.navigation"), false).on_click(
                        spawn_view(self.panel, "testing.open.navigation", |id, _| {
                            NavTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("stress-test", i18n::t(cx, "testing.open.stress"), false).on_click(
                        spawn_view(self.panel, "testing.open.stress", |id, _| {
                            StressTest::new(id)
                        }),
                    ),
                )
                .child(
                    button!("spawn-catalog", i18n::t(cx, "testing.open.catalog"), false)
                        .on_click(spawn_catalog(self.panel)),
                )
                .child(
                    button!(
                        "locale-test",
                        i18n::t(cx, "testing.open.localization"),
                        false
                    )
                    .on_click({
                        let panel = self.panel;
                        move |_, _, cx| {
                            // 全キーの表を並べるので Testing と同じ大きさ。
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::spawn(panel, |id, _, _| LocaleTest::new(id))
                                    .sized(TESTING_SIZE, TESTING_PX)
                                    .titled("locale.title"),
                            )
                        }
                    }),
                );

        // ボタン・切り替え・長押し・ポップアップ。
        let press_row = row()
            .child(
                button!(
                    "click-me",
                    i18n::tf(cx, "testing.clicked", &[("count", &self.clicks)]),
                    false
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clicks += 1;
                    cx.notify();
                })),
            )
            .child(
                button!("toggle", i18n::t(cx, "testing.toggle"), self.toggled).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.toggled = !this.toggled;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button!("hold-me", i18n::t(cx, "testing.hold"), false)
                    .active(|s| s.bg(rgba(theme.control_active))),
            )
            .child(
                div()
                    .child(
                        button!(
                            "open-menu",
                            i18n::tf(cx, "testing.menu", &[("item", &self.picked.unwrap_or("-"))]),
                            self.menu_open,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu_open = !this.menu_open;
                            cx.notify();
                        })),
                    )
                    .when(self.menu_open, |d| {
                        d.child(deferred(
                            anchored().child(
                                div()
                                    .mt(px(spacing::XS))
                                    .p(px(spacing::XS))
                                    .flex()
                                    .flex_col()
                                    .gap(px(spacing::XS))
                                    .rounded(px(radius::LG))
                                    .bg(rgba(theme.card))
                                    .shadow_md()
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.menu_open = false;
                                        cx.notify();
                                    }))
                                    .children(["menu-a", "menu-b"].map(|id| {
                                        button!(id, id, self.picked == Some(id)).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.picked = Some(id);
                                                this.menu_open = false;
                                                cx.notify();
                                            }),
                                        )
                                    })),
                            ),
                        ))
                    }),
            );
        // ドラッグ＆ドロップ: 左を右へ運ぶ。
        let drag_row = row()
            .child(
                button!("drag-chip", i18n::t(cx, "testing.drag_me"), false)
                    .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
            )
            .child(
                div()
                    .id("drop-zone")
                    .debug_selector(|| "drop-zone".into())
                    .flex_1()
                    .h(px(54.0))
                    .px(px(spacing::LG))
                    .flex()
                    .items_center()
                    .rounded(px(radius::LG))
                    .border_2()
                    .border_dashed()
                    .border_color(rgba(theme.neutral_text_low))
                    .bg(rgba(theme.control))
                    .drag_over::<Chip>(|s, _, _, _| s.bg(rgb(DROP_TARGET_BG)))
                    .on_drop(cx.listener(|this, _: &Chip, _, cx| {
                        this.dropped += 1;
                        cx.notify();
                    }))
                    .child(i18n::tf(
                        cx,
                        "testing.drop_here",
                        &[("count", &self.dropped)],
                    )),
            );

        let field = div()
            .id("text-field")
            .debug_selector(|| "text-field".into())
            .track_focus(&focus)
            .h(px(54.0))
            .px(px(spacing::LG))
            .flex()
            .items_center()
            .rounded(px(radius::LG))
            .bg(rgba(theme.control))
            .focus(|s| s.bg(rgba(theme.accent_bg_active)))
            .on_click(cx.listener(move |this, _, window, cx| {
                ui::keyboard::focus_input(this.panel, &focus, window, cx);
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                match (k.key.as_str(), &k.key_char) {
                    ("backspace", _) => {
                        this.text.pop();
                    }
                    (_, Some(c)) => this.text.push_str(c),
                    ("space", None) => this.text.push(' '),
                    (key, None) if key.chars().count() == 1 => this.text.push_str(key),
                    _ => return,
                }
                cx.notify();
            }))
            .child(if self.text.is_empty() {
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "testing.type_hint"))
            } else {
                div().child(self.text.clone())
            });

        let scrolled = format!("{:.1}", self.scrolled);
        let rows: Vec<SharedString> = (0..LIST_ROWS)
            .map(|i| i18n::tf(cx, "testing.row", &[("index", &i), ("scrolled", &scrolled)]))
            .collect();
        let list =
            div()
                .id("scroll-list")
                .debug_selector(|| "scroll-list".into())
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.list)
                .rounded(px(radius::LG))
                .bg(rgba(theme.control))
                .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                    this.scrolled += match e.delta {
                        ScrollDelta::Lines(p) => p.y,
                        ScrollDelta::Pixels(p) => f32::from(p.y) / SCROLL_LINE_PX,
                    };
                    cx.notify();
                }))
                .child(components::elastic("pull-list", &self.list).children(
                    rows.into_iter().map(|row| {
                        div()
                            .h(gpui::px(ROW_PX))
                            .px(px(spacing::LG))
                            .flex()
                            .items_center()
                            .child(row)
                    }),
                ));

        // 見出し: 何の画面か、このパネルの今の状態、閉じる。
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(spacing::SM))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(i18n::t(cx, "testing.title")),
                    )
                    .child(
                        div()
                            .debug_selector(|| "panel-status".into())
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::tf(
                                cx,
                                "testing.status",
                                &[
                                    ("runtime", &format!("{:?}", st.runtime)),
                                    (
                                        "attached",
                                        &st.attached
                                            .as_ref()
                                            .map_or(i18n::t(cx, "common.none"), |d| {
                                                format!("{d:?}").into()
                                            }),
                                    ),
                                    (
                                        "lock",
                                        &i18n::t(
                                            cx,
                                            if st.locked {
                                                "testing.locked"
                                            } else {
                                                "testing.unlocked"
                                            },
                                        ),
                                    ),
                                ],
                            )),
                    )
                    .child(input_row),
            );

        // カードが複数なので地を敷く。左 = このパネルと開く画面、右 = 部品の操作。
        components::panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .id("testing-left")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(spacing::MD))
                            .child(
                                section(&theme, i18n::t(cx, "testing.section.this_panel"))
                                    .child(labeled(
                                        &theme,
                                        i18n::t(cx, "testing.label.view"),
                                        panel_row,
                                    ))
                                    .child(labeled(
                                        &theme,
                                        i18n::t(cx, "testing.label.attach_to"),
                                        attach_row,
                                    )),
                            )
                            .child(
                                section(&theme, i18n::t(cx, "testing.section.open"))
                                    .child(open_row),
                            ),
                    )
                    .child(
                        section(&theme, i18n::t(cx, "testing.section.interact"))
                            .flex_1()
                            .min_w_0()
                            .child(labeled(
                                &theme,
                                i18n::t(cx, "testing.label.press"),
                                press_row,
                            ))
                            .child(labeled(&theme, i18n::t(cx, "testing.label.drag"), drag_row))
                            .child(labeled(&theme, i18n::t(cx, "testing.label.type"), field))
                            .child(
                                labeled(
                                    &theme,
                                    i18n::t(cx, "testing.label.scroll"),
                                    div().relative().flex_1().min_h_0().child(list).child(
                                        components::scrollbars(&theme, "scroll-bars", &self.list)
                                            .pulled_by("pull-list"),
                                    ),
                                )
                                .flex_1()
                                .min_h_0(),
                            ),
                    ),
            )
    }
}

/// ポインタの方式（マウス / タッチのエミュレート）を試す画面（`pointer-test`）。
/// `pointer-mode` で方式を切り替え（タッチのとき光る）、同じ操作がどう効くかを見る:
/// マウスでは `drag-chip` を `drop-zone` へ運べ、タッチでは `swipe-list` を押したまま動かすとスクロールする。
/// `tap-me` はどちらでも押せる。
#[derive(Default)]
pub struct PointerTest {
    panel: PanelId,
    pub taps: usize,
    pub dropped: usize,
    /// スワイプする一覧の状態（`offset().y` が負 = 下へスクロール済み）。
    pub list: ScrollHandle,
}

impl PointerTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }
}

impl Render for PointerTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let mode = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.pointer_mode)
            .unwrap_or_default();
        let (label, next) = match mode {
            PointerMode::Mouse => (i18n::t(cx, "pointer.mode_mouse"), PointerMode::Touch),
            PointerMode::Touch => (i18n::t(cx, "pointer.mode_touch"), PointerMode::Mouse),
        };
        let swipe_rows: Vec<SharedString> = (0..LIST_ROWS)
            .map(|i| i18n::tf(cx, "pointer.swipe_row", &[("index", &i)]))
            .collect();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                row()
                    .child(
                        button(&theme, "pointer-mode", label, mode == PointerMode::Touch)
                            .on_click(command(self.panel, PanelCommand::PointerMode(next))),
                    )
                    .child(
                        button(
                            &theme,
                            "tap-me",
                            i18n::tf(cx, "pointer.tapped", &[("count", &self.taps)]),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.taps += 1;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                row()
                    .child(
                        button(&theme, "drag-chip", i18n::t(cx, "testing.drag_me"), false)
                            .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
                    )
                    .child(
                        div()
                            .id("drop-zone")
                            .debug_selector(|| "drop-zone".into())
                            .p_2()
                            .bg(rgba(theme.control))
                            .drag_over::<Chip>(|s, _, _, _| s.bg(rgb(DROP_TARGET_BG)))
                            .on_drop(cx.listener(|this, _: &Chip, _, cx| {
                                this.dropped += 1;
                                cx.notify();
                            }))
                            .child(i18n::tf(cx, "pointer.dropped", &[("count", &self.dropped)])),
                    ),
            )
            .child(
                div()
                    .id("swipe-list")
                    .debug_selector(|| "swipe-list".into())
                    .h(gpui::px(240.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.list)
                    .bg(rgba(theme.control))
                    .children(
                        swipe_rows
                            .into_iter()
                            .map(|row| div().h(gpui::px(ROW_PX)).px_2().child(row)),
                    ),
            )
    }
}

/// レーザーを調整する画面（`smoothing-test`）。`smoothing-slider` で手ブレ抑えの強さを、`laser-length-slider` で
/// 線の見える長さを変えるとその場で効き、設定ファイルの `pointer_smoothing` / `laser_length` にも残る
/// （`PanelCommand::PointerSmoothing` / `LaserLength`）。下の小さな的を狙って、止めたときの揺れと、振ったときの遅れを見比べる。
pub struct SmoothingTest {
    panel: PanelId,
}

impl SmoothingTest {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
    }
}

/// 狙う的の数と大きさ（px）。小さいほど揺れが分かる。
const AIM_TARGETS: usize = 8;
const AIM_PX: f32 = 32.0;

impl Render for SmoothingTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        let max = kernel::overlay::POINTER_SMOOTHING_MAX;
        let now = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&panel))
            .map_or(kernel::overlay::POINTER_SMOOTHING, |c| {
                c.status.pointer_smoothing
            });
        let length = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&panel))
            .map_or(kernel::overlay::LASER_LENGTH, |c| c.status.laser_length);
        let (short, long) = (
            kernel::overlay::LASER_LENGTH_MIN,
            kernel::overlay::LASER_MAX,
        );
        let release = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&panel))
            .map_or(kernel::overlay::POINTER_RELEASE_DEG, |c| {
                c.status.pointer_release_deg
            });
        let (tight, loose) = (
            kernel::overlay::POINTER_RELEASE_DEG_MIN,
            kernel::overlay::POINTER_RELEASE_DEG_MAX,
        );
        let preset = |id: &'static str, label: SharedString, v: f32| {
            button(&theme, id, label, (now - v).abs() < 1e-3)
                .on_click(command(panel, PanelCommand::PointerSmoothing(v)))
        };
        let length_preset = |id: &'static str, label: SharedString, v: f32| {
            button(&theme, id, label, (length - v).abs() < 1e-3)
                .on_click(command(panel, PanelCommand::LaserLength(v)))
        };
        let push = move |c: PanelCommand, cx: &mut App| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .push(c)
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
                    push(PanelCommand::PointerSmoothing(v * max), cx)
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
                        push(
                            PanelCommand::PointerRelease(tight + v * (loose - tight)),
                            cx,
                        )
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
                    move |v, _, cx| push(PanelCommand::LaserLength(short + v * (long - short)), cx),
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
                            for c in [
                                PanelCommand::PointerSmoothing(kernel::overlay::POINTER_SMOOTHING),
                                PanelCommand::PointerRelease(kernel::overlay::POINTER_RELEASE_DEG),
                                PanelCommand::LaserLength(kernel::overlay::LASER_LENGTH),
                            ] {
                                push(c, cx);
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

/// ツールチップを試す画面（`tooltip-test`）。
/// - `tip-short` / `tip-long` は隣り合っていて、移ると猶予（0.125 秒）の間に文字が変わり、箱の幅が伸び縮みする。
/// - `tip-far` は離れていて、届くまでに猶予が過ぎると一度消えてから出直す。`no-tip` には説明が無い（指すと猶予の後に消える）。
/// - アイコンだけのボタン（`tip-icon-*`）には必ず説明が付く（`components::icon_button`）。
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
                    .child(button(&theme, "no-tip", i18n::t(cx, "tooltip.none"), false)),
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

/// ダイアログの幅（px）。上からアイコン・タイトル・概要・ボタン（2 個）。文の幅 344px。1px = 1mm。
const DIALOG_WIDTH: u32 = 440;

/// 文言のキーが `title` / `description` のダイアログを、文に合った高さ（`components::dialog_height`）で `spec` に与える。
fn dialog_sized(
    spec: PanelSpec,
    title: &str,
    description: &str,
    window: &Window,
    cx: &App,
) -> PanelSpec {
    let h = components::dialog_height(
        window,
        DIALOG_WIDTH,
        &i18n::t(cx, title),
        &i18n::t(cx, description),
        2,
    );
    spec.sized(
        Vec2::new(DIALOG_WIDTH as f32 / 1000.0, h as f32 / 1000.0),
        (DIALOG_WIDTH, h),
    )
}

/// 設定画面の大きさ（m）と解像度（px）。1px = 1mm。
pub const SETTINGS_SIZE: Vec2 = Vec2::new(1.024, 0.768);
pub const SETTINGS_PX: (u32, u32) = (1024, 768);
/// サイドバーの幅（px）。
const SIDEBAR_PX: f32 = 320.0;

/// 設定のカテゴリ（ダミー）: `debug_selector`・アイコン・文言のキー・項目の数。
// ponytail: 中身はダミー。本物の設定を載せるときに、項目の表と `crates/settings` への切り出しを決める。
const SETTINGS_CATEGORIES: [(&str, &design::Icon, &str, usize); 6] = [
    (
        "settings-general",
        &design::icons::GEAR,
        "settings.category.general",
        5,
    ),
    (
        "settings-account",
        &design::icons::USER_GEAR,
        "settings.category.account",
        3,
    ),
    (
        "settings-privacy",
        &design::icons::EYE_SLASH,
        "settings.category.privacy",
        4,
    ),
    (
        "settings-appearance",
        &design::icons::PALETTE,
        "settings.category.appearance",
        5,
    ),
    (
        "settings-security",
        &design::icons::SHIELD,
        "settings.category.security",
        2,
    ),
    (
        "settings-about",
        &design::icons::INFO,
        "settings.category.about",
        1,
    ),
];
/// 項目（ダミー）の `debug_selector` とアイコン。カテゴリが変わっても同じ並びを使う。
const SETTINGS_ITEMS: [(&str, &design::Icon); 5] = [
    ("setting-0", &design::icons::BELL),
    ("setting-1", &design::icons::GLOBE),
    ("setting-2", &design::icons::SPEAKER_HIGH),
    ("setting-3", &design::icons::MONITOR),
    ("setting-4", &design::icons::KEYBOARD),
];

/// 設定画面（`settings-test`）。左のサイドバーにカテゴリの一覧、区切り線を挟んで右に選んだカテゴリの項目の一覧。
/// 地は背景色（`components::panel`）。サイドバーの行（`components::sidebar_item`）は背景が無く、選んでいるカテゴリだけアクセントの色が付く。中身の一覧の行はカード色（`components::list`）。
/// 中身は積み重ね（[`components::navigation`]）: 項目を押すと右から詳しい画面が入り、`settings-back` か右から左へのスワイプで戻る。
#[derive(Default)]
pub struct Settings {
    /// 選んでいるカテゴリ（[`SETTINGS_CATEGORIES`] の添字）。
    pub category: usize,
    /// 中身に積んだ画面。先頭はカテゴリの項目の一覧（`None`）、その上に項目の詳しい画面（`Some(項目)`）。
    pub pages: Vec<Option<usize>>,
}

impl Settings {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            category: 0,
            pages: vec![None],
        }
    }
}

/// 見出しの行。サイドバーと中身で高さを揃え、一覧の上端を揃える。タイトルは左右の中央、`left` のボタン（戻る）は左端に寄せる（閉じるのはナビゲーションバー）。
/// 高さはアイコンボタンと同じなので、ボタンは上下にも左右にも余白を足さない（パネルの内側の余白だけが四方に揃う）。
fn heading(title: impl IntoElement, left: Option<gpui::AnyElement>) -> Div {
    div()
        .relative()
        .h(px(54.0))
        .flex()
        .items_center()
        .text_size(px(font_size::LG))
        .font_weight(FontWeight::BOLD)
        // ボタンの有無で中央がずれないよう、タイトルは重ねて置く。
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(title),
        )
        .child(div().flex().children(left))
}

/// 押すと `view` の積み重ねに `page` を積む・下ろす（`None`）。
fn stack_op<V: 'static, P: 'static + Copy>(
    view: &gpui::Entity<V>,
    stack: fn(&mut V) -> &mut Vec<P>,
    page: Option<P>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| {
        view.update(cx, |v, cx| {
            match page {
                Some(p) => stack(v).push(p),
                None => drop(stack(v).pop()),
            }
            cx.notify()
        })
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let categories = SETTINGS_CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, &(id, glyph, key, _))| {
                components::sidebar_item(&theme, id, glyph, i18n::t(cx, key), i == self.category)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.category = i;
                        this.pages = vec![None];
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let (this, category) = (cx.entity(), self.category);
        let pages: fn(&mut Settings) -> &mut Vec<Option<usize>> = |v| &mut v.pages;
        // 中身の画面 1 枚。
        let page = move |p: &Option<usize>, _: &mut Window, cx: &mut App| {
            let theme = Theme::current(cx);
            let (_, _, title, count) = SETTINGS_CATEGORIES[category];
            let (head, rows) = match *p {
                // カテゴリの項目の一覧。押すと詳しい画面を積む。
                None => (
                    heading(i18n::t(cx, title), None),
                    SETTINGS_ITEMS[..count]
                        .iter()
                        .enumerate()
                        .map(|(n, &(id, glyph))| {
                            components::list_item(
                                &theme,
                                id,
                                glyph,
                                i18n::tf(cx, "settings.item", &[("n", &(n + 1))]),
                            )
                            .on_click(stack_op(
                                &this,
                                pages,
                                Some(Some(n)),
                            ))
                        })
                        .collect::<Vec<_>>(),
                ),
                // 項目の詳しい画面（ダミー）。
                Some(n) => (
                    heading(
                        i18n::tf(cx, "settings.item", &[("n", &(n + 1))]),
                        Some(
                            components::icon_button(
                                &theme,
                                "settings-back",
                                &design::icons::CARET_LEFT,
                                i18n::t(cx, "common.back"),
                                Variant::OnBackground,
                            )
                            .on_click(stack_op(&this, pages, None))
                            .into_any_element(),
                        ),
                    ),
                    SETTINGS_ITEMS[..3]
                        .iter()
                        .enumerate()
                        .map(|(m, &(_, glyph))| {
                            components::list_item(
                                &theme,
                                ["setting-detail-0", "setting-detail-1", "setting-detail-2"][m],
                                glyph,
                                i18n::tf(cx, "settings.detail", &[("n", &(m + 1))]),
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                .child(head)
                .child(components::list(&theme, rows))
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

/// 画面の積み重ね（[`components::navigation`]）を試す画面 [`NavTest`] の画面 1 枚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavPage {
    /// 一番下。項目（`nav-item-0..`）を押すと [`NavPage::Detail`] を積む。
    Root,
    /// 項目の詳しい画面。`nav-deeper` で [`NavPage::Deep`] を積む。`nav-tap`（押した回数を数える）と `nav-slider` がある。
    Detail(usize),
    /// 一番上。
    Deep(usize),
}

/// 画面の積み重ね（push 遷移とスワイプバック）を試す画面（`nav-test`）。設定画面の中身と同じ部品。
/// - 項目を押すと右から次の画面が入り、`nav-back` か、押したまま右から左へ動かす（スワイプバック）と戻る。
/// - スワイプして離した所のボタン（`nav-tap`）は押されない。スライダー（`nav-slider`）のハンドルを左へ動かしても戻らない。
/// - パネルを掴んでいる手のポインタは無視する（掴みながらトリガーで動かしても戻らず、もう一方の手ではスワイプできる）。
#[derive(Default)]
pub struct NavTest {
    pub stack: Vec<NavPage>,
    /// `nav-tap` を押した回数。
    pub taps: usize,
    /// `nav-slider` の値。
    pub value: f32,
    /// スワイプバックで戻った回数。
    pub swiped_back: usize,
}

impl NavTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            stack: vec![NavPage::Root],
            taps: 0,
            value: 0.5,
            swiped_back: 0,
        }
    }
}

impl Render for NavTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let stack: fn(&mut NavTest) -> &mut Vec<NavPage> = |v| &mut v.stack;
        let page = move |p: &NavPage, _: &mut Window, cx: &mut App| {
            let theme = Theme::current(cx);
            let body = div().size_full().flex().flex_col().gap(px(spacing::MD));
            match *p {
                NavPage::Root => body.child(components::list(
                    &theme,
                    (0..3).map(|n| {
                        components::list_item(
                            &theme,
                            ["nav-item-0", "nav-item-1", "nav-item-2"][n],
                            &design::icons::GEAR,
                            i18n::tf(cx, "nav.item", &[("n", &(n + 1))]),
                        )
                        .on_click(stack_op(
                            &this,
                            stack,
                            Some(NavPage::Detail(n)),
                        ))
                    }),
                )),
                NavPage::Detail(n) => {
                    let (tap, set) = (this.clone(), this.clone());
                    body.child(components::list(
                        &theme,
                        [
                            components::list_item(
                                &theme,
                                "nav-deeper",
                                &design::icons::INFO,
                                i18n::tf(cx, "nav.deeper", &[("n", &(n + 1))]),
                            )
                            .on_click(stack_op(
                                &this,
                                stack,
                                Some(NavPage::Deep(n)),
                            )),
                            components::list_item(
                                &theme,
                                "nav-tap",
                                &design::icons::BELL,
                                i18n::t(cx, "nav.tap"),
                            )
                            .on_click(move |_, _, cx| {
                                tap.update(cx, |v, cx| {
                                    v.taps += 1;
                                    cx.notify()
                                })
                            }),
                        ],
                    ))
                    .child(
                        components::slider(&theme, "nav-slider", this.read(cx).value, {
                            move |v, _, cx| {
                                set.update(cx, |s, cx| {
                                    s.value = v;
                                    cx.notify()
                                })
                            }
                        })
                        .length(400.0),
                    )
                }
                NavPage::Deep(n) => body.child(i18n::tf(cx, "nav.deep", &[("n", &(n + 1))])),
            }
            .into_any_element()
        };
        let depth = self.stack.len() - 1;
        let title = match self.stack[depth] {
            NavPage::Root => i18n::t(cx, "nav.title"),
            NavPage::Detail(n) => i18n::tf(cx, "nav.item", &[("n", &(n + 1))]),
            NavPage::Deep(n) => i18n::tf(cx, "nav.deeper", &[("n", &(n + 1))]),
        };
        let back_button = (depth > 0).then(|| {
            components::icon_button(
                &theme,
                "nav-back",
                &design::icons::CARET_LEFT,
                i18n::t(cx, "common.back"),
                Variant::OnBackground,
            )
            .on_click(stack_op(&cx.entity(), stack, None))
            .into_any_element()
        });
        let back = cx.entity();
        components::panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(heading(title, back_button))
            .child(div().flex_1().child(components::navigation(
                &theme,
                "nav",
                &self.stack,
                page,
                move |_, cx| {
                    back.update(cx, |v, cx| {
                        v.stack.pop();
                        v.swiped_back += 1;
                        cx.notify()
                    })
                },
            )))
    }
}

/// 親子関係を試す画面（`dialog-test`）。ダイアログ（[`Dialog`]）を子として開く 2 種類のボタンがある。
/// - `open-dialog`: 外側（この画面など）を押すと閉じる（既定）。
/// - `open-modal-dialog`: 外側を押しても閉じない（`PanelSpec::without_dismiss`）。`dialog-ok` / `dialog-cancel` で答えるまで残る。
///
/// 開いている間はこの画面が暗くなって押せず（掴んで動かすと子も付いてくる）、ダイアログを掴むと親も付いてくる（独立しない）。
#[derive(Default)]
pub struct DialogTest {
    panel: PanelId,
    /// ダイアログが答えた回数と、最後の答え。
    pub answered: usize,
    pub answer: Option<&'static str>,
}

impl DialogTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }
}

impl Render for DialogTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        let me = cx.entity().downgrade();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                row()
                    .child({
                        let me = me.clone();
                        button(&theme, "open-dialog", i18n::t(cx, "dialog.open"), false).on_click(
                            move |_, window, cx| {
                                let me = me.clone();
                                let spec = dialog_sized(
                                    PanelSpec::child(panel, true, move |id, _, _| {
                                        Dialog::new(
                                            id,
                                            me,
                                            &design::icons::INFO,
                                            "dialog.dismiss_title",
                                            "dialog.dismiss_body",
                                        )
                                    })
                                    // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
                                    .without_nav_bar(),
                                    "dialog.dismiss_title",
                                    "dialog.dismiss_body",
                                    window,
                                    cx,
                                );
                                cx.default_global::<PanelControl>().spawns.push(spec)
                            },
                        )
                    })
                    .child(
                        button(
                            &theme,
                            "open-modal-dialog",
                            i18n::t(cx, "dialog.open_modal"),
                            false,
                        )
                        .on_click(move |_, window, cx| {
                            let me = me.clone();
                            let spec = dialog_sized(
                                PanelSpec::child(panel, true, move |id, _, _| {
                                    Dialog::new(
                                        id,
                                        me,
                                        &design::icons::WARNING,
                                        "dialog.required_title",
                                        // わざと長い文。3 行を超える分は文の側だけがスクロールし、ボタンは残る。
                                        "dialog.required_body",
                                    )
                                })
                                .without_dismiss()
                                .without_nav_bar(),
                                "dialog.required_title",
                                "dialog.required_body",
                                window,
                                cx,
                            );
                            cx.default_global::<PanelControl>().spawns.push(spec)
                        }),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "dialog-answer".into())
                    .child(i18n::tf(
                        cx,
                        "dialog.answered",
                        &[
                            ("count", &self.answered),
                            (
                                "answer",
                                &self
                                    .answer
                                    .map_or("-".into(), |a| i18n::t(cx, answer_key(a))),
                            ),
                        ],
                    )),
            )
    }
}

/// ダイアログの答え（`OK` / `Cancel`）を出すときの文言のキー。
fn answer_key(answer: &str) -> &'static str {
    match answer {
        "OK" => "common.ok",
        _ => "common.cancel",
    }
}

/// [`DialogTest`] が開く、OK / Cancel だけのダイアログ（`components::dialog`）。押した答えを親へ渡して自分を閉じる。
/// `title` は種類の見分け用。`title` / `description` は文言のキー（描くときに今の言語で引く）。
pub struct Dialog {
    panel: PanelId,
    parent: WeakEntity<DialogTest>,
    glyph: &'static design::Icon,
    title: &'static str,
    description: &'static str,
}

impl Dialog {
    pub fn new(
        panel: PanelId,
        parent: WeakEntity<DialogTest>,
        glyph: &'static design::Icon,
        title: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            panel,
            parent,
            glyph,
            title,
            description,
        }
    }
}

impl Render for Dialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        // 答えの値（親に渡す）は言語によらず、見出しだけを訳す。
        let answer = |id: &'static str, value: &'static str, label: SharedString, variant| {
            let parent = self.parent.clone();
            let close = command(self.panel, PanelCommand::Close);
            components::button(&theme, id, label, variant).on_click(move |e, w, cx| {
                parent
                    .update(cx, |t, cx| {
                        t.answered += 1;
                        t.answer = Some(value);
                        cx.notify();
                    })
                    .ok();
                close(e, w, cx);
            })
        };
        components::dialog(
            &theme,
            "dialog",
            self.glyph,
            i18n::t(cx, self.title),
            i18n::t(cx, self.description),
            [
                answer(
                    "dialog-ok",
                    "OK",
                    i18n::t(cx, "common.ok"),
                    Variant::Primary,
                ),
                answer(
                    "dialog-cancel",
                    "Cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                ),
            ],
        )
    }
}

/// 半透明の背景に描画される画面（`testing-transparent`）。右上の Close ボタンで自分を閉じる。
pub const TRANSPARENT_BG: u32 = 0x202020a0;

#[derive(Default)]
pub struct TestingTransparent {}

impl TestingTransparent {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for TestingTransparent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_3()
            .bg(rgba(TRANSPARENT_BG))
            .text_color(rgba(theme.neutral_text))
            .child(i18n::t(cx, "testing.open.transparent"))
    }
}

/// ループアニメーションの周期（秒）。
const ANIM_PERIOD: f32 = 2.0;

/// 毎フレーム中身が変わる画面（`testing-anim`）: 描いた回数と、周期 [`ANIM_PERIOD`] で左右を往復し色の変わる四角。
/// 見えている間はフレームループが毎フレーム描き直す（[`ui::PanelState::live`]）。
pub struct TestingAnim {
    /// 描いた回数。
    pub frames: u64,
    start: Instant,
}

impl TestingAnim {
    pub fn new(panel: PanelId, cx: &mut App) -> Self {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .live = true;
        Self {
            frames: 0,
            start: Instant::now(),
        }
    }
}

impl Render for TestingAnim {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        self.frames += 1;
        let phase = self.start.elapsed().as_secs_f32() / ANIM_PERIOD % 1.0;
        // 0→1→0 と往復する。
        let x = 1.0 - (phase * 2.0 - 1.0).abs();
        components::card_panel(&theme)
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .debug_selector(|| "anim-frames".into())
                    .text_size(px(64.0))
                    .child(i18n::tf(cx, "testing.frame", &[("count", &self.frames)])),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(40.0))
                    .left(relative(0.05 + 0.7 * x))
                    .size(px(160.0))
                    .bg(hsla(phase, 0.8, 0.5, 1.0)),
            )
    }
}

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

/// スクロールバーの試験画面（`testing-scrollbar`）: 縦だけ・横だけ・縦横の 3 つのあふれる領域。
/// 領域は `scrollbar-v` / `scrollbar-h` / `scrollbar-both`、棒は `{領域}-bars-v` / `-h`（`-thumb` 付きが棒）。
pub struct TestingScrollbar {
    pub vertical: ScrollHandle,
    pub horizontal: ScrollHandle,
    pub both: ScrollHandle,
    /// 縦の領域の中にある、入れ子のスクロール要素。
    pub inner: ScrollHandle,
}

impl TestingScrollbar {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            vertical: ScrollHandle::new(),
            horizontal: ScrollHandle::new(),
            both: ScrollHandle::new(),
            inner: ScrollHandle::new(),
        }
    }
}

/// 中身の 1 マス（色で位置が分かる）。
fn cell(i: usize) -> Div {
    div()
        .flex_none()
        .size(px(48.0))
        .rounded(px(8.0))
        .bg(hsla((i % 12) as f32 / 12.0, 0.5, 0.45, 1.0))
        .flex()
        .items_center()
        .justify_center()
        .child(format!("{i}"))
}

impl Render for TestingScrollbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        // 領域の枠: 入れ物（relative）に、スクロールする本体と棒を重ねる。
        let area =
            |id: &'static str, pull: &'static str, handle: &ScrollHandle, body: Stateful<Div>| {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(8.0))
                    .bg(rgba(theme.control))
                    .child(body.size_full().track_scroll(handle))
                    .child(components::scrollbars(&theme, id, handle).pulled_by(pull))
            };
        let vertical = area(
            "bars-v",
            "pull-v",
            &self.vertical,
            div()
                .id("scrollbar-v")
                .debug_selector(|| "scrollbar-v".into())
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(
                    components::elastic("pull-v", &self.vertical)
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .child(
                            // 入れ子: ポインタが触れている内側だけが動く。
                            div()
                                .debug_selector(|| "scrollbar-nested".into())
                                .relative()
                                .flex_none()
                                .h(px(120.0))
                                .rounded(px(8.0))
                                .bg(rgba(theme.card))
                                .child(
                                    components::claim_scroll(
                                        div()
                                            .id("scrollbar-inner")
                                            .debug_selector(|| "scrollbar-inner".into())
                                            .size_full()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.inner),
                                        &self.inner,
                                    )
                                    .child(
                                        components::elastic("pull-inner", &self.inner)
                                            .flex_none()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .p_1()
                                            .children((0..20).map(|i| {
                                                div().flex_none().child(format!("inner {i}"))
                                            })),
                                    ),
                                )
                                .child(
                                    components::scrollbars(&theme, "bars-inner", &self.inner)
                                        .pulled_by("pull-inner"),
                                ),
                        )
                        .children((0..40).map(|i| cell(i).w_full())),
                ),
        );
        let horizontal = area(
            "bars-h",
            "pull-h",
            &self.horizontal,
            div()
                .id("scrollbar-h")
                .debug_selector(|| "scrollbar-h".into())
                .overflow_x_scroll()
                .flex()
                .child(
                    components::elastic("pull-h", &self.horizontal)
                        .flex_none()
                        .flex()
                        .gap_2()
                        .p_2()
                        .children((0..40).map(cell)),
                ),
        );
        let both = area(
            "bars-both",
            "pull-both",
            &self.both,
            components::free_scroll(div().id("scrollbar-both"))
                .debug_selector(|| "scrollbar-both".into())
                .overflow_scroll()
                .flex()
                .flex_col()
                .child(
                    components::elastic("pull-both", &self.both)
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        // 幅を中身に合わせないと、親の幅に揃えられて横にあふれない。
                        .w(px(30.0 * 56.0 + 16.0))
                        .children((0..20).map(|r| {
                            div()
                                .flex()
                                .flex_none()
                                .gap_2()
                                .children((0..30).map(move |c| cell(r * 30 + c)))
                        })),
                ),
        );
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap_2()
                    .child(vertical)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(horizontal)
                            .child(both),
                    ),
            )
    }
}

/// フォントの見本（`testing-font`）: 英数・かな漢字・混在を、太さと大きさを変えて並べる。
pub struct TestingFont {}

impl TestingFont {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for TestingFont {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        use gpui::FontWeight;
        let sample = |id: &'static str, size: f32, weight: FontWeight, text: &'static str| {
            div()
                .debug_selector(|| id.into())
                .text_size(px(size))
                .font_weight(weight)
                .child(text)
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(sample(
                "font-latin",
                24.0,
                FontWeight::NORMAL,
                "Gen Interface JP: The quick brown fox jumps over the lazy dog 0123456789",
            ))
            .child(sample(
                "font-latin-bold",
                24.0,
                FontWeight::BOLD,
                "Gen Interface JP Bold: Sphinx of black quartz, judge my vow",
            ))
            .child(sample(
                "font-ja",
                24.0,
                FontWeight::NORMAL,
                "Gen Interface JP 和文: 吾輩は猫である。名前はまだ無い。",
            ))
            .child(sample(
                "font-ja-bold",
                24.0,
                FontWeight::BOLD,
                "Gen Interface JP 太字: いろはにほへと ちりぬるを",
            ))
            .child(sample(
                "font-mixed",
                19.0,
                FontWeight::NORMAL,
                "Mixed 混在: Hello、世界! VR パネル 123 ABC あいう",
            ))
            .child(sample(
                "font-small",
                14.0,
                FontWeight::NORMAL,
                "Small 14px: 小さい文字の読みやすさ — ABC abc 漢字かな",
            ))
    }
}

/// ローカライズを確かめる画面（`locale-test`）。
/// - `lang-en` / `lang-ja` で言語を切り替える（全パネルに効き、設定ファイルの `language` に残る。`PanelCommand::Language`）。
///   ボタンの名前はその言語自身で出す。今の言語のボタンが光る。
/// - `locale-current` に今の言語、`locale-missing` に訳の抜け（言語ごとに表に無いキー）を出す。
/// - 見本（`locale-samples`）: 長い文の折り返し、差し込み（`{count}`）、訳で長さの変わるボタン。
/// - `locale-table` に全キーを、言語ごとの文言と並べて出す（今の言語の列を強調）。抜けは赤字で `—`。
pub struct LocaleTest {
    panel: PanelId,
    pub table: ScrollHandle,
}

impl LocaleTest {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            table: ScrollHandle::new(),
        }
    }
}

impl Render for LocaleTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let now = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map_or(Language::default(), |c| c.status.language);
        let current = lang(now);
        let switch = row().children(i18n::Lang::ALL.map(|l| {
            let id = match l {
                i18n::Lang::En => "lang-en",
                i18n::Lang::Ja => "lang-ja",
            };
            button(&theme, id, l.native_name(), l == current)
                .on_click(command(self.panel, PanelCommand::Language(language(l))))
        }));
        let missing: Vec<(i18n::Lang, Vec<&'static str>)> = i18n::Lang::ALL
            .into_iter()
            .map(|l| (l, i18n::missing(l)))
            .filter(|(_, keys)| !keys.is_empty())
            .collect();
        let missing = div()
            .debug_selector(|| "locale-missing".into())
            .flex()
            .flex_col()
            .gap(px(spacing::XS))
            .when(missing.is_empty(), |d| {
                d.text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "locale.complete"))
            })
            .children(missing.into_iter().map(|(l, keys)| {
                div()
                    .text_color(rgba(theme.danger_text))
                    .child(i18n::tf(
                        cx,
                        "locale.missing",
                        &[("language", &l.native_name()), ("count", &keys.len())],
                    ))
                    .child(div().text_size(px(font_size::SM)).child(keys.join(", ")))
            }));
        let samples = div()
            .debug_selector(|| "locale-samples".into())
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .child(div().child(i18n::t(cx, "locale.sample_long")))
            .child(div().child(i18n::tf(cx, "locale.sample_count", &[("count", &3)])))
            .child(row().child(button(
                &theme,
                "locale-sample-button",
                i18n::t(cx, "locale.sample_button"),
                false,
            )));
        // 全キーの表: キーと、言語ごとの文言。
        let cell = |text: SharedString, strong: bool| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(spacing::SM))
                .when(!strong, |d| d.text_color(rgba(theme.neutral_text_low)))
                .child(text)
        };
        let header = div()
            .flex()
            .py(px(spacing::XS))
            .font_weight(FontWeight::BOLD)
            .child(cell(i18n::t(cx, "locale.key"), true))
            .children(i18n::Lang::ALL.map(|l| cell(l.native_name().into(), l == current)));
        let rows = i18n::keys().into_iter().map(|key| {
            div()
                .flex()
                .py(px(spacing::XS))
                .text_size(px(font_size::SM))
                .child(cell(key.into(), false))
                .children(i18n::Lang::ALL.map(|l| match i18n::get(l, key) {
                    Some(text) => cell(text.into(), l == current),
                    None => cell("—".into(), true).text_color(rgba(theme.danger_text)),
                }))
        });
        let table = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("locale-table")
                    .debug_selector(|| "locale-table".into())
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.table)
                    .child(
                        components::elastic("pull-locale", &self.table)
                            .flex()
                            .flex_col()
                            .child(header)
                            .children(rows),
                    ),
            )
            .child(
                components::scrollbars(&theme, "locale-bars", &self.table).pulled_by("pull-locale"),
            );
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .p(px(spacing::XL))
            .text_color(rgba(theme.neutral_text))
            .child(switch)
            .child(
                div()
                    .debug_selector(|| "locale-current".into())
                    .child(i18n::tf(
                        cx,
                        "locale.current",
                        &[("language", &current.native_name())],
                    )),
            )
            .child(missing)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(i18n::t(cx, "locale.samples")),
            )
            .child(samples)
            .child(table)
    }
}

/// 負荷試験で出すパネルの大きさ（m）。解像度は既定のまま（800 × 600）。
pub const STRESS_SIZE: Vec2 = Vec2::new(0.3, 0.225);
/// 一度に足す枚数（[`StressTest`] の `stress-add`）。
pub const STRESS_STEP: usize = 10;
/// 並べるときの列数と間隔（m）、頭からの距離（m）。
const GRID_COLS: usize = 8;
const GRID_GAP: Vec2 = Vec2::new(0.33, 0.255);
const GRID_DISTANCE: f32 = 2.0;

/// 負荷試験の画面（`stress-test`）。[`TestingAnim`]・[`TestingLod`]・[`TestingTransparent`] を順に出す。
/// `stress-fill` は開いているパネルが上限（[`ui::PanelControl::limit`]: [`ui::MAX_PANELS`] とランタイムの上限の小さい方）
/// ちょうどになるまで出す。ランタイムがそれより手前で作れなくなれば（OpenVR のほかのアプリの分など）そこで止まり、
/// 届いた数が `stress-count` に出る。
pub struct StressTest {
    /// 出したパネル（閉じたものも含む）。ビルド時に積まれる。
    spawned: Rc<RefCell<Vec<PanelId>>>,
    rng: u64,
}

impl StressTest {
    pub fn new(_panel: PanelId) -> Self {
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            spawned: Rc::default(),
            rng: seed | 1,
        }
    }

    /// 出したパネルのうち閉じていないもの。
    fn alive(&self, cx: &App) -> Vec<PanelId> {
        let ctl = cx.try_global::<PanelControl>();
        self.spawned
            .borrow()
            .iter()
            .copied()
            .filter(|id| {
                ctl.and_then(|c| c.panels.get(id))
                    .is_none_or(|s| !s.status.closed)
            })
            .collect()
    }

    /// 上限を超えない範囲で、最大 `n` 枚を並べる位置に出す。
    fn spawn(&mut self, n: usize, cx: &mut App) {
        let start = self.alive(cx).len();
        let c = cx.default_global::<PanelControl>();
        // spawns に積んだまま次のフレームを待っている分も数える。
        let room = c.limit.saturating_sub(c.open + c.spawns.len());
        for i in start..start + n.min(room) {
            let ids = self.spawned.clone();
            let track = move |id| ids.borrow_mut().push(id);
            let spec = match i % 3 {
                0 => PanelSpec::new(Pose::IDENTITY, move |id, _, cx| {
                    track(id);
                    TestingAnim::new(id, cx)
                }),
                1 => PanelSpec::new(Pose::IDENTITY, move |id, _, _| {
                    track(id);
                    TestingLod::new(id)
                }),
                _ => PanelSpec::new(Pose::IDENTITY, move |id, _, _| {
                    track(id);
                    TestingTransparent::new(id)
                }),
            };
            c.spawns.push(
                spec.in_view(grid_pose(i))
                    .sized(STRESS_SIZE, (800, 600))
                    // 負荷を測るための埋め草で、まとめて閉じる（`stress-close`）。バーを付けると開ける数が半分になる。
                    .without_nav_bar(),
            );
        }
    }

    /// 閉じていない出したパネルに、`i` 枚目なら `f(i)` の操作を積む。
    fn each(&self, cx: &mut App, mut f: impl FnMut(usize) -> PanelCommand) {
        let alive = self.alive(cx);
        let c = cx.default_global::<PanelControl>();
        for (i, id) in alive.into_iter().enumerate() {
            c.panels.entry(id).or_default().commands.push(f(i));
        }
    }

    /// xorshift64 で 0〜1。
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// 並べるときの `i` 枚目の位置（頭から見た相対姿勢）。正面 [`GRID_DISTANCE`] に [`GRID_COLS`] 列で上から下へ。
pub fn grid_pose(i: usize) -> Pose {
    let (col, row) = ((i % GRID_COLS) as f32, (i / GRID_COLS) as f32);
    Pose::new(
        Vec3::new(
            (col - (GRID_COLS - 1) as f32 / 2.0) * GRID_GAP.x,
            0.6 - row * GRID_GAP.y,
            -GRID_DISTANCE,
        ),
        Quat::IDENTITY,
    )
}

/// `p` に置いたパネルが頭（原点）の方を向く姿勢。
fn facing_head(p: Vec3) -> Pose {
    Pose::new(p, Quat::from_rotation_y((-p.x).atan2(-p.z)))
}

impl Render for StressTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (open, limit) = cx
            .try_global::<PanelControl>()
            .map_or((0, 0), |c| (c.open, c.limit));
        let alive = self.alive(cx).len();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .debug_selector(|| "stress-count".into())
                    .child(i18n::tf(
                        cx,
                        "stress.count",
                        &[("open", &open), ("limit", &limit), ("spawned", &alive)],
                    )),
            )
            .child(
                row()
                    .child(
                        button(&theme, "stress-fill", i18n::t(cx, "stress.fill"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(usize::MAX, cx))),
                    )
                    .child(
                        button(&theme, "stress-add", format!("+{STRESS_STEP}"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(STRESS_STEP, cx))),
                    )
                    .child(
                        button(
                            &theme,
                            "stress-close",
                            i18n::t(cx, "stress.close_all"),
                            false,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.each(cx, |_| PanelCommand::Close)),
                        ),
                    )
                    .child(
                        button(
                            &theme,
                            "stress-scatter",
                            i18n::t(cx, "stress.scatter"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            // 左右 ±3m・上下 -1〜1.5m・前方 0.8〜8m（距離がばらけて LOD もばらける）。
                            let poses: Vec<_> = (0..this.alive(cx).len())
                                .map(|_| {
                                    facing_head(Vec3::new(
                                        this.random() * 6.0 - 3.0,
                                        this.random() * 2.5 - 1.0,
                                        -0.8 - this.random() * 7.2,
                                    ))
                                })
                                .collect();
                            this.each(cx, |i| PanelCommand::Place(poses[i]));
                        })),
                    )
                    .child(
                        button(&theme, "stress-grid", i18n::t(cx, "stress.grid"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.each(cx, |i| PanelCommand::Place(grid_pose(i)))
                            }),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};

    /// ボタンは操作要求を積むだけで、反映はフレームループ（`ui::sync_panel`）の仕事。
    #[gpui::test]
    fn attach_button_queues_command(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("attach-hmd").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        let queued = cx.update(|_, cx| cx.global::<PanelControl>().panels[&0].commands.clone());
        assert_eq!(queued, [PanelCommand::Attach(Device::Hmd, HMD_OFFSET)]);
    }

    /// テーマボタンは全体のテーマを切り替える要求を積む。切り替わると画面の色も変わる。
    #[gpui::test]
    fn theme_button_requests_toggle_and_screen_follows(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("toggle-theme").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        let queued = cx.update(|_, cx| {
            cx.global::<PanelControl>().panels[&PanelId::default()]
                .commands
                .clone()
        });
        assert_eq!(queued, [PanelCommand::Theme(kernel::ThemeMode::Light)]);
        cx.update(|_, cx| cx.set_global(Mode::Light));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| Theme::current(cx)) == Theme::LIGHT);
    }

    /// 言語ボタンは次の言語へ切り替える要求を積む（設定ファイルへの書き戻しはカーネルの仕事）。
    #[gpui::test]
    fn language_button_requests_the_next_language(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("toggle-language").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        let queued = cx.update(|_, cx| {
            cx.global::<PanelControl>().panels[&PanelId::default()]
                .commands
                .clone()
        });
        assert_eq!(queued, [PanelCommand::Language(Language::Ja)]);
    }

    /// ローカライズの画面: 言語のボタンが切り替えを要求し、言語が変わると文言が訳されて長さも変わる。
    #[gpui::test]
    fn locale_screen_switches_language_and_text_follows(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| LocaleTest::new(0));
        cx.simulate_resize(gpui::size(px(1200.0), px(1000.0)));
        cx.run_until_parked();
        let width = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("locale-sample-button").unwrap().size.width
        };
        let english = width(cx);
        let c = cx.debug_bounds("lang-ja").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        let queued = cx.update(|_, cx| cx.global::<PanelControl>().panels[&0].commands.clone());
        assert_eq!(queued, [PanelCommand::Language(Language::Ja)]);
        // フレームループ（ui::sync_panel）の代わりに、状態とグローバルの言語を反映する。
        cx.update(|window, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(0)
                .or_default()
                .status
                .language = Language::Ja;
            cx.set_global(i18n::Lang::Ja);
            window.refresh();
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| i18n::t(cx, "locale.sample_button")),
            "変更を保存"
        );
        assert_ne!(
            width(cx),
            english,
            "the button is laid out with the translation"
        );
    }

    /// あふれた一覧にはスクロールバーが出て、棒を掴んで引くと一覧が動く。
    #[gpui::test]
    fn scrollbar_appears_and_drags(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Testing::default());
        // 一覧（30 行 × 24px）があふれる高さにする。
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        // 初回のレイアウトで初めてあふれが分かるので、描き直されてから出る（本番はフレームループが描き直す）。
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let thumb = cx.debug_bounds("scroll-bars-v-thumb").expect("棒が出る");
        let from = thumb.center();
        cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            from + gpui::point(px(0.0), px(40.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            from + gpui::point(px(0.0), px(40.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        let y = view.read_with(cx, |t, _| t.list.offset().y);
        assert!(y < px(0.0), "引いた分だけ一覧が進む: {y:?}");
    }

    /// タッチのスライド（長押しなし）で棒の上から動かすと、棒が動いて中身が進み、中身は惰性で流れない。
    #[gpui::test]
    fn scrollbar_follows_a_touch_slide_without_long_press(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let at = cx.debug_bounds("scroll-bars-v-thumb").unwrap().center();
        let wheel = |phase| ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(30.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: phase,
        };
        cx.simulate_event(wheel(gpui::TouchPhase::Started));
        let y = view.read_with(cx, |t, _| t.list.offset().y);
        assert!(y < px(-30.0), "棒が 30px 動くと中身はそれ以上進む: {y:?}");
        cx.simulate_event(wheel(gpui::TouchPhase::Ended));
        assert_eq!(
            view.read_with(cx, |t, _| t.list.offset().y),
            y,
            "惰性では動かない"
        );
    }

    /// 先頭に着いたままさらに引くと中身が引っ張られ、時間が経つと戻る（動きが有効なときだけ）。
    #[gpui::test]
    fn content_is_pulled_at_the_edge_and_settles(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
        cx.update(|_, cx| cx.set_global(design::Motion::default()));
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let area = cx.debug_bounds("scrollbar-v").unwrap();
        let top = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("scrollbar-nested").unwrap().origin.y
        };
        let rest = top(cx);
        cx.simulate_event(ScrollWheelEvent {
            position: gpui::point(area.center().x, area.bottom() - px(20.0)),
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let pulled = top(cx);
        assert!(pulled > rest, "先頭を越えて引くと中身が下へ引かれる");
        // 引き続けると上限なく引っ張れる（戻る力は離したときだけ）。
        for _ in 0..20 {
            cx.simulate_event(ScrollWheelEvent {
                position: gpui::point(area.center().x, area.bottom() - px(20.0)),
                delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
                modifiers: gpui::Modifiers::none(),
                touch_phase: gpui::TouchPhase::Moved,
            });
        }
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(
            top(cx) > pulled + px(100.0),
            "引くほど伸びる: {:?}",
            top(cx)
        );
        // 指を離すと戻り始める。
        cx.simulate_event(ScrollWheelEvent {
            position: gpui::point(area.center().x, area.bottom() - px(20.0)),
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(0.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::Ended,
        });
        std::thread::sleep(std::time::Duration::from_millis(900));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(top(cx), rest, "離したら戻る");
    }

    /// 末尾でも上限なく引っ張れ、タッチは手を止めても（離すまで）戻らない。
    #[gpui::test]
    fn pull_at_the_end_is_unlimited_and_held_while_touching(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
        cx.update(|_, cx| cx.set_global(design::Motion::default()));
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        // 一番下までスクロールしておく。
        view.read_with(cx, |t, _| {
            let m = t.vertical.max_offset();
            t.vertical.set_offset(gpui::point(px(0.0), -m.y));
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let area = cx.debug_bounds("scrollbar-v").unwrap();
        let at = gpui::point(area.center().x, area.bottom() - px(20.0));
        let top = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("scrollbar-nested").unwrap().origin.y
        };
        let rest = top(cx);
        let wheel = |dy: f32, phase| ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(dy))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: phase,
        };
        cx.simulate_event(wheel(-40.0, gpui::TouchPhase::Started));
        for _ in 0..20 {
            cx.simulate_event(wheel(-40.0, gpui::TouchPhase::Moved));
        }
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(
            top(cx) < rest - px(150.0),
            "末尾でも上限なく引っ張れる: {:?}",
            top(cx)
        );
        // 手を止めても、離す合図が来るまで戻らない。
        let held = top(cx);
        std::thread::sleep(std::time::Duration::from_millis(700));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(top(cx), held, "タッチ中は手を止めても戻らない");
        cx.simulate_event(wheel(0.0, gpui::TouchPhase::Ended));
        std::thread::sleep(std::time::Duration::from_millis(900));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(top(cx), rest, "離したら戻る");
    }

    /// 入れ子の内側を引っ張っても、外側の棒は反応しない。
    #[gpui::test]
    fn pulling_the_inner_does_not_squash_the_outer_bar(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
        cx.update(|_, cx| cx.set_global(design::Motion::default()));
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let outer = cx.debug_bounds("bars-v-v-thumb").unwrap().size.height;
        let inner = cx.debug_bounds("bars-inner-v-thumb").unwrap().size.height;
        let at = cx.debug_bounds("scrollbar-inner").unwrap().center();
        cx.simulate_event(ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(40.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(
            cx.debug_bounds("bars-v-v-thumb").unwrap().size.height,
            outer
        );
        assert!(
            cx.debug_bounds("bars-inner-v-thumb").unwrap().size.height < inner,
            "内側の棒は縮む"
        );
    }

    /// 入れ子のスクロール要素の上でホイールを回すと、内側だけが動いて外側は動かない。
    #[gpui::test]
    fn nested_scroll_moves_only_the_innermost(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let at = cx.debug_bounds("scrollbar-inner").unwrap().center();
        cx.simulate_event(ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(-30.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        let (inner, outer) = view.read_with(cx, |t, _| (t.inner.offset().y, t.vertical.offset().y));
        assert!(inner < px(0.0), "内側が進む: {inner:?}");
        assert_eq!(outer, px(0.0), "外側は動かない");
    }

    /// スクロールバーの試験画面は 3 つの領域それぞれに棒が出る（縦横の領域は両方出る）。
    #[gpui::test]
    fn scrollbar_screen_shows_bars_for_overflowing_axes(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| TestingScrollbar::new(0));
        cx.simulate_resize(gpui::size(px(1200.0), px(900.0)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        for name in [
            "bars-v-v-thumb",
            "bars-h-h-thumb",
            "bars-both-v-thumb",
            "bars-both-h-thumb",
        ] {
            assert!(cx.debug_bounds(name).is_some(), "{name} が出る");
        }
        assert!(
            cx.debug_bounds("bars-v-h-thumb").is_none(),
            "縦だけの領域に横の棒は出ない"
        );
    }

    /// Profiler ボタンは計測の有効・無効を切り替える。
    #[gpui::test]
    fn profiler_button_toggles_profiling(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        let profiling = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| cx.global::<PanelControl>().profiling)
        };
        let c = cx.debug_bounds("toggle-profiler").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        assert!(profiling(cx));
        cx.simulate_click(c, Modifiers::none());
        assert!(!profiling(cx));
    }

    /// 描画のテストパネルと負荷試験の画面は Testing から召喚できる。
    #[gpui::test]
    fn test_panel_buttons_spawn_panels(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        for id in [
            "spawn-anim",
            "spawn-lod",
            "spawn-font",
            "stress-test",
            "keyboard-test",
        ] {
            let c = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(c, Modifiers::none());
        }
        let spawns = cx.update(|_, cx| cx.global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 5);
    }

    #[gpui::test]
    fn keyboard_test_opens_in_front_of_the_view_instead_of_inheriting_the_source_tilt(
        cx: &mut TestAppContext,
    ) {
        let mut kernel = kernel::Kernel::with_backend(
            kernel::vr::mock::MockBackend::default(),
            Default::default(),
        );
        kernel.subsystems.backend.frame.hmd =
            Pose::new(Vec3::new(2., 1., 3.), Quat::from_rotation_y(0.8));
        kernel.tick();
        let source = kernel
            .add_panel(
                "source",
                Pose::new(Vec3::new(5., -2., -4.), Quat::from_rotation_z(1.)),
                TESTING_SIZE,
                TESTING_PX,
            )
            .unwrap();
        let (_, cx) = cx.add_window_view(|_, _| Testing::new(source));
        cx.run_until_parked();
        let at = cx.debug_bounds("keyboard-test").unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.update(|_, cx| {
            let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
            let pose = spec.pose_for(&kernel, 1);
            assert_eq!(
                pose,
                kernel.hmd().leveled() * Pose::new(Vec3::new(0., 0.23, -0.85), Quat::IDENTITY)
            );
            assert!(spec.parent().is_none());
        });
    }

    /// Fill は上限ちょうどまで出し（待っている分も数える）、Grid・Close all は出したもののうち開いているものだけに効く。
    #[gpui::test]
    fn stress_fill_stops_at_limit_and_commands_reach_spawned(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| StressTest::new(0));
        cx.update(|_, cx| {
            let c = cx.default_global::<PanelControl>();
            (c.open, c.limit) = (10, 15);
        });
        cx.run_until_parked();
        let fill = cx.debug_bounds("stress-fill").unwrap().center();
        cx.simulate_click(fill, Modifiers::none());
        cx.simulate_click(fill, Modifiers::none());
        let spawns =
            cx.update(|_, cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
        assert_eq!(spawns.len(), 5);

        // 開いたことにする（ビルドで積まれる ID の代わり）。1 枚は閉じられた。
        view.update(cx, |s, _| s.spawned.borrow_mut().extend([1, 2, 3]));
        cx.update(|_, cx| {
            let c = cx.default_global::<PanelControl>();
            c.panels.entry(2).or_default().status.closed = true;
        });
        for id in ["stress-grid", "stress-close"] {
            let c = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(c, Modifiers::none());
        }
        let commands = cx.update(|_, cx| {
            let c = cx.global::<PanelControl>();
            [1, 2, 3].map(|id| c.panels.get(&id).map_or(0, |s| s.commands.len()))
        });
        assert_eq!(commands, [2, 0, 2]);
    }

    #[test]
    fn grid_rows_are_full_before_wrapping() {
        assert_eq!(grid_pose(0).position.y, grid_pose(GRID_COLS - 1).position.y);
        assert!(grid_pose(GRID_COLS).position.y < grid_pose(0).position.y);
        assert!(grid_pose(0).position.x < 0.0 && grid_pose(GRID_COLS - 1).position.x > 0.0);
    }
}
