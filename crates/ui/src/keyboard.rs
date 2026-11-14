//! キーボードは対象パネルへだけ入力を送る。GPUI / OS への転送は対象パネルが登録する。
use gpui::{
    App, AppContext, Context, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    PlatformInput, Window, prelude::*,
};
use kernel::{
    PanelCommand, PanelId, Vec2,
    subsystems::{
        keyboard::{KeyboardType, Layout, Stroke},
        os::keyboard::Settings,
    },
};
use std::{collections::HashMap, rc::Rc};

mod view;
pub(crate) use view::preload;
pub use view::{KeyAppearance, KeyboardView};

/// アクション（閉じるなど、パネルの操作のボタン）の一辺。
const ACTION_SIZE: f32 = 48.;
/// フルサイズの F キーの段の右端（Pause の右端。1u）。ここからテンキーの右端までが空いている。
const FULL_FUNCTION_ROW_END_U: f32 = 18.5;

/// キーの上に設けるアクションの段（ボタン + キーとの間）の高さ。フルサイズはアクションを
/// テンキーの上の空きに置くので、段を設けない。
fn action_row(kind: KeyboardType) -> f32 {
    if kind == KeyboardType::Full {
        0.
    } else {
        ACTION_SIZE + design::spacing::SM
    }
}

/// アクションを右寄せで並べる場所。子に [`ACTION_SIZE`] 角のボタンを足していく。
/// フルサイズはテンキーの上の空き（F キーの段の右）にキーの段の高さで重ね、右端のボタンがテンキーの − の列にそろう。
/// ほかはキーの上の段。カードの子として、フルサイズはキーより後（上に重ねる）、ほかはキーより前に置く。
fn action_area(kind: KeyboardType) -> gpui::Div {
    let area = gpui::div()
        .flex()
        .flex_row()
        .justify_end()
        .items_center()
        .gap(gpui::px(design::spacing::SM))
        .flex_shrink_0();
    if action_row(kind) == 0. {
        let free = (KeyboardType::Full.width() - FULL_FUNCTION_ROW_END_U) * view::KEY_UNIT;
        area.absolute()
            .top(gpui::px(design::spacing::LG))
            .right(gpui::px(design::spacing::LG))
            .w(gpui::px(free))
            .h(gpui::px(view::KEY_UNIT))
            .px(gpui::px((view::KEY_UNIT - ACTION_SIZE) / 2.))
    } else {
        area.h(gpui::px(ACTION_SIZE))
    }
}

/// 種類（`kind`）のキーがちょうど収まるパネルの解像度。
fn panel_resolution(kind: KeyboardType) -> (u32, u32) {
    let inset = 2. * (design::components::PANEL_MARGIN + design::spacing::LG);
    (
        (kind.width() * view::KEY_UNIT + inset).ceil() as u32,
        (kind.height() * view::KEY_UNIT + inset + action_row(kind)).ceil() as u32,
    )
}

/// ホームポジション（F と J の間。G と H の境目）の、キーの並びの左端からの位置（1u。`geometry::keycaps` の AC 段）。
const HOME_U: f32 = 6.75;

/// パネル（幅 1m）の中心から見た、ホームポジションの横位置（m）。キーの並びはパネルの左の余白の内側から始まる。
fn home_offset(resolution: (u32, u32)) -> f32 {
    let left = design::components::PANEL_MARGIN + design::spacing::LG;
    (left + HOME_U * view::KEY_UNIT) / resolution.0 as f32 - 0.5
}

/// 掴むとパネルの中心ではなくホームポジションを手で動かして頭へ向ける（ホームポジションを中心に角度が付く）。
/// 位置はパネルのローカル座標（m）。パネルの幅は解像度に比例する（1m あたりのピクセル数が一定）。
fn grab_anchor(resolution: (u32, u32), width_m: f32) -> kernel::Pose {
    kernel::Pose::new(
        kernel::vr::Vec3::X * home_offset(resolution) * width_m,
        kernel::vr::Quat::IDENTITY,
    )
}

/// 全体の設定のキーボードの種類（設定が無ければ既定）。
fn keyboard_kind(cx: &App) -> KeyboardType {
    crate::settings(cx).keyboard
}

