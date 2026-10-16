//! キーボードは対象パネルへだけ入力を送る。GPUI / OS への転送は対象パネルが登録する。
use gpui::{
    App, AppContext, Context, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    PlatformInput, Window, prelude::*,
};
use kernel::{
    PanelCommand, PanelId, Vec2,
    subsystems::{
        keyboard::{Layout, Stroke},
        os::keyboard::{Output, Settings, Watcher, chord},
    },
};
use std::{collections::HashMap, rc::Rc};

mod view;
pub use view::{KeyAppearance, KeyboardView};

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
    watcher: Watcher,
    pub active: Option<Session>,
    generation: u64,
    pub error: Option<String>,
}
impl gpui::Global for InputRouting {}

pub fn system_keyboard(cx: &mut App) -> Settings {
    cx.default_global::<InputRouting>().watcher.settings()
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

/// OS を操作するパネル用の転送ヘルパー。キーボード自身はこれを呼ばない。
pub fn send_to_os(input: KeyboardInput, foreground: u64, cx: &mut App) -> Result<(), String> {
    let settings = system_keyboard(cx);
    let native = settings.layout.as_ref().is_some_and(|(name, variant)| {
        *name == input.layout.name && *variant == input.layout.variant
    });
    let stroke = input.stroke;
    let output = if !native
        && !stroke.shortcut()
        && !stroke.text.is_empty()
        && !stroke.text.chars().any(char::is_control)
    {
        Output::Text(stroke.text)
    } else if !native && stroke.composing {
        return Ok(());
    } else {
        Output::Keys(chord(&stroke.held, stroke.code))
    };
    cx.default_global::<InputRouting>()
        .watcher
        .send(foreground, output)
        .map_err(|e| e.to_string())
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

pub fn dismiss_keyboard(cx: &mut App) {
    if let Some(session) = cx.default_global::<InputRouting>().active.take() {
        if let Some(panel) = session.panel {
            cx.default_global::<crate::PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .push(PanelCommand::Close);
        }
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
    cx.default_global::<crate::PanelControl>().spawns.push(
        crate::PanelSpec::spawn(target, move |panel, _, cx| {
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
                keyboard: cx.new(|cx| {
                    let mut keyboard = KeyboardView::new(panel, cx);
                    keyboard.target = target;
                    keyboard.show_settings = false;
                    keyboard
                }),
            }
        })
        .sized(Vec2::new(1.52, 0.464), (1520, 464))
        .below(target, std::f32::consts::FRAC_PI_4)
        .locked(),
    );
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
    if routing.active.is_some_and(|s| s.panel == Some(panel)) {
        routing.active = None;
    } else if routing.active.is_some_and(|s| s.target == panel) {
        dismiss_keyboard(cx);
    }
}

struct KeyboardPanel {
    keyboard: gpui::Entity<KeyboardView>,
}
impl Render for KeyboardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        design::components::card_panel(&design::Theme::current(cx))
            .p(gpui::px(design::spacing::LG))
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
                    .all(|s| s.parent().is_none() && s.locked && s.below.is_some())
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
}
