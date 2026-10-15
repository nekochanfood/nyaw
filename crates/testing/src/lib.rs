//! Layer 5 画面: Testing パネル。UI 層（`ui`）の上に載る画面の1つで、一連の機能を1画面で試せるテスト用の画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック数・HMD / 手 / トラッカーへの取り付け・取り外し・ロック・メニューボタンと掴みの表示
//! - GPUI: 切り替え・押している間の見た目・ドラッグ＆ドロップ・スクロールする一覧・フォーカスと文字入力・ポップアップメニュー
//! - ポインタの方式: [`PointerTest`] でマウス・タッチのエミュレートを切り替える
//! - ツールチップ: [`TooltipTest`] で、カーソルに付いて出る説明（向き・猶予・文字の差し替え・幅の伸び縮み）を見る
//! - 親子: [`DialogTest`] がダイアログ（[`Dialog`]）を子として開く。開いている間の親は暗くなり、子を掴むと親も付いてくる
//! - 描画: [`TestingAnim`]（毎フレーム変わる）・[`TestingLod`]（今の LOD を大きく出す）・[`TestingFont`]（フォントの見本）・[`TestingScrollbar`]（縦・横・両方のスクロールバー）
//! - 負荷: [`StressTest`] で上限いっぱいまでパネルを出し、閉じる・ちりばめる・並べる
//!
//! 要素はすべて `debug_selector` で名前を付けてある（名前 = `id`）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

use design::components::{self, Variant};
use design::{Mode, Theme};
use gpui::{
    App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Stateful, WeakEntity, Window, anchored, deferred, div, hsla, prelude::*, px,
    relative, rgb, rgba,
};
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, PointerMode};
use ui::{PanelControl, PanelSpec};

/// 「HMD に取り付け」で使う相対姿勢: 顔の正面 1m。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」で使う相対姿勢: 0.3m 前（手首の時計のような配置）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// 色は `design` のダークテーマの役割から引く（`color_at` は 0xRRGGBB なのでアルファを落とす）。
const THEME: Theme = Theme::of(Mode::Dark);
const fn rgb24(rgba: u32) -> u32 {
    rgba >> 8
}
/// パネルの地。どの画面もカード 1 枚なので、カードがそのままパネルになる。
pub const PANEL_BG: u32 = rgb24(THEME.card);
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
/// Testing パネルの大きさ（m）と解像度（px）。ボタンが高い（最小ヒット領域）ので、既定の 0.8m × 0.6m では入りきらない。
pub const TESTING_SIZE: Vec2 = Vec2::new(1.2, 0.9);
pub const TESTING_PX: (u32, u32) = (1200, 900);
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
    label: impl Into<String>,
    active: bool,
) -> components::Button {
    themed_button(theme, id, label, active)
}

fn themed_button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<String>,
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
            PanelSpec::spawn(panel, |id, _, _| Testing::new(id)).sized(TESTING_SIZE, TESTING_PX),
        )
    }
}

fn spawn_dialog_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, |id, _, _| DialogTest::new(id)))
    }
}

fn spawn_pointer_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, |id, _, _| PointerTest::new(id)))
    }
}

fn spawn_transparent(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, |id, _, _| {
                TestingTransparent::new(id)
            }))
    }
}

/// 隣に `build` の画面を開く。
fn spawn_view<V: Render>(
    panel: PanelId,
    build: fn(PanelId, &mut App) -> V,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, move |id, _, cx| build(id, cx)))
    }
}