#[derive(Clone, Debug)]
pub struct KeyboardInput {
    pub stroke: Stroke,
    pub layout: Layout,
}
type Receiver = Rc<dyn Fn(KeyboardInput, &mut App) -> Result<(), String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardMode {
    Transient,
    Persistent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub target: PanelId,
    pub panel: Option<PanelId>,
    pub mode: KeyboardMode,
    generation: u64,
}

#[derive(Default)]
pub struct InputRouting {
    receivers: HashMap<PanelId, Receiver>,
    pub active: Option<Session>,
    generation: u64,
    pub error: Option<String>,
}
impl gpui::Global for InputRouting {}

/// OS の前面のアプリのキーボードの状態。フレームループがカーネル（[`kernel::Kernel::os_keyboard`]）から書き写す。
/// 最初に呼ぶとカーネルに監視を始めさせる（それまでは既定値）。
pub fn system_keyboard(cx: &mut App) -> Settings {
    let ctl = cx.default_global::<crate::PanelControl>();
    ctl.os_keyboard_wanted = true;
    ctl.os_keyboard.clone()
}

/// 対象パネルが、そのパネルに届いた入力の処理を登録する。
pub fn register_input(
    panel: PanelId,
    receive: impl Fn(KeyboardInput, &mut App) -> Result<(), String> + 'static,
    cx: &mut App,
) {
    cx.default_global::<InputRouting>()
        .receivers
        .insert(panel, Rc::new(receive));
}

pub fn unregister_input(panel: PanelId, cx: &mut App) {
    if cx.has_global::<InputRouting>() {
        cx.global_mut::<InputRouting>().receivers.remove(&panel);
    }
}

/// GPUI パネルの入力欄を選択したときの標準処理。入力先のフォーカスを保持して召喚する。
pub fn focus_input(panel: PanelId, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(focus, cx);
    let (handle, focus) = (window.window_handle(), focus.clone());
    register_input(
        panel,
        move |input, cx| {
            if input.stroke.composing {
                return Ok(());
            }
            cx.update_window(handle, |_, window, cx| {
                if window.focused(cx).as_ref() != Some(&focus) {
                    return Err("Input field no longer has focus".into());
                }
                let key = gpui_key(&input.stroke);
                window.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: key.clone(),
                        is_held: false,
                        prefer_character_input: !input.stroke.shortcut(),
                    }),
                    cx,
                );
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke: key }), cx);
                window.refresh();
                Ok(())
            })
            .map_err(|e| e.to_string())?
        },
        cx,
    );
    show_keyboard(panel, KeyboardMode::Transient, cx);
}

/// OS を操作するパネル（`panel`）の仮想キーボードを出し入れする（出したら `true`）。
/// 届いた入力はカーネルに頼んで OS の前面のアプリへ送る（[`PanelCommand::TypeToOs`]）。
pub fn toggle_os_keyboard(panel: PanelId, cx: &mut App) -> bool {
    register_os_input(panel, cx);
    let shown = toggle_keyboard(panel, cx);
    if !shown {
        unregister_input(panel, cx);
    }
    shown
}

/// `from` へ打つ OS キーボードが出ていれば、閉じずに `to`（OS を操作するパネル）へ打つものにする（譲ったら `true`）。
/// どちらも同じ OS キーボードなので、出し直さずに同じ場所のまま使い続けられる。
pub fn hand_over_os_keyboard(from: PanelId, to: PanelId, cx: &mut App) -> bool {
    let routing = cx.default_global::<InputRouting>();
    let Some(session) = routing
        .active
        .as_mut()
        .filter(|s| s.target == from && s.mode == KeyboardMode::Persistent)
    else {
        return false;
    };
    session.target = to;
    let keyboard = session.panel;
    routing.error = None;
    unregister_input(from, cx);
    register_os_input(to, cx);
    redraw_nav_bar(from, cx);
    redraw_nav_bar(to, cx);
    if let Some(keyboard) = keyboard {
        crate::redraw(keyboard, cx);
    }
    true
}

/// `panel` に届いた入力を、カーネルに頼んで OS の前面のアプリへ送る（[`PanelCommand::TypeToOs`]）。
fn register_os_input(panel: PanelId, cx: &mut App) {
    register_input(
        panel,
        move |input, cx| {
            crate::command(
                panel,
                PanelCommand::TypeToOs {
                    stroke: input.stroke,
                    layout: input.layout,
                },
                cx,
            );
            Ok(())
        },
        cx,
    );
}

