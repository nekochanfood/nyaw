//! Layer 5 画面: Testing パネル。UI 層（`ui`）の上に載る画面の1つで、一連の機能を1画面で試せるテスト用の画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック数・HMD / 手 / トラッカーへの取り付け・取り外し・ロック・メニューボタンと掴みの表示
//! - GPUI: 切り替え・押している間の見た目・ドラッグ＆ドロップ・スクロールする一覧・フォーカスと文字入力・ポップアップメニュー
//! - Web: Testing の Web 版（[`WEB_PAGE`]。React のページ）を隣に開く
//! - ポインタの方式: [`PointerTest`] でマウス・タッチのエミュレートを切り替える
//!
//! 要素はすべて `debug_selector` で名前を付けてある（名前 = `id`）。

use gpui::{
    App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Stateful, Window, anchored, deferred, div, prelude::*, rgb, rgba,
};
use kernel::vr::{Device, Hand, Pose, Quat, Vec3};
use kernel::{PanelCommand, PanelId, PointerMode};
use ui::{PanelControl, PanelSpec};

/// 「HMD に取り付け」で使う相対姿勢: 顔の正面 1m。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」で使う相対姿勢: 0.3m 前（手首の時計のような配置）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// Testing の Web 版のページ名（`web/testing/`。`webpages::TESTING`）。
pub const WEB_PAGE: &str = "testing";

pub const PANEL_BG: u32 = 0x202020;
pub const BUTTON_BG: u32 = 0x3060c0;
/// レーザーが当たっている間のボタン色。VR 上でどこを指しているかの視覚的フィードバック。
pub const BUTTON_HOVER_BG: u32 = 0x4f86f7;
/// 有効になっている切り替えボタン・表示や、押している間のボタンの色。
pub const BUTTON_ACTIVE_BG: u32 = 0xc07030;
/// ドラッグ中の値を落とせる場所の上にいるときの色。
pub const DROP_TARGET_BG: u32 = 0x30a050;
/// フォーカスのある入力欄の色。
pub const FIELD_FOCUS_BG: u32 = 0x505080;
const AREA_BG: u32 = 0x303030;
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

/// 押せる四角いボタン。`active` は切り替えボタンが有効なとき。
fn button(id: &'static str, label: impl Into<String>, active: bool) -> Stateful<Div> {
    let bg = if active { BUTTON_ACTIVE_BG } else { BUTTON_BG };
    div()
        .id(id)
        .debug_selector(move || id.into())
        .p_2()
        .bg(rgb(bg))
        .hover(|s| s.bg(rgb(BUTTON_HOVER_BG)))
        .child(label.into())
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
            .child(button("spawn-panel", "Spawn panel", false).on_click(spawn(self.panel)))
            .child(
                button("spawn-transparent", "Spawn transparent", false)
                    .on_click(spawn_transparent(self.panel)),
            )
            .child(button("open-web", "Web Testing", false).on_click(spawn_web(self.panel)))
            .child(
                button("pointer-test", "Pointer test", false)
                    .on_click(spawn_pointer_test(self.panel)),
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
}
