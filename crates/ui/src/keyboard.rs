//! キーボードは対象パネルへだけ入力を送る。GPUI / OS への転送は対象パネルが登録する。
use gpui::{
    App, AppContext, Context, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    PlatformInput, Window, prelude::*,
};
use kernel::{
    PanelCommand, PanelId, Vec2,
    subsystems::{
        keyboard::{Layout, Stroke},
        os::keyboard::Settings,
    },
};
use std::{collections::HashMap, rc::Rc};

mod view;
pub use view::{KeyAppearance, KeyboardView};

const CLOSE_SIZE: f32 = 48.;

fn panel_resolution() -> (u32, u32) {
    let inset = 2. * (design::components::PANEL_MARGIN + design::spacing::LG);
    let kind = kernel::subsystems::keyboard::KeyboardType::Full;
    (
        (kind.width() * view::KEY_UNIT + inset).ceil() as u32,
        (kind.height() * view::KEY_UNIT + inset + CLOSE_SIZE + design::spacing::SM).ceil() as u32,
    )
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
    register_input(
        panel,
        move |input, cx| {
            cx.default_global::<crate::PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .push(PanelCommand::TypeToOs {
                    stroke: input.stroke,
                    layout: input.layout,
                });
            Ok(())
        },
        cx,
    );
    let shown = toggle_keyboard(panel, cx);
    if !shown {
        unregister_input(panel, cx);
    }
    shown
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
        cx.default_global::<crate::PanelControl>()
            .panels
            .entry(target)
            .or_default()
            .redraw = true;
    });
}

/// `target` のナビゲーションバーを描き直させる（OS キーボードのトグルの色をキーボードの出入りに合わせる）。
fn redraw_nav_bar(target: PanelId, cx: &mut App) {
    let ctl = cx.default_global::<crate::PanelControl>();
    if let Some(&bar) = ctl.nav_bars.get(&target) {
        ctl.panels.entry(bar).or_default().redraw = true;
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
        cx.default_global::<crate::PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .commands
            .push(PanelCommand::Close);
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
    let resolution = panel_resolution();
    // ダッシュボードにはめた画面へ打つなら、ダッシュボードの下に出して固定する（画面を外すとキーボードも外れる。`crate::undock`）。
    let dock = crate::docked(cx)
        .filter(|&d| d == target)
        .and(cx.global::<crate::PanelControl>().dock);
    let spec = crate::PanelSpec::spawn(target, move |panel, _, cx| {
            let current = cx.default_global::<InputRouting>().active.as_mut();
            if let Some(session) = current.filter(|s| s.generation == generation) {
                session.panel = Some(panel);
            } else {
                cx.default_global::<crate::PanelControl>()
                    .panels
                    .entry(panel)
                    .or_default()
                    .commands
                    .push(PanelCommand::Close);
            }
            KeyboardPanel {
                panel,
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
        .without_nav_bar();
    let spec = match dock {
        Some((owner, tilt)) => spec.below(owner, tilt).docked_to(owner),
        None => spec.below(target, std::f32::consts::FRAC_PI_4),
    };
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

struct KeyboardPanel {
    panel: PanelId,
    keyboard: gpui::Entity<KeyboardView>,
}
impl Render for KeyboardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = design::Theme::current(cx);
        let panel = self.panel;
        design::components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(gpui::px(design::spacing::SM))
            .p(gpui::px(design::spacing::LG))
            .child(
                gpui::div()
                    .flex()
                    .justify_end()
                    .h(gpui::px(CLOSE_SIZE))
                    .flex_shrink_0()
                    .child(
                        design::components::icon_button(
                            &theme,
                            "close-keyboard",
                            &design::icons::X,
                            i18n::t(cx, "common.close"),
                            design::components::Variant::Default,
                        )
                        .w(gpui::px(CLOSE_SIZE))
                        .h(gpui::px(CLOSE_SIZE))
                        .on_click(move |_, _, cx| {
                            cx.default_global::<crate::PanelControl>()
                                .panels
                                .entry(panel)
                                .or_default()
                                .commands
                                .push(PanelCommand::Close);
                        }),
                    ),
            )
            .child(self.keyboard.clone())
    }
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

    /// ダッシュボードから外した画面へ打つキーボードも、一緒に外れて独立する。
    #[gpui::test]
    fn undocking_a_panel_also_undocks_its_keyboard(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.default_global::<InputRouting>().active = Some(Session {
                target: 5,
                panel: Some(9),
                mode: KeyboardMode::Persistent,
                generation: 1,
            });
            crate::undock(5, cx);
            let ctl = cx.global::<crate::PanelControl>();
            for p in [5, 9] {
                assert_eq!(ctl.panels[&p].commands, [PanelCommand::Parent(None)]);
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
                    .all(|s| s.parent().is_none() && !s.locked && s.below.is_some())
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
    fn spawned_keyboard_is_below_tilted_and_independent(cx: &mut TestAppContext) {
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
            assert!(pose.position.y < kernel.panel(target).pose.position.y - 0.5);
            let normal = pose.orientation * kernel::vr::Vec3::Z;
            assert!(normal.y > 0.7 && normal.z > 0.7);
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
        let resolution = panel_resolution();
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
            inset + CLOSE_SIZE + design::spacing::SM
        );
        assert!(f32::from(keys.right()) <= resolution.0 as f32 - inset);
        assert!(f32::from(keys.bottom()) <= resolution.1 as f32 - inset);
        for name in names {
            let name = Box::leak(format!("key-{name}").into_boxed_str());
            let key = cx.debug_bounds(name).unwrap();
            assert!(key.origin.x >= keys.origin.x && key.origin.y >= keys.origin.y);
            assert!(
                key.right() <= keys.right() && key.bottom() <= keys.bottom(),
                "{name}"
            );
        }
        let close = cx.debug_bounds("close-keyboard").unwrap().center();
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