/// ビューの借用が解けてから対象パネル一つに配送する。宛先を失っても他へ転送しない。
pub fn send_key(target: PanelId, input: KeyboardInput, cx: &mut App) {
    cx.defer(move |cx| {
        let closed = cx
            .try_global::<crate::PanelControl>()
            .and_then(|c| c.panels.get(&target))
            .is_some_and(|p| p.status.closed);
        let receive = cx
            .default_global::<InputRouting>()
            .receivers
            .get(&target)
            .cloned();
        let result = if closed {
            Err("Input panel is closed".into())
        } else if let Some(receive) = receive {
            receive(input, cx)
        } else {
            Err("Input panel is unavailable".into())
        };
        cx.default_global::<InputRouting>().error = result.err();
        crate::redraw(target, cx);
    });
}

/// `target` のナビゲーションバーを描き直させる（OS キーボードのトグルの色をキーボードの出入りに合わせる）。
fn redraw_nav_bar(target: PanelId, cx: &mut App) {
    let ctl = cx.default_global::<crate::PanelControl>();
    if let Some(&bar) = ctl.nav_bars.get(&target) {
        crate::redraw(bar, cx);
    }
}

pub fn dismiss_keyboard(cx: &mut App) {
    let session = cx.default_global::<InputRouting>().active.take();
    if let Some(session) = session {
        redraw_nav_bar(session.target, cx);
    }
    if let Some(session) = session
        && let Some(panel) = session.panel
    {
        crate::command(panel, PanelCommand::Close, cx);
    }
}

pub fn show_keyboard(target: PanelId, mode: KeyboardMode, cx: &mut App) {
    if cx
        .default_global::<InputRouting>()
        .active
        .is_some_and(|s| s.target == target && s.mode == mode)
    {
        return;
    }
    dismiss_keyboard(cx);
    let routing = cx.default_global::<InputRouting>();
    routing.generation += 1;
    let generation = routing.generation;
    routing.active = Some(Session {
        target,
        panel: None,
        mode,
        generation,
    });
    routing.error = None;
    redraw_nav_bar(target, cx);
    let kind = keyboard_kind(cx);
    let resolution = panel_resolution(kind);
    let home = home_offset(resolution);
    let spec = crate::PanelSpec::spawn(target, move |panel, _, cx| {
        let current = cx.default_global::<InputRouting>().active.as_mut();
        if let Some(session) = current.filter(|s| s.generation == generation) {
            session.panel = Some(panel);
        } else {
            crate::command(panel, PanelCommand::Close, cx);
        }
        KeyboardPanel {
            panel,
            kind,
            px_per_m: resolution.0 as f32,
            keyboard: cx.new(|cx| {
                let mut keyboard = KeyboardView::new(panel, cx);
                keyboard.target = target;
                keyboard.show_settings = false;
                keyboard
            }),
        }
    })
    .sized(
        Vec2::new(1., resolution.1 as f32 / resolution.0 as f32),
        resolution,
    )
    // 仮想キーボードは自前の X ボタン（`close-keyboard`）で閉じる。ナビゲーションバーは付けない。
    .without_nav_bar()
    // タッチの方式では離すまで押されず、押している間の手ブレでスワイプになって押し損じる。押した瞬間に押す。
    .pointer_mode(kernel::PointerMode::Mouse)
    // 開いた瞬間の手元の打ちやすい高さに、対象パネル（ダッシュボードにはめた画面も）と同じ向きで、
    // ホームポジションを対象の中心の左右に合わせて出す。親子関係は作らない（対象を動かしても付いて行かない）。
    .typing(home)
    .grab_anchor(grab_anchor(resolution, 1.));
    cx.default_global::<crate::PanelControl>().spawns.push(spec);
}

/// デスクトップパネルのトグルボタンから呼ぶ。フォーカスが移っても常駐する。
pub fn toggle_keyboard(target: PanelId, cx: &mut App) -> bool {
    if cx
        .default_global::<InputRouting>()
        .active
        .is_some_and(|s| s.target == target && s.mode == KeyboardMode::Persistent)
    {
        dismiss_keyboard(cx);
        false
    } else {
        show_keyboard(target, KeyboardMode::Persistent, cx);
        true
    }
}

pub fn keyboard_visible(target: PanelId, cx: &App) -> bool {
    cx.try_global::<InputRouting>()
        .and_then(|r| r.active)
        .is_some_and(|s| s.target == target)
}

