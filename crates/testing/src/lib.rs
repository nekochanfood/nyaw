//! Layer 5 画面: Testing パネル。UI 層（`ui`）の上に載る画面の1つで、一連の機能を1画面で試せるテスト用の画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック数・HMD / 手 / トラッカーへの取り付け・取り外し・ロック・メニューボタンと掴みの表示
//! - GPUI: 切り替え・押している間の見た目・ドラッグ＆ドロップ・スクロールする一覧・フォーカスと文字入力・ポップアップメニュー
//! - Web: Testing の Web 版（[`WEB_PAGE`]。React のページ）を隣に開く
//! - ポインタの方式: [`PointerTest`] でマウス・タッチのエミュレートを切り替える
//! - 親子: [`DialogTest`] がダイアログ（[`Dialog`]）を子として開く。開いている間の親は暗くなり、子を掴むと親も付いてくる
//! - 描画: [`TestingAnim`]（毎フレーム変わる）・[`TestingLod`]（今の LOD を大きく出す）
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

/// Testing の Web 版のページ名（`web/testing/`。`webpages::TESTING`）。
pub const WEB_PAGE: &str = "testing";

/// 色は `design` のダークテーマの役割から引く（`color_at` は 0xRRGGBB なのでアルファを落とす）。
const THEME: Theme = Theme::of(Mode::Dark);
const fn rgb24(rgba: u32) -> u32 {
    rgba >> 8
}
pub const PANEL_BG: u32 = rgb24(THEME.panel);
pub const BUTTON_BG: u32 = rgb24(THEME.accent_solid);
/// レーザーが当たっている間のボタン色。VR 上でどこを指しているかの視覚的フィードバック。
pub const BUTTON_HOVER_BG: u32 = rgb24(THEME.accent_solid_hover);
/// 有効になっている切り替えボタン・表示や、押している間のボタンの色。
pub const BUTTON_ACTIVE_BG: u32 = rgb24(THEME.accent_solid_active);
/// ドラッグ中の値を落とせる場所の上にいるときの色。
pub const DROP_TARGET_BG: u32 = 0x30a050;
/// フォーカスのある入力欄の色。
pub const FIELD_FOCUS_BG: u32 = rgb24(THEME.accent_bg_active);
const AREA_BG: u32 = rgb24(THEME.neutral_bg);
/// ダイアログの背景色（親より明るくして重なりが見えるように）。
pub const DIALOG_BG: u32 = rgb24(THEME.neutral_bg_active);
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
fn button(id: &'static str, label: impl Into<String>, active: bool) -> Stateful<Div> {
    let b = components::button(&THEME, id, label.into(), Variant::Primary);
    if active {
        b.bg(rgba(THEME.accent_solid_active))
    } else {
        b
    }
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
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, |id, _, _| Testing::new(id)))
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