/// design のカタログを隣に開く（Close はこのパネルを閉じる）。
fn spawn_catalog(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, |id, _, cx| {
                // 動きの見本を毎フレーム描く。
                cx.default_global::<PanelControl>()
                    .panels
                    .entry(id)
                    .or_default()
                    .live = true;
                design::catalog::Catalog::with_close(command(id, PanelCommand::Close))
            }))
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

        let vr_row = row()
            .child(
                button!("animate-panel", "Animation", st.animated)
                    .on_click(command(self.panel, PanelCommand::Animated(!st.animated))),
            )
            .child(button!("spawn-panel", "Spawn panel", false).on_click(spawn(self.panel)))
            .child(
                button!("spawn-transparent", "Spawn transparent", false)
                    .on_click(spawn_transparent(self.panel)),
            )
            .child(
                button!("spawn-anim", "Spawn anim", false)
                    .on_click(spawn_view(self.panel, TestingAnim::new)),
            )
            .child(
                button!("spawn-lod", "Spawn LOD", false)
                    .on_click(spawn_view(self.panel, |id, _| TestingLod::new(id))),
            )
            .child(
                button!("spawn-font", "Spawn font", false)
                    .on_click(spawn_view(self.panel, |id, _| TestingFont::new(id))),
            )
            .child(
                button!("spawn-scrollbar", "Spawn scrollbar", false)
                    .on_click(spawn_view(self.panel, |id, _| TestingScrollbar::new(id))),
            )
            .child(
                button!("spawn-catalog", "Spawn catalog", false)
                    .on_click(spawn_catalog(self.panel)),
            )
            .child(
                button!("stress-test", "Stress test", false)
                    .on_click(spawn_view(self.panel, |id, _| StressTest::new(id))),
            )
            .child(
                button!("pointer-test", "Pointer test", false)
                    .on_click(spawn_pointer_test(self.panel)),
            )
            .child(
                button!("tooltip-test", "Tooltip test", false)
                    .on_click(spawn_view(self.panel, |id, _| TooltipTest::new(id))),
            )
            .child(
                button!("dialog-test", "Dialog test", false)
                    .on_click(spawn_dialog_test(self.panel)),
            )
            .child(
                button!("close-panel", "Close panel", false)
                    .on_click(command(self.panel, PanelCommand::Close)),
            )
            .child(
                button!("toggle-profiler", "Profiler", profiling).on_click(|_, _, cx| {
                    let c = cx.default_global::<PanelControl>();
                    c.profiling = !c.profiling;
                }),
            )
            .child(
                button!("click-me", format!("Clicked {}", self.clicks), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button!("attach-hmd", "HMD", on(&Device::Hmd)).on_click(command(
                    self.panel,
                    PanelCommand::Attach(Device::Hmd, attach_offset(self.panel, HMD_OFFSET)),
                )),
            )
            .child(
                button!(
                    "attach-left",
                    "Left hand",
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
                    "Right hand",
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
                        Some(Device::Tracker(id)) => format!("Tracker {id}"),
                        _ => "Tracker (none)".into(),
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
                button!("detach", "Detach", false)
                    .on_click(command(self.panel, PanelCommand::Detach)),
            )
            .child(
                button!("lock", "Lock", st.locked)
                    .on_click(command(self.panel, PanelCommand::Lock(!st.locked))),
            )
            .child(button!("menu-held", "Menu", st.menu))
            .child(button!("grabbed", "Grab", st.grabbed));

        let gpui_row = row()
            .child(
                button!("toggle-theme", format!("Theme: {:?}", st.theme), false)
                    .on_click(command(self.panel, PanelCommand::Theme(st.theme.toggled()))),
            )
            .child(
                button!("toggle", "Toggle", self.toggled).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.toggled = !this.toggled;
                        cx.notify();
                    },
                )),
            )
            .child(button!("hold-me", "Hold", false).active(|s| s.bg(rgba(theme.control_active))))
            .child(
                button!("drag-chip", "Drag me", false)
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
                    .child(format!("Dropped {}", self.dropped)),
            )
            .child(
                div()
                    .child(
                        button!(
                            "open-menu",
                            format!("Menu: {}", self.picked.unwrap_or("-")),
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
                                    .flex()
                                    .flex_col()
                                    .bg(rgba(theme.control))
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

        let field = div()
            .id("text-field")
            .debug_selector(|| "text-field".into())
            .track_focus(&focus)
            .p_2()
            .bg(rgba(theme.control))
            .focus(|s| s.bg(rgba(theme.accent_bg_active)))
            .on_click(cx.listener(move |_, _, window, cx| window.focus(&focus, cx)))
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
            .child(format!("Text: {}", self.text));

        let list = div()
            .id("scroll-list")
            .debug_selector(|| "scroll-list".into())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.list)
            .bg(rgba(theme.control))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                this.scrolled += match e.delta {
                    ScrollDelta::Lines(p) => p.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / SCROLL_LINE_PX,
                };
                cx.notify();
            }))
            .child(
                components::elastic("pull-list", &self.list).children((0..LIST_ROWS).map(|i| {
                    div()
                        .h(gpui::px(ROW_PX))
                        .px_2()
                        .child(format!("Row {i} (scrolled {:.1})", self.scrolled))
                })),
            );

        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(format!(
                "runtime: {:?} / attached: {:?} / locked: {}",
                st.runtime, st.attached, st.locked
            ))
            .child(vr_row)
            .child(gpui_row)
            .child(field)
            .child(div().relative().flex_1().min_h_0().child(list).child(
                components::scrollbars(&theme, "scroll-bars", &self.list).pulled_by("pull-list"),
            ))
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
            PointerMode::Mouse => ("Mode: Mouse", PointerMode::Touch),
            PointerMode::Touch => ("Mode: Touch", PointerMode::Mouse),
        };
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
                        button(&theme, "tap-me", format!("Tapped {}", self.taps), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.taps += 1;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        button(&theme, "close-pointer-test", "Close", false)
                            .on_click(command(self.panel, PanelCommand::Close)),
                    ),
            )
            .child(
                row()
                    .child(
                        button(&theme, "drag-chip", "Drag me", false)
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
                            .child(format!("Dropped {}", self.dropped)),
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
                    .children((0..LIST_ROWS).map(|i| {
                        div()
                            .h(gpui::px(ROW_PX))
                            .px_2()
                            .child(format!("Swipe row {i}"))
                    })),
            )
    }
}