/// ポインタで別の場所を選んだときだけ一時キーボードを閉じる。
pub fn pointer_selected(panel: Option<PanelId>, cx: &mut App) {
    if cx
        .try_global::<InputRouting>()
        .and_then(|r| r.active)
        .is_some_and(|s| s.mode == KeyboardMode::Transient && s.panel.is_some() && s.panel != panel)
    {
        dismiss_keyboard(cx);
    }
}

pub fn panel_closed(panel: PanelId, cx: &mut App) {
    if !cx.has_global::<InputRouting>() {
        return;
    }
    let routing = cx.global_mut::<InputRouting>();
    routing.receivers.remove(&panel);
    if let Some(session) = routing.active.filter(|s| s.panel == Some(panel)) {
        routing.active = None;
        redraw_nav_bar(session.target, cx);
    } else if routing.active.is_some_and(|s| s.target == panel) {
        dismiss_keyboard(cx);
    }
}

/// 呼び出し元が隠れたら（ダッシュボードごと隠したはめた画面など）、そこへ打つキーボードも閉じる。
/// キーボードは呼び出し元の子ではないので、一緒には隠れない。
pub fn panel_hidden(panel: PanelId, cx: &mut App) {
    if cx
        .try_global::<InputRouting>()
        .and_then(|r| r.active)
        .is_some_and(|s| s.target == panel)
    {
        dismiss_keyboard(cx);
    }
}

struct KeyboardPanel {
    panel: PanelId,
    /// パネルの大きさを合わせてある種類（アクションの置き場所もこれで決まる）。
    kind: KeyboardType,
    /// 1m あたりのピクセル数（幅 1m で出したときの横の解像度）。
    px_per_m: f32,
    keyboard: gpui::Entity<KeyboardView>,
}
impl Render for KeyboardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = design::Theme::current(cx);
        let panel = self.panel;
        // 譲られたら（[`hand_over_os_keyboard`]）打つ先を合わせる。
        let target = cx
            .try_global::<InputRouting>()
            .and_then(|r| r.active)
            .filter(|s| s.panel == Some(panel))
            .map(|s| s.target);
        if let Some(target) = target {
            self.keyboard.update(cx, |keyboard, cx| {
                if keyboard.target != target {
                    keyboard.target = target;
                    cx.notify();
                }
            });
        }
        // 種類が変わったら、キーがちょうど収まる大きさにパネルを変える（1m あたりのピクセル数を保つのでキーの大きさは同じ）。
        let kind = keyboard_kind(cx);
        if kind != self.kind {
            let to = panel_resolution(kind);
            self.kind = kind;
            crate::commands(
                panel,
                [
                    PanelCommand::Resize(to.0, to.1),
                    PanelCommand::GrabAnchor(Some(grab_anchor(to, to.0 as f32 / self.px_per_m))),
                ],
                cx,
            );
        }
        let close = design::components::icon_button(
            &theme,
            "close-keyboard",
            &design::icons::X,
            i18n::t(cx, "common.close"),
            design::components::Variant::Default,
        )
        .w(gpui::px(ACTION_SIZE))
        .h(gpui::px(ACTION_SIZE))
        .on_click(move |_, _, cx| {
            crate::command(panel, PanelCommand::Close, cx);
        });
        let actions = action_area(self.kind).child(close);
        let card = design::components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(gpui::px(design::spacing::SM))
            .p(gpui::px(design::spacing::LG));
        if action_row(self.kind) == 0. {
            card.child(self.keyboard.clone()).child(actions)
        } else {
            card.child(actions).child(self.keyboard.clone())
        }
    }
}

