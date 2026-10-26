use super::{InputRouting, KeyboardInput, send_key, system_keyboard};
use design::{
    Theme,
    components::{self, Variant},
    font_size, spacing,
};
use gpui::{App, Context, Window, div, prelude::*, px, rgba, uniform_list};
use kernel::{
    PanelId,
    subsystems::{
        keyboard::{Database, Keyboard, KeyboardType, Layout},
        os::keyboard::Settings,
    },
};
use std::{sync::mpsc, thread, time::Duration};

pub(super) const KEY_UNIT: f32 = 64.;

/// キーを押した音。押したキーの位置（パネルの中の点）から鳴らす。
const TAP_SOUND: &[u8] = include_bytes!("../../assets/keyboard/tap.wav");
const ENTER_SOUND: &[u8] = include_bytes!("../../assets/keyboard/enter.wav");
const BACKSPACE_SOUND: &[u8] = include_bytes!("../../assets/keyboard/backspace.wav");
const SPACEBAR_SOUND: &[u8] = include_bytes!("../../assets/keyboard/spacebar.wav");
/// 押すたびに音の高さを揺らす幅（速さの倍率 1 ± これ）。同じ音の繰り返しを機械的に聞こえさせない。
const PITCH_JITTER: f32 = 0.1;

/// XKB のキー名ごとの音。Enter・Backspace・Space だけ専用で、ほかは共通。
fn key_sound(name: &str) -> &'static [u8] {
    match name {
        "RTRN" | "KPEN" => ENTER_SOUND,
        "BKSP" => BACKSPACE_SOUND,
        "SPCE" => SPACEBAR_SOUND,
        _ => TAP_SOUND,
    }
}