/// 隣に Testing の Web 版（Tauri の WebView）を開く。
fn spawn_web(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn_web(panel, WEB_PAGE))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.clone())
            .unwrap_or_default();
        let on = |d: &Device| st.attached.as_ref() == Some(d);
        let profiling = cx.try_global::<PanelControl>().is_some_and(|c| c.profiling);
        let tracker = st.trackers.first().map(|id| Device::Tracker(id.clone()));
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();

        let vr_row = row()
            .child(
                button("animate-panel", "Animation", st.animated)
                    .on_click(command(self.panel, PanelCommand::Animated(!st.animated))),
            )
            .child(button("spawn-panel", "Spawn panel", false).on_click(spawn(self.panel)))
            .child(
                button("spawn-transparent", "Spawn transparent", false)
                    .on_click(spawn_transparent(self.panel)),
            )
            .child(
                button("spawn-anim", "Spawn anim", false)
                    .on_click(spawn_view(self.panel, TestingAnim::new)),
            )
            .child(
                button("spawn-lod", "Spawn LOD", false)
                    .on_click(spawn_view(self.panel, |id, _| TestingLod::new(id))),
            )
            .child(
                button("stress-test", "Stress test", false)
                    .on_click(spawn_view(self.panel, |id, _| StressTest::new(id))),
            )
            .child(button("open-web", "Web Testing", false).on_click(spawn_web(self.panel)))
            .child(
                button("pointer-test", "Pointer test", false)
                    .on_click(spawn_pointer_test(self.panel)),
            )
            .child(
                button("dialog-test", "Dialog test", false).on_click(spawn_dialog_test(self.panel)),
            )
            .child(
                button("close-panel", "Close panel", false)
                    .on_click(command(self.panel, PanelCommand::Close)),
            )
            .child(
                button("toggle-profiler", "Profiler", profiling).on_click(|_, _, cx| {
                    let c = cx.default_global::<PanelControl>();
                    c.profiling = !c.profiling;
                }),
            )
            .child(
                button("click-me", format!("Clicked {}", self.clicks), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button("attach-hmd", "HMD", on(&Device::Hmd)).on_click(command(
                    self.panel,
                    PanelCommand::Attach(Device::Hmd, attach_offset(self.panel, HMD_OFFSET)),
                )),
            )
            .child(
                button(
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
                button(
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
                let b = button(
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
                button("detach", "Detach", false)
                    .on_click(command(self.panel, PanelCommand::Detach)),
            )
            .child(
                button("lock", "Lock", st.locked)
                    .on_click(command(self.panel, PanelCommand::Lock(!st.locked))),
            )
            .child(button("menu-held", "Menu", st.menu))
            .child(button("grabbed", "Grab", st.grabbed));

        let gpui_row = row()
            .child(
                button("toggle", "Toggle", self.toggled).on_click(cx.listener(|this, _, _, cx| {
                    this.toggled = !this.toggled;
                    cx.notify();
                })),
            )
            .child(button("hold-me", "Hold", false).active(|s| s.bg(rgb(BUTTON_ACTIVE_BG))))
            .child(
                button("drag-chip", "Drag me", false)
                    .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
            )
            .child(
                div()
                    .id("drop-zone")
                    .debug_selector(|| "drop-zone".into())
                    .p_2()
                    .bg(rgb(AREA_BG))
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
                        button(
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
                                    .bg(rgb(AREA_BG))
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.menu_open = false;
                                        cx.notify();
                                    }))
                                    .children(["menu-a", "menu-b"].map(|id| {
                                        button(id, id, self.picked == Some(id)).on_click(
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
            .bg(rgb(AREA_BG))
            .focus(|s| s.bg(rgb(FIELD_FOCUS_BG)))
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
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list)
            .bg(rgb(AREA_BG))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                this.scrolled += match e.delta {
                    ScrollDelta::Lines(p) => p.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / SCROLL_LINE_PX,
                };
                cx.notify();
            }))
            .children((0..LIST_ROWS).map(|i| {
                div()
                    .h(gpui::px(ROW_PX))
                    .px_2()
                    .child(format!("Row {i} (scrolled {:.1})", self.scrolled))
            }));

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(PANEL_BG))
            .text_color(rgb(0xffffff))
            .child(format!(
                "runtime: {:?} / attached: {:?} / locked: {}",
                st.runtime, st.attached, st.locked
            ))
            .child(vr_row)
            .child(gpui_row)
            .child(field)
            .child(list)
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
        let mode = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map(|c| c.status.pointer_mode)
            .unwrap_or_default();
        let (label, next) = match mode {
            PointerMode::Mouse => ("Mode: Mouse", PointerMode::Touch),
            PointerMode::Touch => ("Mode: Touch", PointerMode::Mouse),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(PANEL_BG))
            .text_color(rgb(0xffffff))
            .child(
                row()
                    .child(
                        button("pointer-mode", label, mode == PointerMode::Touch)
                            .on_click(command(self.panel, PanelCommand::PointerMode(next))),
                    )
                    .child(
                        button("tap-me", format!("Tapped {}", self.taps), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.taps += 1;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        button("close-pointer-test", "Close", false)
                            .on_click(command(self.panel, PanelCommand::Close)),
                    ),
            )
            .child(
                row()
                    .child(
                        button("drag-chip", "Drag me", false)
                            .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
                    )
                    .child(
                        div()
                            .id("drop-zone")
                            .debug_selector(|| "drop-zone".into())
                            .p_2()
                            .bg(rgb(AREA_BG))
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
                    .bg(rgb(AREA_BG))
                    .children((0..LIST_ROWS).map(|i| {
                        div()
                            .h(gpui::px(ROW_PX))
                            .px_2()
                            .child(format!("Swipe row {i}"))
                    })),
            )
    }
}

/// ダイアログの大きさ。
const DIALOG_SIZE: Vec2 = Vec2::new(0.4, 0.25);
const DIALOG_PX: (u32, u32) = (400, 250);

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
        let panel = self.panel;
        let me = cx.entity().downgrade();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(PANEL_BG))
            .text_color(rgb(0xffffff))
            .child(title_bar(panel, "dialog-test", "close-dialog-test"))
            .child(
                row()
                    .child({
                        let me = me.clone();
                        button("open-dialog", "Open dialog", false).on_click(move |_, _, cx| {
                            let me = me.clone();
                            cx.default_global::<PanelControl>().spawns.push(
                                PanelSpec::child(panel, true, move |id, _, _| {
                                    Dialog::new(id, me, "Click outside to dismiss")
                                })
                                .sized(DIALOG_SIZE, DIALOG_PX),
                            )
                        })
                    })
                    .child(
                        button("open-modal-dialog", "Open modal dialog", false).on_click(
                            move |_, _, cx| {
                                let me = me.clone();
                                cx.default_global::<PanelControl>().spawns.push(
                                    PanelSpec::child(panel, true, move |id, _, _| {
                                        Dialog::new(id, me, "Answer required")
                                    })
                                    .without_dismiss()
                                    .sized(DIALOG_SIZE, DIALOG_PX),
                                )
                            },
                        ),
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

/// [`DialogTest`] が開く、OK / Cancel だけのダイアログ。押した答えを親へ渡して自分を閉じる。`message` は種類の見分け用。
pub struct Dialog {
    panel: PanelId,
    parent: WeakEntity<DialogTest>,
    message: &'static str,
}

impl Dialog {
    pub fn new(panel: PanelId, parent: WeakEntity<DialogTest>, message: &'static str) -> Self {
        Self {
            panel,
            parent,
            message,
        }
    }
}

impl Render for Dialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let answer = |id: &'static str, label: &'static str| {
            let parent = self.parent.clone();
            let close = command(self.panel, PanelCommand::Close);
            button(id, label, false).on_click(move |e, w, cx| {
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .p_3()
            .bg(rgb(DIALOG_BG))
            .text_color(rgb(0xffffff))
            .child(self.message)
            .child(
                row()
                    .child(answer("dialog-ok", "OK"))
                    .child(answer("dialog-cancel", "Cancel")),
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_3()
            .bg(rgba(TRANSPARENT_BG))
            .text_color(rgb(0xffffff))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_start()
                    .child("testing-transparent")
                    .child(
                        button("close-transparent", "Close", false)
                            .on_click(command(self.panel, PanelCommand::Close)),
                    ),
            )
    }
}

/// 閉じるボタン付きの見出し。
fn title_bar(panel: PanelId, title: impl Into<String>, close_id: &'static str) -> Div {
    div()
        .flex()
        .justify_between()
        .items_start()
        .child(title.into())
        .child(button(close_id, "Close", false).on_click(command(panel, PanelCommand::Close)))
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let phase = self.start.elapsed().as_secs_f32() / ANIM_PERIOD % 1.0;
        // 0→1→0 と往復する。
        let x = 1.0 - (phase * 2.0 - 1.0).abs();
        div()
            .size_full()
            .relative()
            .p_3()
            .bg(rgb(PANEL_BG))
            .text_color(rgb(0xffffff))
            .child(title_bar(self.panel, "testing-anim", "close-anim"))
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
            .text_color(rgb(0xffffff))
            .child(title_bar(self.panel, "testing-lod", "close-lod"))
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
        let (open, limit) = cx
            .try_global::<PanelControl>()
            .map_or((0, 0), |c| (c.open, c.limit));
        let alive = self.alive(cx).len();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(PANEL_BG))
            .text_color(rgb(0xffffff))
            .child(title_bar(self.panel, "stress-test", "close-stress"))
            .child(
                div()
                    .debug_selector(|| "stress-count".into())
                    .child(format!("open {open} / limit {limit} (spawned {alive})")),
            )
            .child(
                row()
                    .child(
                        button("stress-fill", "Fill to limit", false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(usize::MAX, cx))),
                    )
                    .child(
                        button("stress-add", format!("+{STRESS_STEP}"), false)
                            .on_click(cx.listener(|this, _, _, cx| this.spawn(STRESS_STEP, cx))),
                    )
                    .child(button("stress-close", "Close all", false).on_click(
                        cx.listener(|this, _, _, cx| this.each(cx, |_| PanelCommand::Close)),
                    ))
                    .child(
                        button("stress-scatter", "Scatter", false).on_click(cx.listener(
                            |this, _, _, cx| {
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
                            },
                        )),
                    )
                    .child(button("stress-grid", "Grid", false).on_click(cx.listener(
                        |this, _, _, cx| this.each(cx, |i| PanelCommand::Place(grid_pose(i))),
                    ))),
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

    /// Web Testing ボタンは、隣に Testing の Web 版のページを開くよう頼む。
    #[gpui::test]
    fn open_web_button_spawns_web_page(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Testing::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("open-web").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        let pages: Vec<_> = cx.update(|_, cx| {
            cx.global::<PanelControl>()
                .spawns
                .iter()
                .map(|s| s.web_page().map(str::to_owned))
                .collect()
        });
        assert_eq!(pages, [Some(WEB_PAGE.to_owned())]);
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
        for id in ["spawn-anim", "spawn-lod", "stress-test"] {
            let c = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(c, Modifiers::none());
        }
        let spawns = cx.update(|_, cx| cx.global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 3);
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