/// 見本: 召喚したキーボードと同じ地（カードと余白。閉じるボタンとその段は無い）に `keys`（[`KeyboardView::preview`]）を
/// 載せ、置かれた場所に収まるいちばん大きい縮尺（実寸まで）で、その中央に描く。場所の大きさは呼び出し側が決める。
/// 地は `keyboard-preview-card`。場所を測ってから組むので、種類や場所の幅が変わってもはみ出さない。
pub fn preview(keys: &gpui::Entity<KeyboardView>, cx: &App) -> impl IntoElement + use<> {
    let (theme, kind, keys) = (design::Theme::current(cx), keyboard_kind(cx), keys.clone());
    gpui::canvas(
        move |bounds, window, cx| {
            // 実寸の地（キー + 四方の余白）が場所に収まる倍率。場所が無ければ（開いた直後の窓など）描かない。
            let pad = design::spacing::LG;
            let fit = |room: gpui::Pixels, units: f32| {
                f32::from(room) / (units * view::KEY_UNIT + 2. * pad)
            };
            let (width, height) = (bounds.size.width, bounds.size.height);
            let k = fit(width, kind.width())
                .min(fit(height, kind.height()))
                .min(1.);
            if k.is_nan() || k <= 0. {
                return None;
            }
            let unit = k * view::KEY_UNIT;
            keys.update(cx, |keys, _| keys.unit = unit);
            let card = design::components::card(&theme)
                .debug_selector(|| "keyboard-preview-card".into())
                .flex_none()
                .rounded(gpui::px(design::radius::XL * k))
                .p(gpui::px(pad * k))
                .child(
                    gpui::div()
                        .w(gpui::px(kind.width() * unit))
                        .h(gpui::px(kind.height() * unit))
                        .child(keys),
                );
            let mut place = gpui::div()
                .w(width)
                .h(height)
                .flex()
                .items_center()
                .justify_center()
                .child(card)
                .into_any_element();
            place.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
            Some(place)
        },
        |_, place, window, cx| {
            if let Some(mut place) = place {
                place.paint(window, cx)
            }
        },
    )
    .size_full()
}