/// ツールチップを試す画面（`tooltip-test`）。
/// - `tip-short` / `tip-long` は隣り合っていて、移ると猶予（0.5 秒）の間に文字が変わり、箱の幅が伸び縮みする。
/// - `tip-far` は離れていて、届くまでに猶予が過ぎると一度消えてから出直す。`no-tip` には説明が無い（指すと猶予の後に消える）。
/// - アイコンだけのボタン（`tip-icon-*`）には必ず説明が付く（`components::icon_button`）。
/// - `tip-status` に、手ごとの今の文字と向き（上下）を出す。手を頭より上に掲げて見上げると下に、下から指すと上に出る。
#[derive(Default)]
pub struct TooltipTest {
    panel: PanelId,
}

impl TooltipTest {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
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
        let icon =
            |id, glyph, tip| components::icon_button(&theme, id, glyph, tip, Variant::Default);
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .justify_between()
            .p_3()
            .child(
                row()
                    .child(div().debug_selector(|| "tip-status".into()).child(status))
                    .child(
                        button(&theme, "close-tooltip-test", "Close", false)
                            .on_click(command(self.panel, PanelCommand::Close)),
                    ),
            )
            .child(
                row()
                    .child(button(&theme, "tip-short", "Short", false).tooltip("Save"))
                    .child(
                        button(&theme, "tip-long", "Long", false)
                            .tooltip("Open the settings of this panel"),
                    )
                    .child(button(&theme, "no-tip", "No tooltip", false)),
            )
            .child(
                row()
                    .child(icon("tip-icon-gear", &design::icons::GEAR, "Settings"))
                    .child(icon(
                        "tip-icon-heart",
                        &design::icons::HEART,
                        "Add to favorites",
                    ))
                    .child(icon(
                        "tip-icon-search",
                        &design::icons::MAGNIFYING_GLASS,
                        "検索",
                    )),
            )
            .child(
                row()
                    .justify_end()
                    .child(button(&theme, "tip-far", "Far", false).tooltip("Far away")),
            )
    }
}