/// 1 ± [`PITCH_JITTER`] の一様な乱数。乱数のクレートは使わず、標準のハッシュの乱数の鍵を使う。
fn jittered_pitch() -> f32 {
    use std::hash::{BuildHasher, Hasher};
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    let unit = (bits >> 40) as f32 / (1u64 << 24) as f32;
    1. + PITCH_JITTER * (2. * unit - 1.)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyAppearance {
    pub gap: f32,
    pub corner_radius: f32,
}
impl Default for KeyAppearance {
    fn default() -> Self {
        Self::for_type(KeyboardType::Full)
    }
}
impl KeyAppearance {
    pub fn for_type(kind: KeyboardType) -> Self {
        let (gap, corner_radius) = match kind {
            KeyboardType::Full => (
                design::keyboard::full::GAP,
                design::keyboard::full::CORNER_RADIUS,
            ),
            KeyboardType::Tenkeyless => (
                design::keyboard::tenkeyless::GAP,
                design::keyboard::tenkeyless::CORNER_RADIUS,
            ),
            KeyboardType::Compact => (
                design::keyboard::compact::GAP,
                design::keyboard::compact::CORNER_RADIUS,
            ),
        };
        Self { gap, corner_radius }
    }
    pub fn normalized(self) -> Self {
        Self {
            gap: if self.gap.is_finite() {
                self.gap.clamp(0., 16.)
            } else {
                Self::default().gap
            },
            corner_radius: if self.corner_radius.is_finite() {
                self.corner_radius.max(0.)
            } else {
                Self::default().corner_radius
            },
        }
    }
}

enum Loaded {
    Catalog(Vec<Layout>),
    Keyboard(u64, Result<Box<Keyboard>, String>),
    Error(String),
}

/// どの画面にも載せられるキーボード。配列の読み込みは専用スレッド、入力先の決定は UI に任せる。
pub struct KeyboardView {
    /// このキーボードが載っているパネル（キーの音を鳴らす）。
    panel: PanelId,
    pub target: PanelId,
    pub appearance: Option<KeyAppearance>,
    pub show_settings: bool,
    pub keyboard: Option<Keyboard>,
    pub layouts: Vec<Layout>,
    pub kind: KeyboardType,
    pub automatic: bool,
    pub loading: bool,
    pub error: Option<String>,
    settings: Settings,
    requested: Layout,
    generation: u64,
    request: mpsc::Sender<(u64, Layout)>,
    loaded: mpsc::Receiver<Loaded>,
    picker: bool,
    repeating: Option<(u16, KeyboardInput, u64)>,
    repeat_id: u64,
}

impl KeyboardView {
    pub fn new(panel: PanelId, cx: &mut Context<Self>) -> Self {
        let (request, receiver) = mpsc::channel::<(u64, Layout)>();
        let (sender, loaded) = mpsc::channel();
        thread::spawn(move || {
            let database = match Database::load() {
                Ok(db) => db,
                Err(e) => {
                    let _ = sender.send(Loaded::Error(e.to_string()));
                    return;
                }
            };
            if sender
                .send(Loaded::Catalog(database.layouts.clone()))
                .is_err()
            {
                return;
            }
            while let Ok(mut request) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    request = newer;
                }
                let result = database.keyboard(&request.1).map(Box::new);
                if sender.send(Loaded::Keyboard(request.0, result)).is_err() {
                    return;
                }
            }
        });
        let initial = Layout::new("us", "");
        let _ = request.send((0, initial.clone()));
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.poll(cx) {
                            cx.notify();
                            cx.default_global::<crate::PanelControl>()
                                .panels
                                .entry(panel)
                                .or_default()
                                .redraw = true;
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            panel,
            target: panel,
            appearance: None,
            show_settings: true,
            keyboard: None,
            layouts: Vec::new(),
            kind: KeyboardType::Full,
            automatic: true,
            loading: true,
            error: None,
            settings: Settings::default(),
            requested: initial,
            generation: 0,
            request,
            loaded,
            picker: false,
            repeating: None,
            repeat_id: 0,
        }
    }

    pub fn select_layout(&mut self, layout: Layout) {
        self.generation += 1;
        self.requested = layout.clone();
        self.loading = true;
        self.error = None;
        if self.request.send((self.generation, layout)).is_err() {
            self.error = Some("Keyboard worker stopped".into());
            self.loading = false;
        }
    }

    pub fn poll(&mut self, cx: &mut App) -> bool {
        let mut changed = false;
        while let Ok(loaded) = self.loaded.try_recv() {
            match loaded {
                Loaded::Catalog(layouts) => self.layouts = layouts,
                Loaded::Keyboard(generation, result) if generation == self.generation => {
                    self.loading = false;
                    match result {
                        Ok(mut keyboard) => {
                            if self.automatic {
                                keyboard
                                    .sync_locks(self.settings.caps_lock, self.settings.num_lock);
                            }
                            self.keyboard = Some(*keyboard);
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Loaded::Error(error) => {
                    self.error = Some(error);
                    self.loading = false;
                }
                _ => continue,
            }
            changed = true;
        }
        let settings = system_keyboard(cx);
        if self.settings != settings {
            self.settings = settings;
            if self.automatic {
                self.follow_system();
                if let Some(keyboard) = &mut self.keyboard {
                    keyboard.sync_locks(self.settings.caps_lock, self.settings.num_lock);
                }
            }
            changed = true;
        }
        changed
    }

    fn follow_system(&mut self) {
        let layout = self
            .settings
            .layout
            .as_ref()
            .map(|(name, variant)| Layout::new(name, variant))
            .or_else(|| {
                self.settings
                    .layout_description
                    .as_ref()
                    .and_then(|description| {
                        self.layouts
                            .iter()
                            .find(|l| l.description == *description)
                            .cloned()
                    })
            });
        if let Some(layout) = layout
            && (layout.name != self.requested.name || layout.variant != self.requested.variant)
        {
            self.select_layout(layout);
        }
    }

    /// 配列が読めていて、キーを受け付けられる。
    fn ready(&self) -> bool {
        !self.loading && self.error.is_none() && !(self.automatic && self.settings.layout.is_none())
    }

    pub fn press(&mut self, code: u16, cx: &mut App) -> Option<KeyboardInput> {
        if !self.ready() {
            return None;
        }
        if let Some(keyboard) = &mut self.keyboard
            && let Some(stroke) = keyboard.click(code)
        {
            let input = KeyboardInput {
                stroke,
                layout: keyboard.layout.clone(),
            };
            send_key(self.target, input.clone(), cx);
            return Some(input);
        }
        None
    }

    /// 押していたキーを離した（離した所がキーの外でも）。キーリピートを止める。
    fn release(&mut self, code: u16) {
        if self
            .repeating
            .as_ref()
            .is_some_and(|(key, _, _)| *key == code)
        {
            self.repeating = None;
        }
    }

    fn press_down(
        &mut self,
        code: u16,
        sound: &'static [u8],
        at: kernel::Vec2,
        cx: &mut Context<Self>,
    ) {
        let toggle = self.keyboard.as_ref().is_some_and(|k| k.is_toggle(code));
        let ready = self.ready();
        let input = self.press(code, cx);
        // Ctrl・Shift などのトグルは文字を生まない（押した状態になるだけ）が、押したことは音で返す。
        if input.is_some() || (toggle && ready) {
            cx.default_global::<crate::PanelControl>()
                .panels
                .entry(self.panel)
                .or_default()
                .commands
                .push(kernel::PanelCommand::PlayAt {
                    sound: kernel::audio::Sound::Wav(sound),
                    at,
                    pitch: jittered_pitch(),
                });
        }
        cx.notify();
        let Some(input) = input.filter(|i| !toggle && !i.stroke.shortcut() && !i.stroke.composing)
        else {
            return;
        };
        self.repeat_id = self.repeat_id.wrapping_add(1);
        let repeat_id = self.repeat_id;
        self.repeating = Some((code, input, repeat_id));
        cx.spawn(async move |this, cx| {
            let mut delay = Duration::from_millis(500);
            loop {
                cx.background_executor().timer(delay).await;
                if !this
                    .update(cx, |this, cx| {
                        let Some((_, input, _)) = this
                            .repeating
                            .as_ref()
                            .filter(|(key, _, id)| *key == code && *id == repeat_id)
                        else {
                            return false;
                        };
                        send_key(this.target, input.clone(), cx);
                        true
                    })
                    .unwrap_or(false)
                {
                    break;
                }
                delay = Duration::from_millis(50);
            }
        })
        .detach();
    }
}

impl Render for KeyboardView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let selected = self
            .layouts
            .iter()
            .find(|l| l.name == self.requested.name && l.variant == self.requested.variant)
            .map(|l| l.description.clone())
            .unwrap_or_else(|| self.requested.description.clone());
        let controls = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(spacing::SM))
            .child(
                components::button(
                    &theme,
                    "keyboard-auto",
                    i18n::t(cx, "keyboard.follow"),
                    Variant::Default,
                )
                .selected(self.automatic)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.automatic = !this.automatic;
                    if this.automatic {
                        this.follow_system();
                    }
                    cx.notify();
                })),
            )
            .child(
                components::button(&theme, "keyboard-layout", selected, Variant::Default)
                    .selected(self.picker)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.picker = !this.picker;
                        cx.notify();
                    })),
            )
            .children(KeyboardType::ALL.map(|kind| {
                let (id, label) = match kind {
                    KeyboardType::Full => ("keyboard-full", "keyboard.full"),
                    KeyboardType::Tenkeyless => ("keyboard-tkl", "keyboard.tkl"),
                    KeyboardType::Compact => ("keyboard-compact", "keyboard.compact"),
                };
                components::button(&theme, id, i18n::t(cx, label), Variant::Default)
                    .selected(self.kind == kind)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.kind = kind;
                        if let Some(keyboard) = &mut this.keyboard {
                            keyboard.reset();
                        }
                        cx.notify();
                    }))
            }))
            .child(
                components::button(
                    &theme,
                    "keyboard-reset",
                    i18n::t(cx, "keyboard.reset"),
                    Variant::Default,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(keyboard) = &mut this.keyboard {
                        keyboard.reset();
                    }
                    cx.notify();
                })),
            );
        let target = i18n::tf(cx, "keyboard.target", &[("panel", &self.target)]);
        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .when(self.show_settings, |root| root.child(controls))
            .when(self.show_settings, |root| {
                root.child(
                    div()
                        .debug_selector(|| "keyboard-target".into())
                        .child(target),
                )
            })
            .when(self.show_settings, |root| {
                root.child(
                    div()
                        .debug_selector(|| "keyboard-ime".into())
                        .text_size(px(font_size::SM))
                        .child(self.settings.input_method.clone()),
                )
            });
        if self.picker {
            root = root.child(
                uniform_list(
                    "keyboard-layout-list",
                    self.layouts.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let theme = Theme::current(cx);
                        range
                            .map(|index| {
                                let layout = this.layouts[index].clone();
                                let id = format!("layout-{}-{}", layout.name, layout.variant);
                                let label = format!(
                                    "{} · {}{}",
                                    layout.description,
                                    layout.name,
                                    if layout.variant.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" / {}", layout.variant)
                                    }
                                );
                                div().h(px(62.)).w_full().child(
                                    components::button(&theme, id, label, Variant::Default)
                                        .w_full()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.automatic = false;
                                            this.picker = false;
                                            this.select_layout(layout.clone());
                                            cx.notify();
                                        })),
                                )
                            })
                            .collect()
                    }),
                )
                .h(px(248.))
                .w_full(),
            );
        }
        if self.show_settings && self.loading {
            root = root.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .child(components::spinner(&theme, 16.0))
                    .child(i18n::t(cx, "keyboard.loading")),
            );
        }
        let error = self
            .error
            .clone()
            .or_else(|| self.settings.error.clone())
            .or_else(|| cx.default_global::<InputRouting>().error.clone());
        if let Some(error) = error.filter(|_| self.show_settings) {
            root = root.child(
                div()
                    .debug_selector(|| "keyboard-error".into())
                    .child(error),
            );
        }
        if self.show_settings && self.automatic && self.settings.layout.is_none() {
            root = root.child(div().child(i18n::t(cx, "keyboard.unmatched")));
        }
        if let Some(keyboard) = &self.keyboard {
            let unit = KEY_UNIT;
            let appearance = self
                .appearance
                .unwrap_or_else(|| KeyAppearance::for_type(self.kind))
                .normalized();
            let gap = appearance.gap;
            let keys = keyboard.keycaps(self.kind);
            let canvas = div()
                .id("virtual-keyboard")
                .debug_selector(|| "virtual-keyboard".into())
                .relative()
                .flex_shrink_0()
                .w(px(self.kind.width() * unit))
                .h(px(self.kind.height() * unit))
                .children(keys.into_iter().flat_map(|key| {
                    let code = key.code;
                    let sound = key_sound(&key.name);
                    let label = keyboard.label(code);
                    let selector = format!("legend-{}-{label}", key.name);
                    let mut content = div()
                        .relative()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(2.))
                        .text_size(px(if label.chars().count() > 5 {
                            font_size::SM
                        } else {
                            font_size::MD
                        }));
                    let glyph = match keyboard.symbol_name(code) {
                        Some("Muhenkan") => Some((&design::icons::PROHIBIT, "keyboard.noconvert")),
                        Some("Henkan_Mode") => {
                            Some((&design::icons::ARROWS_CLOCKWISE, "keyboard.convert"))
                        }
                        _ => None,
                    };
                    if let Some((glyph, _)) = glyph {
                        content = content.child(
                            design::icon(glyph)
                                .weight(design::Weight::Bold)
                                .size(px(24.)),
                        );
                    } else {
                        content =
                            content.child(div().debug_selector(move || selector).child(label));
                    }
                    if let Some(alphabet) = keyboard.alphabet_label(code) {
                        let selector = format!("alphabet-{}-{alphabet}", key.name);
                        content = content.child(
                            div()
                                .debug_selector(move || selector)
                                .text_size(px(font_size::SM))
                                .text_color(rgba(theme.neutral_text_low))
                                .child(alphabet),
                        );
                    }
                    if key.shape == kernel::subsystems::keyboard::KeyShape::SteppedEnter {
                        content = content.ml(px(16.));
                    }
                    let button = components::shaped_button(
                        &theme,
                        format!("key-{}", key.name),
                        key.outline(unit, gap),
                        appearance.corner_radius,
                    )
                    .content(content)
                    .selected(keyboard.is_on(code))
                    .absolute()
                    .left(px(key.x * unit + gap / 2.))
                    .top(px(key.y * unit + gap / 2.))
                    .w(px(key.width * unit - gap))
                    .h(px(key.height * unit - gap));
                    let button = if let Some((_, tooltip)) = glyph {
                        button.tooltip(i18n::t(cx, tooltip))
                    } else {
                        button
                    };
                    // 押す判定はキーの見た目（隙間と角丸を除く）ではなく、隙間の半分まで含めたキーの升目全体で取る。
                    // レーザーは手ブレするので、隙間や角に当たって押し損じないように。
                    // ホバー音も升目で鳴らす（升目は隙間なく並ぶので、隙間のそばの手ブレで鳴り直さない）。
                    let hover: gpui::SharedString = format!("key-{}", key.name).into();
                    let cells = key.regions().into_iter().map(|(x, y, w, h)| {
                        let hover = hover.clone();
                        div()
                            .absolute()
                            .left(px(x * unit))
                            .top(px(y * unit))
                            .w(px(w * unit))
                            .h(px(h * unit))
                            .on_mouse_move(move |_, window, cx| {
                                design::cue::hover(&hover, window, cx)
                            })
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    let at = event.position;
                                    this.press_down(
                                        code,
                                        sound,
                                        kernel::Vec2::new(f32::from(at.x), f32::from(at.y)),
                                        cx,
                                    );
                                }),
                            )
                            .on_mouse_up(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _, _, _| this.release(code)),
                            )
                            .on_mouse_up_out(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _, _, _| this.release(code)),
                            )
                            .into_any_element()
                    });
                    std::iter::once(button.into_any_element())
                        .chain(cells.collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                }));
            root = root.child(
                div()
                    .id("keyboard-scroll")
                    .flex_shrink_0()
                    .h(px(self.kind.height() * unit))
                    .overflow_x_scroll()
                    .w_full()
                    .child(canvas),
            );
        }
        root.text_color(rgba(theme.neutral_text))
            .when(self.show_settings, |root| {
                root.child(
                    div()
                        .text_size(px(font_size::SM))
                        .child(i18n::t(cx, "keyboard.toggle_hint")),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{cell::Cell, rc::Rc};

    fn ready(db: &Database, layout: &str, kind: KeyboardType) -> KeyboardView {
        let layout = Layout::new(layout, "");
        let (request, _) = mpsc::channel();
        let (_, loaded) = mpsc::channel();
        KeyboardView {
            panel: 0,
            target: 0,
            appearance: None,
            show_settings: true,
            keyboard: Some(db.keyboard(&layout).unwrap()),
            layouts: db.layouts.clone(),
            kind,
            automatic: false,
            loading: false,
            error: None,
            settings: Settings::default(),
            requested: layout,
            generation: 0,
            request,
            loaded,
            picker: false,
            repeating: None,
            repeat_id: 0,
        }
    }

    #[gpui::test]
    fn rendered_ansi_iso_and_jis_keys_have_the_expected_bounds(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        for layout in ["us", "de", "jp"] {
            for kind in KeyboardType::ALL {
                let view = ready(&db, layout, kind);
                let (_, cx) = cx.add_window_view(|_, _| view);
                cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
                cx.run_until_parked();
                let origin = cx.debug_bounds("virtual-keyboard").unwrap().origin;
                let y = if kind == KeyboardType::Compact {
                    0.
                } else {
                    96.
                };
                let expected = [
                    ("key-TLDE", 0., y, 56., 56.),
                    ("key-AC01", 112., y + 128., 56., 56.),
                    (
                        "key-SPCE",
                        if layout == "jp" { 304. } else { 240. },
                        y + 256.,
                        if layout == "jp" { 216. } else { 392. },
                        56.,
                    ),
                    (
                        "key-RTRN",
                        if layout == "us" { 816. } else { 864. },
                        y + if layout == "us" { 128. } else { 64. },
                        if layout == "us" { 136. } else { 88. },
                        if layout == "us" { 56. } else { 120. },
                    ),
                    (
                        "key-LFSH",
                        0.,
                        y + 192.,
                        if layout == "de" { 72. } else { 136. },
                        56.,
                    ),
                ];
                for (name, x, y, width, height) in expected {
                    let bounds = cx.debug_bounds(name).unwrap();
                    assert_eq!(
                        (
                            f32::from(bounds.origin.x - origin.x),
                            f32::from(bounds.origin.y - origin.y),
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height)
                        ),
                        (x + 4., y + 4., width, height),
                        "{layout}: {kind:?}: {name}"
                    );
                }
                assert_eq!(
                    cx.debug_bounds("key-KP7").is_some(),
                    kind == KeyboardType::Full
                );
                assert_eq!(
                    cx.debug_bounds("key-FK01").is_some(),
                    kind != KeyboardType::Compact
                );
                assert_eq!(cx.debug_bounds("key-LSGT").is_some(), layout == "de");
                assert_eq!(cx.debug_bounds("key-HENK").is_some(), layout == "jp");
            }
        }
    }

    #[test]
    fn key_sounds_vary_in_pitch_within_the_jitter() {
        let pitches: Vec<f32> = (0..64).map(|_| jittered_pitch()).collect();
        assert!(pitches.iter().all(|p| (p - 1.).abs() <= PITCH_JITTER));
        assert!(pitches.iter().any(|p| *p != pitches[0]));
        assert_eq!(key_sound("SPCE"), SPACEBAR_SOUND);
        assert_eq!(key_sound("AC01"), TAP_SOUND);
    }

    /// キーを押すと、載っているパネルにそのキーの点から鳴らす要求を積む。
    #[gpui::test]
    fn pressing_a_key_plays_its_sound_from_the_key(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        let view = ready(&db, "us", KeyboardType::Compact);
        let (_, cx) = cx.add_window_view(|_, _| view);
        cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
        cx.run_until_parked();
        let key = cx.debug_bounds("key-SPCE").unwrap().center();
        cx.simulate_click(key, gpui::Modifiers::none());
        let commands = cx.update(|_, cx| {
            cx.global::<crate::PanelControl>().panels[&0]
                .commands
                .clone()
        });
        let Some(kernel::PanelCommand::PlayAt { sound, at, pitch }) = commands.first() else {
            panic!("{commands:?}");
        };
        assert_eq!(*sound, kernel::audio::Sound::Wav(SPACEBAR_SOUND));
        assert_eq!(*at, kernel::Vec2::new(f32::from(key.x), f32::from(key.y)));
        assert!((pitch - 1.).abs() <= PITCH_JITTER);
    }

    /// キーに入るとホバー音を要求する。同じキーの隙間へ手ブレしても鳴り直さず、隣のキーへ移ると鳴る。
    #[gpui::test]
    fn hovering_a_key_requests_the_hover_sound_once_per_key(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        let view = ready(&db, "us", KeyboardType::Compact);
        let (_, cx) = cx.add_window_view(|_, _| view);
        cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
        cx.run_until_parked();
        let a = cx.debug_bounds("key-AC01").unwrap();
        let s = cx.debug_bounds("key-AC02").unwrap();
        let mut hovers = |at| {
            cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
            design::drive::advance(Duration::from_millis(100));
            cx.update(|window, cx| design::cue::take(window, cx))
                .into_iter()
                .filter(|(cue, _)| *cue == design::cue::Cue::Hover)
                .count()
        };
        assert_eq!(hovers(a.center()), 1);
        assert_eq!(hovers(a.top_right() + gpui::point(px(2.), px(4.))), 0);
        assert_eq!(hovers(a.center()), 0);
        assert_eq!(hovers(s.center()), 1);
    }

    /// キーの見た目の外でも、隙間（の半分まで）や角丸の外に当たればそのキーを押す（レーザーの手ブレで押し損じない）。
    #[gpui::test]
    fn pressing_the_gap_or_corner_next_to_a_key_presses_it(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        let view = ready(&db, "us", KeyboardType::Compact);
        let (_, cx) = cx.add_window_view(|_, _| view);
        cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
        cx.run_until_parked();
        let key = cx.debug_bounds("key-SPCE").unwrap();
        for at in [
            key.bottom_left() + gpui::point(key.size.width / 2., px(2.)),
            key.origin + gpui::point(px(1.), px(1.)),
        ] {
            cx.simulate_click(at, gpui::Modifiers::none());
            let commands = cx.update(|_, cx| {
                let ctl = cx.global_mut::<crate::PanelControl>();
                std::mem::take(&mut ctl.panels.get_mut(&0).unwrap().commands)
            });
            assert!(
                matches!(
                    commands.first(),
                    Some(kernel::PanelCommand::PlayAt { sound, .. })
                        if *sound == kernel::audio::Sound::Wav(SPACEBAR_SOUND)
                ),
                "{at:?}: {commands:?}"
            );
        }
    }

    #[test]
    fn os_layout_and_ime_changes_request_the_corresponding_variant() {
        let db = Database::load().unwrap();
        let mut view = ready(&db, "us", KeyboardType::Full);
        let (request, requests) = mpsc::channel();
        view.request = request;
        view.settings.layout = Some(("jp".into(), "kana".into()));
        view.follow_system();
        let (_, layout) = requests.try_recv().unwrap();
        assert_eq!(
            (layout.name.as_str(), layout.variant.as_str()),
            ("jp", "kana")
        );
        assert!(view.loading);
        view.follow_system();
        assert!(requests.try_recv().is_err());
        view.settings.layout = Some(("us".into(), "dvorak".into()));
        view.follow_system();
        let (_, layout) = requests.try_recv().unwrap();
        assert_eq!(
            (layout.name.as_str(), layout.variant.as_str()),
            ("us", "dvorak")
        );
    }

    #[gpui::test]
    fn shift_updates_rendered_letters_symbols_and_kana_legends(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        for layout in ["us", "de", "jp"] {
            let mut view = ready(&db, layout, KeyboardType::Compact);
            if layout == "jp" {
                view.keyboard = Some(db.keyboard(&Layout::new("jp", "kana")).unwrap());
            }
            let (_, cx) = cx.add_window_view(|_, _| view);
            cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
            cx.run_until_parked();
            if layout == "jp" {
                assert!(cx.debug_bounds("legend-AD03-イ").is_some());
                assert!(cx.debug_bounds("alphabet-AD03-e").is_some());
            } else {
                assert!(cx.debug_bounds("legend-AC01-a").is_some());
            }
            let at = cx.debug_bounds("key-LFSH").unwrap().center();
            cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
            cx.simulate_mouse_up(at, gpui::MouseButton::Left, gpui::Modifiers::none());
            cx.run_until_parked();
            if layout == "jp" {
                assert!(cx.debug_bounds("legend-AD03-ィ").is_some());
                assert!(cx.debug_bounds("alphabet-AD03-E").is_some());
            } else {
                assert!(cx.debug_bounds("legend-AC01-A").is_some());
                assert!(cx.debug_bounds("legend-AE01-!").is_some());
                assert!(
                    cx.debug_bounds(if layout == "us" {
                        "legend-AE02-@"
                    } else {
                        "legend-AE02-\""
                    })
                    .is_some()
                );
            }
        }
    }

    #[gpui::test]
    fn holding_a_letter_starts_and_releasing_it_stops_repeat(cx: &mut TestAppContext) {
        let db = Database::load().unwrap();
        let (view, cx) = cx.add_window_view(|_, _| ready(&db, "us", KeyboardType::Compact));
        let sent = Rc::new(Cell::new(0));
        let count = sent.clone();
        cx.update(|_, cx| {
            crate::keyboard::register_input(
                0,
                move |_, _| {
                    count.set(count.get() + 1);
                    Ok(())
                },
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(1700.), px(1000.)));
        cx.run_until_parked();
        let at = cx.debug_bounds("key-AC01").unwrap().center();
        cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
        assert_eq!(
            view.read_with(cx, |view, _| view
                .repeating
                .as_ref()
                .map(|(_, input, _)| input.stroke.text.clone())),
            Some("a".into())
        );
        cx.run_until_parked();
        assert_eq!(sent.get(), 1);
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.simulate_mouse_up(at, gpui::MouseButton::Left, gpui::Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.repeating.is_none()));
        cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.run_until_parked();
        assert_eq!(sent.get(), 2);
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(sent.get(), 2, "the previous hold must not repeat");
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        assert_eq!(sent.get(), 3, "the new hold should repeat");
        cx.simulate_mouse_up(
            at + gpui::point(px(100.), px(0.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        assert!(view.read_with(cx, |view, _| view.repeating.is_none()));
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        assert_eq!(sent.get(), 3);
    }
}