fn gpui_key(stroke: &Stroke) -> Keystroke {
    let symbol = stroke.symbols.first().and_then(|s| s.name()).unwrap_or("");
    let key = match symbol {
        "BackSpace" => "backspace".into(),
        "Return" | "KP_Enter" => "enter".into(),
        "Tab" | "ISO_Left_Tab" => "tab".into(),
        "Escape" => "escape".into(),
        "space" => "space".into(),
        "Delete" | "KP_Delete" => "delete".into(),
        "Prior" | "KP_Prior" => "pageup".into(),
        "Next" | "KP_Next" => "pagedown".into(),
        _ => stroke
            .symbols
            .first()
            .and_then(|s| s.char())
            .filter(|c| !c.is_control())
            .map(|c| c.to_string())
            .unwrap_or_else(|| symbol.trim_start_matches("KP_").to_lowercase()),
    };
    Keystroke {
        key,
        key_char: (!stroke.text.is_empty()).then(|| stroke.text.clone()),
        modifiers: Modifiers {
            control: stroke.control,
            alt: stroke.alt,
            shift: stroke.shift,
            platform: stroke.super_key,
            function: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::cell::RefCell;

    /// 呼び出し元が閉じても隠れても、キーボードは閉じる（子ではないので自分では消えない）。
    #[gpui::test]
    fn closing_or_hiding_the_caller_closes_its_keyboard(cx: &mut TestAppContext) {
        cx.update(|cx| {
            for gone in [panel_closed, panel_hidden] {
                cx.default_global::<InputRouting>().active = Some(Session {
                    target: 5,
                    panel: Some(9),
                    mode: KeyboardMode::Persistent,
                    generation: 1,
                });
                gone(5, cx);
                assert!(cx.global::<InputRouting>().active.is_none());
                let ctl = cx.global_mut::<crate::PanelControl>();
                let commands = &mut ctl.panels.get_mut(&9).unwrap().commands;
                assert_eq!(std::mem::take(commands), [PanelCommand::Close]);
            }
        });
    }

    fn input(text: &str) -> KeyboardInput {
        KeyboardInput {
            layout: Layout::new("us", ""),
            stroke: Stroke {
                code: 30,
                text: text.into(),
                ..Stroke::default()
            },
        }
    }

    /// OS キーボードのトグルの色を合わせるため、キーボードを出す・閉じるたびに持ち主のバー（だけ）を描き直させる。
    #[gpui::test]
    fn showing_and_dismissing_redraws_the_owners_nav_bar(cx: &mut TestAppContext) {
        let redraw = |cx: &mut App, id| {
            std::mem::take(
                &mut cx
                    .default_global::<crate::PanelControl>()
                    .panels
                    .entry(id)
                    .or_default()
                    .redraw,
            )
        };
        cx.update(|cx| {
            cx.default_global::<crate::PanelControl>()
                .nav_bars
                .insert(0, 5);
            assert!(toggle_keyboard(0, cx));
            assert!(redraw(cx, 5) && !redraw(cx, 0));
            assert!(!toggle_keyboard(0, cx));
            assert!(redraw(cx, 5));
        });
    }

    /// OS キーボードを譲ると、閉じずに打つ先だけが変わり、元のパネルへはもう届かない。
    #[gpui::test]
    fn handing_over_the_os_keyboard_keeps_it_open_for_the_new_target(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(toggle_os_keyboard(0, cx));
            let spawns = cx.global::<crate::PanelControl>().spawns.len();
            assert!(
                !hand_over_os_keyboard(7, 1, cx),
                "出ていない先からは譲らない"
            );
            assert!(hand_over_os_keyboard(0, 1, cx));
            let routing = cx.global::<InputRouting>();
            assert_eq!(routing.active.unwrap().target, 1);
            assert!(!routing.receivers.contains_key(&0) && routing.receivers.contains_key(&1));
            assert_eq!(cx.global::<crate::PanelControl>().spawns.len(), spawns);
            assert!(!toggle_os_keyboard(1, cx));
        });
    }

    #[gpui::test]
    fn one_panel_receives_each_input_and_closed_targets_never_fall_back(cx: &mut TestAppContext) {
        let first = Rc::new(RefCell::new(String::new()));
        let second = Rc::new(RefCell::new(String::new()));
        let (a, b) = (first.clone(), second.clone());
        cx.update(|cx| {
            register_input(
                0,
                move |input, _| {
                    a.borrow_mut().push_str(&input.stroke.text);
                    Ok(())
                },
                cx,
            );
            register_input(
                1,
                move |input, _| {
                    b.borrow_mut().push_str(&input.stroke.text);
                    Ok(())
                },
                cx,
            );
            send_key(0, input("a"), cx);
        });
        cx.run_until_parked();
        assert_eq!(&*first.borrow(), "a");
        assert!(second.borrow().is_empty());
        cx.update(|cx| send_key(1, input("b"), cx));
        cx.run_until_parked();
        assert_eq!(&*first.borrow(), "a");
        assert_eq!(&*second.borrow(), "b");
        cx.update(|cx| {
            cx.default_global::<crate::PanelControl>()
                .panels
                .entry(0)
                .or_default()
                .status
                .closed = true;
            send_key(0, input("discard"), cx);
        });
        cx.run_until_parked();
        assert_eq!(&*first.borrow(), "a");
        assert_eq!(&*second.borrow(), "b");
        cx.update(|cx| send_key(9, input("missing"), cx));
        cx.run_until_parked();
        assert_eq!(&*second.borrow(), "b");
    }

    #[gpui::test]
    fn persistent_and_transient_sessions_have_distinct_dismissal_rules(cx: &mut TestAppContext) {
        cx.update(|cx| {
            show_keyboard(0, KeyboardMode::Persistent, cx);
            cx.global_mut::<InputRouting>()
                .active
                .as_mut()
                .unwrap()
                .panel = Some(10);
            pointer_selected(Some(8), cx);
            assert_eq!(cx.global::<InputRouting>().active.unwrap().panel, Some(10));
            show_keyboard(1, KeyboardMode::Transient, cx);
            assert_eq!(
                cx.global::<crate::PanelControl>().panels[&10].commands,
                [PanelCommand::Close]
            );
            assert_eq!(cx.global::<InputRouting>().active.unwrap().target, 1);
            let specs = &cx.global::<crate::PanelControl>().spawns;
            assert_eq!(specs.len(), 2);
            assert!(
                specs
                    .iter()
                    .all(|s| s.parent().is_none() && !s.locked && s.typing.is_some())
            );
            cx.global_mut::<InputRouting>()
                .active
                .as_mut()
                .unwrap()
                .panel = Some(11);
            pointer_selected(Some(11), cx);
            assert!(cx.global::<InputRouting>().active.is_some());
            pointer_selected(Some(1), cx);
            assert!(cx.global::<InputRouting>().active.is_none());
            assert_eq!(
                cx.global::<crate::PanelControl>().panels[&11].commands,
                [PanelCommand::Close]
            );
            assert!(toggle_keyboard(0, cx));
            assert!(!toggle_keyboard(0, cx));
            assert!(cx.global::<InputRouting>().active.is_none());
        });
    }

    #[gpui::test]
    fn spawned_keyboard_is_in_front_of_the_head_with_home_centered_and_independent(
        cx: &mut TestAppContext,
    ) {
        let mut kernel = kernel::Kernel::with_backend(
            kernel::vr::mock::MockBackend::default(),
            kernel::Config::default(),
        );
        let target = kernel
            .add_panel(
                "target",
                kernel::Pose::new(
                    kernel::vr::Vec3::new(1., 2., -2.),
                    kernel::vr::Quat::IDENTITY,
                ),
                Vec2::new(1.2, 1.),
                (1200, 1000),
            )
            .unwrap();
        cx.update(|cx| {
            show_keyboard(target, KeyboardMode::Transient, cx);
            let spec = cx.global::<crate::PanelControl>().spawns.last().unwrap();
            let pose = spec.pose_for(&kernel, 1);
            // 対象パネル（右上）ではなく、頭の前の少し下に出る。
            let hmd = kernel.hmd();
            let local = hmd.leveled().inverse().transform_point(pose.position);
            assert!(local.y < -0.2 && local.z < -0.4, "{local}");
            // ホームポジションが対象の中心の左右に来る（頭の正面ではなく）。
            let home = pose.transform_point(kernel::vr::Vec3::X * home_offset(spec.resolution));
            let target_x = kernel.panel(target).pose.position.x;
            assert!((home.x - target_x).abs() < 1e-4, "{home}");
            // 左右の向きは対象と同じ（頭の方へ回り込まない）で、仰ぐ角度だけ頭へ向ける。
            let normal = pose.orientation * kernel::vr::Vec3::Z;
            assert!(normal.x.abs() < 1e-4 && normal.z > 0.5, "{normal}");
            assert!(spec.parent().is_none());
            let panel = kernel
                .add_panel("keyboard", pose, spec.size, spec.resolution)
                .unwrap();
            kernel.apply(target, PanelCommand::Place(kernel::Pose::IDENTITY));
            kernel.tick();
            assert_eq!(kernel.panel(panel).pose, pose);
            assert_eq!(kernel.panel(panel).parent, None);
        });
    }
    /// 種類を変えると、出ているキーボードのパネルがその種類の大きさになり、掴む点もホームポジションへ移る。
    #[gpui::test]
    fn changing_the_type_resizes_the_open_keyboard(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|window, cx| {
            show_keyboard(0, KeyboardMode::Persistent, cx);
            let spec = cx
                .default_global::<crate::PanelControl>()
                .spawns
                .pop()
                .unwrap();
            assert_eq!(spec.resolution, panel_resolution(KeyboardType::Full));
            crate::PanelRoot::new(spec.build(1, window, cx))
        });
        cx.run_until_parked();
        for kind in [KeyboardType::Compact, KeyboardType::Tenkeyless] {
            cx.update(|window, cx| {
                cx.global_mut::<crate::PanelControl>().settings.keyboard = kind;
                window.refresh();
            });
            cx.run_until_parked();
            let to = panel_resolution(kind);
            let width = to.0 as f32 / panel_resolution(KeyboardType::Full).0 as f32;
            let commands = cx.update(|_, cx| {
                std::mem::take(
                    &mut cx
                        .global_mut::<crate::PanelControl>()
                        .panels
                        .get_mut(&1)
                        .unwrap()
                        .commands,
                )
            });
            assert_eq!(
                commands,
                [
                    PanelCommand::Resize(to.0, to.1),
                    PanelCommand::GrabAnchor(Some(grab_anchor(to, width))),
                ]
            );
        }
    }

    /// テンキーの上の空きが無い種類では、アクションはキーの上の段に右寄せで並ぶ。
    /// テンキーの上の空きが無い種類では、アクションはキーの上の段に右寄せで並ぶ。
    #[gpui::test]
    fn actions_sit_in_a_row_above_the_keys_without_a_numpad(cx: &mut TestAppContext) {
        let db = kernel::subsystems::keyboard::Database::load().unwrap();
        for kind in [KeyboardType::Tenkeyless, KeyboardType::Compact] {
            let keyboard = db.keyboard(&Layout::new("us", "")).unwrap();
            let resolution = panel_resolution(kind);
            let (_, cx) = cx.add_window_view(|window, cx| {
                cx.default_global::<crate::PanelControl>().settings.keyboard = kind;
                dismiss_keyboard(cx);
                show_keyboard(0, KeyboardMode::Persistent, cx);
                let spec = cx
                    .default_global::<crate::PanelControl>()
                    .spawns
                    .pop()
                    .unwrap();
                assert_eq!(spec.resolution, resolution);
                let view = spec.build(1, window, cx);
                let panel = view.clone().downcast::<KeyboardPanel>().unwrap();
                panel.read(cx).keyboard.clone().update(cx, |view, _| {
                    view.keyboard = Some(keyboard);
                    view.loading = false;
                });
                crate::PanelRoot::new(view)
            });
            cx.simulate_resize(gpui::size(
                gpui::px(resolution.0 as f32),
                gpui::px(resolution.1 as f32),
            ));
            cx.run_until_parked();
            let inset = design::components::PANEL_MARGIN + design::spacing::LG;
            let close = cx.debug_bounds("close-keyboard").unwrap();
            assert_eq!(
                (f32::from(close.origin.y), f32::from(close.right())),
                (inset, resolution.0 as f32 - inset),
                "{kind:?}"
            );
            assert!(close.bottom() <= cx.debug_bounds("virtual-keyboard").unwrap().origin.y);
        }
    }

    #[gpui::test]
    fn summoned_keys_fit_the_card_and_close_button_closes_only_the_keyboard(
        cx: &mut TestAppContext,
    ) {
        let db = kernel::subsystems::keyboard::Database::load().unwrap();
        let keyboard = db.keyboard(&Layout::new("jp", "")).unwrap();
        let names: Vec<_> = keyboard
            .keycaps(kernel::subsystems::keyboard::KeyboardType::Full)
            .into_iter()
            .map(|k| k.name)
            .collect();
        let resolution = panel_resolution(KeyboardType::Full);
        let (_, cx) = cx.add_window_view(|window, cx| {
            show_keyboard(0, KeyboardMode::Persistent, cx);
            let spec = cx
                .default_global::<crate::PanelControl>()
                .spawns
                .pop()
                .unwrap();
            assert!(!spec.locked);
            let view = spec.build(1, window, cx);
            let panel = view.clone().downcast::<KeyboardPanel>().unwrap();
            let keys = panel.read(cx).keyboard.clone();
            keys.update(cx, |view, _| {
                view.keyboard = Some(keyboard);
                view.automatic = false;
                view.loading = false;
                view.error = Some("A long OS status must not displace any keys".into());
            });
            crate::PanelRoot::new(view)
        });
        cx.simulate_resize(gpui::size(
            gpui::px(resolution.0 as f32),
            gpui::px(resolution.1 as f32),
        ));
        cx.run_until_parked();
        assert!(cx.debug_bounds("keyboard-error").is_none());
        let keys = cx.debug_bounds("virtual-keyboard").unwrap();
        let inset = design::components::PANEL_MARGIN + design::spacing::LG;
        assert_eq!(f32::from(keys.origin.x), inset);
        assert_eq!(
            f32::from(keys.origin.y),
            inset + action_row(KeyboardType::Full)
        );
        assert!(f32::from(keys.right()) <= resolution.0 as f32 - inset);
        assert!(f32::from(keys.bottom()) <= resolution.1 as f32 - inset);
        for name in &names {
            let name = Box::leak(format!("key-{name}").into_boxed_str());
            let key = cx.debug_bounds(name).unwrap();
            assert!(key.origin.x >= keys.origin.x && key.origin.y >= keys.origin.y);
            assert!(
                key.right() <= keys.right() && key.bottom() <= keys.bottom(),
                "{name}"
            );
        }
        // フルサイズの X ボタンはキーの並びの右上（テンキーの上の空き）に収まり、どのキーにも重ならない。
        let close = cx.debug_bounds("close-keyboard").unwrap();
        assert!(close.top_right().x <= keys.right() && close.origin.y >= keys.origin.y);
        for name in &names {
            let name = Box::leak(format!("key-{name}").into_boxed_str());
            assert!(!cx.debug_bounds(name).unwrap().intersects(&close), "{name}");
        }
        let close = close.center();
        cx.simulate_click(close, gpui::Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                cx.global::<crate::PanelControl>().panels[&1].commands,
                [PanelCommand::Close]
            );
            assert!(!cx.global::<crate::PanelControl>().panels.contains_key(&0));
            panel_closed(1, cx);
            assert!(cx.global::<InputRouting>().active.is_none());
        });
    }
}