/// ダイアログの大きさ。
/// 上からアイコン・タイトル・概要・ボタン（2 個）。文の幅 344px。高さは、概要が収まらずスクロールしても
/// 3 行は見える下限（`components::dialog_height`）。1px = 1mm。
const DIALOG_PX: (u32, u32) = (440, components::dialog_height(2));
const DIALOG_SIZE: Vec2 = Vec2::new(
    DIALOG_PX.0 as f32 / 1000.0,
    DIALOG_PX.1 as f32 / 1000.0,
);

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
            .child(title_bar(&theme, panel, "dialog-test", "close-dialog-test"))
            .child(
                row()
                    .child({
                        let me = me.clone();
                        button(&theme, "open-dialog", "Open dialog", false).on_click(
                            move |_, _, cx| {
                                let me = me.clone();
                                cx.default_global::<PanelControl>().spawns.push(
                                    PanelSpec::child(panel, true, move |id, _, _| {
                                        Dialog::new(
                                            id,
                                            me,
                                            &design::icons::INFO,
                                            "Click outside to dismiss",
                                            "Answer below, or press outside this dialog to close it.",
                                        )
                                    })
                                    .sized(DIALOG_SIZE, DIALOG_PX),
                                )
                            },
                        )
                    })
                    .child(
                        button(&theme, "open-modal-dialog", "Open modal dialog", false)
                            .on_click(move |_, _, cx| {
                                let me = me.clone();
                                cx.default_global::<PanelControl>().spawns.push(
                                    PanelSpec::child(panel, true, move |id, _, _| {
                                        Dialog::new(
                                            id,
                                            me,
                                            &design::icons::WARNING,
                                            "Answer required",
                                            // わざと長い文。収まらない分は文の側だけがスクロールし、ボタンは残る。
                                            "This dialog stays open until you press OK or Cancel. \
                                             Pressing outside does nothing, so the parent stays dimmed \
                                             and cannot be used in the meantime. Use it for questions \
                                             that must be answered before going on, such as discarding \
                                             unsaved changes or overwriting a file. Keep the text short \
                                             in real dialogs: if it does not fit, only this part scrolls \
                                             while the buttons stay in place at the bottom.",
                                        )
                                    })
                                    .without_dismiss()
                                    .sized(DIALOG_SIZE, DIALOG_PX),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "dialog-answer".into())
                    .child(format!(
                        "Answered {} ({})",
                        self.answered,
                        self.answer.unwrap_or("-")
                    )),
            )
    }
}

/// [`DialogTest`] が開く、OK / Cancel だけのダイアログ（`components::dialog`）。押した答えを親へ渡して自分を閉じる。
/// `title` は種類の見分け用。
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
        let answer = |id: &'static str, label: &'static str, variant| {
            let parent = self.parent.clone();
            let close = command(self.panel, PanelCommand::Close);
            components::button(&theme, id, label, variant).on_click(move |e, w, cx| {
                parent
                    .update(cx, |t, cx| {
                        t.answered += 1;
                        t.answer = Some(label);
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
            self.title,
            self.description,
            [
                answer("dialog-ok", "OK", Variant::Primary),
                answer("dialog-cancel", "Cancel", Variant::Default),
            ],
        )
    }
}

/// 半透明の背景に描画される画面（`testing-transparent`）。右上の Close ボタンで自分を閉じる。
pub const TRANSPARENT_BG: u32 = 0x202020a0;

#[derive(Default)]
pub struct TestingTransparent {
    panel: PanelId,
}

impl TestingTransparent {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
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
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_start()
                    .child("testing-transparent")
                    .child(
                        button(&theme, "close-transparent", "Close", false)
                            .on_click(command(self.panel, PanelCommand::Close)),
                    ),
            )
    }
}

/// 閉じるボタン付きの見出し。
fn title_bar(
    theme: &Theme,
    panel: PanelId,
    title: impl Into<String>,
    close_id: &'static str,
) -> Div {
    div()
        .flex()
        .justify_between()
        .items_start()
        .child(title.into())
        .child(
            button(theme, close_id, "Close", false).on_click(command(panel, PanelCommand::Close)),
        )
}

/// ループアニメーションの周期（秒）。
const ANIM_PERIOD: f32 = 2.0;

/// 毎フレーム中身が変わる画面（`testing-anim`）: 描いた回数と、周期 [`ANIM_PERIOD`] で左右を往復し色の変わる四角。
/// 見えている間はフレームループが毎フレーム描き直す（[`ui::PanelState::live`]）。
pub struct TestingAnim {
    panel: PanelId,
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
            panel,
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
            .child(title_bar(&theme, self.panel, "testing-anim", "close-anim"))
            .child(
                div()
                    .debug_selector(|| "anim-frames".into())
                    .text_size(px(64.0))
                    .child(format!("frame {}", self.frames)),
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
            .child(title_bar(&theme, self.panel, "testing-lod", "close-lod"))
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
    panel: PanelId,
    pub vertical: ScrollHandle,
    pub horizontal: ScrollHandle,
    pub both: ScrollHandle,
    /// 縦の領域の中にある、入れ子のスクロール要素。
    pub inner: ScrollHandle,
}

impl TestingScrollbar {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
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
            .child(title_bar(
                &theme,
                self.panel,
                "testing-scrollbar",
                "close-scrollbar",
            ))
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
pub struct TestingFont {
    panel: PanelId,
}

impl TestingFont {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
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
            .child(title_bar(&theme, self.panel, "testing-font", "close-font"))
            .child(sample(
                "font-latin",
                24.0,
                FontWeight::NORMAL,
                "Noto Sans: The quick brown fox jumps over the lazy dog 0123456789",
            ))
            .child(sample(
                "font-latin-bold",
                24.0,
                FontWeight::BOLD,
                "Noto Sans Bold: Sphinx of black quartz, judge my vow",
            ))
            .child(sample(
                "font-ja",
                24.0,
                FontWeight::NORMAL,
                "Noto Sans JP: 吾輩は猫である。名前はまだ無い。",
            ))
            .child(sample(
                "font-ja-bold",
                24.0,
                FontWeight::BOLD,
                "Noto Sans JP 太字: いろはにほへと ちりぬるを",
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
    panel: PanelId,
    /// 出したパネル（閉じたものも含む）。ビルド時に積まれる。
    spawned: Rc<RefCell<Vec<PanelId>>>,
    rng: u64,
}

impl StressTest {
    pub fn new(panel: PanelId) -> Self {
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            panel,
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
            c.spawns
                .push(spec.in_view(grid_pose(i)).sized(STRESS_SIZE, (800, 600)));
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
            .child(title_bar(&theme, self.panel, "stress-test", "close-stress"))
            .child(
                div()
                    .debug_selector(|| "stress-count".into())
                    .child(format!("open {open} / limit {limit} (spawned {alive})")),
            )
            .child(
                row()
                    .child(
                        button(&theme, "stress-fill", "Fill to limit", false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(usize::MAX, cx))),
                    )
                    .child(
                        button(&theme, "stress-add", format!("+{STRESS_STEP}"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(STRESS_STEP, cx))),
                    )
                    .child(button(&theme, "stress-close", "Close all", false).on_click(
                        cx.listener(|this, _, _, cx| this.each(cx, |_| PanelCommand::Close)),
                    ))
                    .child(button(&theme, "stress-scatter", "Scatter", false).on_click(
                        cx.listener(|this, _, _, cx| {
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
                        }),
                    ))
                    .child(
                        button(&theme, "stress-grid", "Grid", false).on_click(cx.listener(
                            |this, _, _, cx| this.each(cx, |i| PanelCommand::Place(grid_pose(i))),
                        )),
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
        for id in ["spawn-anim", "spawn-lod", "spawn-font", "stress-test"] {
            let c = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(c, Modifiers::none());
        }
        let spawns = cx.update(|_, cx| cx.global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 4);
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
