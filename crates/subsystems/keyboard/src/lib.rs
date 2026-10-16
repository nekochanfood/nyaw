//! xkeyboard-config による配列・文字・トグル状態。OS への送信先は UI が決める。
use std::{
    collections::BTreeMap,
    fs, io,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use kbvm::{
    Components, Keycode, Keysym,
    lookup::LookupTable,
    state_machine::{Direction, State, StateMachine},
    xkb::{Context, rmlvo::Group},
};

pub mod geometry;
pub use geometry::{KeyShape, KeyboardType, Keycap};

include!(concat!(env!("OUT_DIR"), "/data.rs"));

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Layout {
    pub name: String,
    pub variant: String,
    pub description: String,
}

impl Layout {
    pub fn new(name: &str, variant: &str) -> Self {
        Self {
            name: name.into(),
            variant: variant.into(),
            description: format!("{name} ({variant})"),
        }
    }

    pub fn model(&self) -> &'static str {
        match self.name.as_str() {
            "jp" => "jp106",
            "kr" => "kr106",
            "us" => "pc104",
            _ => "pc105",
        }
    }
}

/// 初回の展開とコンパイルは呼び出し側のワーカースレッドで行う。
pub struct Database {
    context: Context,
    physical_keys: BTreeMap<String, Keycode>,
    compose: Arc<kbvm::xkb::compose::ComposeTable>,
    pub layouts: Vec<Layout>,
}

impl Database {
    pub fn load() -> io::Result<Self> {
        static ROOT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
        let root = ROOT
            .get_or_init(|| Self::unpack().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))?;
        Self::at(root.clone())
    }

    fn unpack() -> io::Result<PathBuf> {
        let root = os::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("xkeyboard-config-2.48");
        for (name, bytes) in DATA {
            let path = root.join(name);
            if fs::read(&path).is_ok_and(|old| old == *bytes) {
                continue;
            }
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, bytes)?;
        }
        Ok(root)
    }

    fn at(root: PathBuf) -> io::Result<Self> {
        let mut builder = Context::builder();
        builder.enable_default_includes(false);
        builder.enable_environment(false);
        builder.append_path(&root);
        builder.load_extra_rules(true);
        let context = builder.build();
        let registry = context.registry(kbvm::xkb::diagnostic::WriteToLog, "evdev");
        let mut layouts = Vec::new();
        for entry in registry.layouts {
            // custom は利用者が別途 symbols/custom を置くための項目で、配布元には配列がない。
            if entry.name == "custom" {
                continue;
            }
            layouts.push(Layout {
                name: entry.name.clone(),
                variant: String::new(),
                description: entry.description.unwrap_or_else(|| entry.name.clone()),
            });
            layouts.extend(entry.variants.into_iter().map(|variant| {
                Layout {
                    name: entry.name.clone(),
                    description: variant
                        .description
                        .unwrap_or_else(|| format!("{} ({})", entry.name, variant.name)),
                    variant: variant.name,
                }
            }));
        }
        layouts.sort_by(|a, b| a.description.cmp(&b.description));
        layouts.dedup_by(|a, b| a.name == b.name && a.variant == b.variant);
        if layouts.is_empty() {
            return Err(io::Error::other("empty XKB registry"));
        }
        let mut compose = context.compose_table_builder();
        compose.buffer(include_bytes!("../data/Compose"));
        let compose = Arc::new(
            compose
                .build(kbvm::xkb::diagnostic::WriteToLog)
                .ok_or_else(|| io::Error::other("Cannot load Compose table"))?,
        );
        let mut physical_keys = BTreeMap::new();
        for (layout, model) in [("us", "pc105"), ("jp", "jp106"), ("kr", "kr106")] {
            let keymap = context.keymap_from_names(
                kbvm::xkb::diagnostic::WriteToLog,
                Some("evdev"),
                Some(model),
                Some(&[Group {
                    layout,
                    variant: "",
                }]),
                Some(&[]),
            );
            physical_keys.extend(
                keymap
                    .keys()
                    .map(|key| (key.name().to_owned(), key.keycode())),
            );
        }
        // evdev の MENU は COMP の別名で、コンパイル済み keymap には正規名だけが残る。
        if let Some(&code) = physical_keys.get("COMP") {
            physical_keys.insert("MENU".into(), code);
        }
        Ok(Self {
            context,
            physical_keys,
            layouts,
            compose,
        })
    }

    pub fn keyboard(&self, layout: &Layout) -> Result<Keyboard, String> {
        if !self
            .layouts
            .iter()
            .any(|l| l.name == layout.name && l.variant == layout.variant)
        {
            return Err(format!(
                "Unknown XKB layout: {}/{}",
                layout.name, layout.variant
            ));
        }
        let mut diagnostics = Vec::new();
        let keymap = self.context.keymap_from_names(
            &mut diagnostics,
            Some("evdev"),
            Some(layout.model()),
            Some(&[Group {
                layout: &layout.name,
                variant: &layout.variant,
            }]),
            Some(&[]),
        );
        let source_keys: BTreeMap<_, _> = keymap
            .keys()
            .map(|k| (k.name().to_owned(), k.keycode()))
            .collect();
        if source_keys.is_empty() {
            return Err(format!(
                "Cannot compile XKB layout: {}/{}",
                layout.name, layout.variant
            ));
        }
        let mut keys = self.physical_keys.clone();
        keys.extend(source_keys);
        let builder = keymap.to_builder();
        let alphabet = if layout.name == "jp" && layout.variant == "kana" {
            Some(
                self.context
                    .keymap_from_names(
                        kbvm::xkb::diagnostic::WriteToLog,
                        Some("evdev"),
                        Some("jp106"),
                        Some(&[Group {
                            layout: "jp",
                            variant: "",
                        }]),
                        Some(&[]),
                    )
                    .to_builder()
                    .build_lookup_table(),
            )
        } else {
            None
        };
        let machine = builder.build_state_machine();
        let state = machine.create_state();
        Ok(Keyboard {
            layout: layout.clone(),
            lookup: builder.build_lookup_table(),
            machine,
            state,
            components: Components::default(),
            held: Vec::new(),
            keys,
            compose: self.compose.clone(),
            compose_state: self.compose.create_state(),
            system_locks: (false, false),
            alphabet,
        })
    }
}

/// 一操作。物理キー番号は evdev（XKB の +8 は含まない）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stroke {
    pub code: u16,
    pub held: Vec<u16>,
    pub symbols: Vec<Keysym>,
    pub text: String,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub shift: bool,
    pub composing: bool,
}

impl Stroke {
    pub fn shortcut(&self) -> bool {
        self.control || self.alt || self.super_key
    }
}

pub struct Keyboard {
    pub layout: Layout,
    lookup: LookupTable,
    machine: StateMachine,
    state: State,
    components: Components,
    held: Vec<(Keycode, bool)>,
    keys: BTreeMap<String, Keycode>,
    compose: Arc<kbvm::xkb::compose::ComposeTable>,
    compose_state: kbvm::xkb::compose::State,
    system_locks: (bool, bool),
    alphabet: Option<LookupTable>,
}

impl Keyboard {
    pub fn sync_locks(&mut self, caps: bool, num: bool) {
        for (code, before, after) in [
            (58, self.system_locks.0, caps),
            (69, self.system_locks.1, num),
        ] {
            if before != after {
                let held = self.is_on(code);
                let key = Keycode::from_evdev(u32::from(code));
                if held {
                    self.event(key, Direction::Up);
                }
                self.event(key, Direction::Down);
                if !held {
                    self.event(key, Direction::Up);
                }
            }
        }
        self.system_locks = (caps, num);
    }
    pub fn code(&self, name: &str) -> Option<u16> {
        self.keys.get(name).map(|k| k.to_evdev() as u16)
    }

    pub fn keycaps(&self, kind: KeyboardType) -> Vec<Keycap> {
        geometry::keycaps(self, kind)
    }

    pub fn is_on(&self, code: u16) -> bool {
        self.held
            .iter()
            .any(|(k, _)| k.to_evdev() == u32::from(code))
    }

    fn symbols(&self, code: u16) -> Vec<Keysym> {
        self.lookup
            .lookup(
                self.components.group,
                self.components.mods,
                Keycode::from_evdev(u32::from(code)),
            )
            .into_iter()
            .map(|s| s.keysym())
            .collect()
    }

    pub fn label(&self, code: u16) -> String {
        self.symbols(code)
            .into_iter()
            .map(|sym| match sym.name().unwrap_or("") {
                "space" => "Space".into(),
                "Tab" | "ISO_Left_Tab" => "Tab".into(),
                "Return" => "Enter".into(),
                "BackSpace" => "⌫".into(),
                "Shift_L" | "Shift_R" => "Shift".into(),
                "Control_L" | "Control_R" => "Ctrl".into(),
                "Super_L" | "Super_R" => "Super".into(),
                "Alt_L" | "Alt_R" => "Alt".into(),
                "ISO_Level3_Shift" => "AltGr".into(),
                "ISO_Level5_Shift" => "Level 5".into(),
                "Caps_Lock" => "Caps Lock".into(),
                "Num_Lock" => "Num".into(),
                "Scroll_Lock" => "Scroll".into(),
                "Zenkaku_Hankaku" | "Kanji" => "Han/Zen".into(),
                "Muhenkan" => "無変換".into(),
                "Henkan_Mode" => "変換".into(),
                "Hiragana_Katakana" => "かな".into(),
                "Eisu_toggle" => "英数".into(),
                "Escape" => "Esc".into(),
                "Left" | "KP_Left" => "←".into(),
                "Right" | "KP_Right" => "→".into(),
                "Up" | "KP_Up" => "↑".into(),
                "Down" | "KP_Down" => "↓".into(),
                "KP_Enter" => "Enter".into(),
                "KP_Divide" => "/".into(),
                "KP_Multiply" => "*".into(),
                "KP_Add" => "+".into(),
                "KP_Subtract" => "−".into(),
                name => sym
                    .char()
                    .filter(|c| !c.is_control())
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| name.trim_start_matches("KP_").replace('_', " ")),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// かな入力中だけ、優先するかなとは別にローマ字の印字も残す。
    pub fn alphabet_label(&self, code: u16) -> Option<String> {
        let alphabet = self.alphabet.as_ref()?;
        let label: String = alphabet
            .lookup(
                self.components.group,
                self.components.mods,
                Keycode::from_evdev(u32::from(code)),
            )
            .into_iter()
            .filter_map(|s| s.keysym().char())
            .filter(|c| !c.is_control())
            .collect();
        let primary = self.label(code);
        (!label.is_empty() && label != primary).then_some(label)
    }

    pub fn symbol_name(&self, code: u16) -> Option<&'static str> {
        self.symbols(code).first().and_then(|s| s.name())
    }

    pub fn is_toggle(&self, code: u16) -> bool {
        self.symbols(code).iter().any(|s| {
            s.is_modifier()
                || matches!(
                    s.name(),
                    Some("Tab" | "ISO_Left_Tab" | "Num_Lock" | "Scroll_Lock" | "Multi_key")
                )
        })
    }

    fn event(&mut self, code: Keycode, direction: Direction) {
        let mut events = Vec::new();
        self.machine
            .handle_key(&mut self.state, &mut events, code, direction);
        for event in events {
            self.components.apply_event(event);
        }
    }

    fn stroke(&mut self, code: u16) -> Stroke {
        let symbols = self.symbols(code);
        let mods = self.components.mods.0;
        let mut stroke = Stroke {
            code,
            held: self
                .held
                .iter()
                .map(|(k, _)| k.to_evdev() as u16)
                .filter(|k| *k != code)
                .collect(),
            symbols,
            control: mods & 4 != 0,
            alt: mods & 8 != 0,
            super_key: mods & 64 != 0,
            shift: mods & 1 != 0,
            ..Stroke::default()
        };
        if !stroke.shortcut() {
            use kbvm::xkb::compose::FeedResult;
            for &symbol in &stroke.symbols {
                match self.compose.feed(&mut self.compose_state, symbol) {
                    Some(FeedResult::Pending) => stroke.composing = true,
                    Some(FeedResult::Composed { string, keysym }) => {
                        if let Some(text) = string {
                            stroke.text.push_str(text);
                        } else if let Some(c) = keysym.and_then(|s| s.char()) {
                            stroke.text.push(c);
                        }
                    }
                    Some(FeedResult::Aborted) => {}
                    None => {
                        if let Some(c) = symbol.char() {
                            stroke.text.push(c);
                        }
                    }
                }
            }
        }
        stroke
    }

    /// トグルは解除まで維持する。単独入力は on → off、組み合わせに使った解除は入力しない。
    pub fn click(&mut self, code: u16) -> Option<Stroke> {
        let key = Keycode::from_evdev(u32::from(code));
        if let Some(index) = self.held.iter().position(|(k, _)| *k == key) {
            let (_, used) = self.held.remove(index);
            let stroke = (!used).then(|| self.stroke(code));
            let locks = self
                .symbols(code)
                .iter()
                .any(|s| matches!(s.name(), Some("Caps_Lock" | "Num_Lock" | "Scroll_Lock")));
            self.event(key, Direction::Up);
            if locks {
                self.event(key, Direction::Down);
                self.event(key, Direction::Up);
            }
            if stroke.is_some() {
                for (_, used) in &mut self.held {
                    *used = true;
                }
            }
            return stroke;
        }
        if self.is_toggle(code) {
            self.event(key, Direction::Down);
            self.held.push((key, false));
            None
        } else {
            let stroke = self.stroke(code);
            for (_, used) in &mut self.held {
                *used = true;
            }
            self.event(key, Direction::Down);
            self.event(key, Direction::Up);
            Some(stroke)
        }
    }

    pub fn reset(&mut self) {
        let locks = self.system_locks;
        self.state = self.machine.create_state();
        self.components = Components::default();
        self.held.clear();
        self.compose_state = self.compose.create_state();
        self.system_locks = (false, false);
        self.sync_locks(locks.0, locks.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_and_sticky_chords() {
        let database = Database::load().unwrap();
        assert!(database.layouts.len() > 700);
        for (layout, variant, expected) in [
            ("us", "", "a"),
            ("fr", "", "q"),
            ("us", "dvorak", "a"),
            ("ru", "", "ф"),
            ("jp", "", "a"),
        ] {
            let mut keyboard = database.keyboard(&Layout::new(layout, variant)).unwrap();
            let a = keyboard.code("AC01").unwrap();
            assert_eq!(keyboard.click(a).unwrap().text, expected);
        }
        let mut keyboard = database.keyboard(&Layout::new("us", "")).unwrap();
        let (shift, ctrl, a, tab) = (42, 29, 30, 15);
        assert!(keyboard.click(shift).is_none());
        assert_eq!(keyboard.label(a), "A");
        assert_eq!(keyboard.click(a).unwrap().text, "A");
        assert!(keyboard.click(shift).is_none());
        assert_eq!(keyboard.label(a), "a");
        keyboard.click(ctrl);
        keyboard.click(shift);
        let stroke = keyboard.click(a).unwrap();
        assert_eq!(stroke.held, [ctrl, shift]);
        assert!(stroke.control && stroke.shift && stroke.text.is_empty());
        keyboard.reset();
        assert!(keyboard.click(tab).is_none());
        assert_eq!(keyboard.click(tab).unwrap().code, tab);
        assert!(keyboard.click(58).is_none());
        assert_eq!(keyboard.label(a), "A");
        keyboard.click(58);
        assert_eq!(keyboard.label(a), "a");
        let mut german = database.keyboard(&Layout::new("de", "")).unwrap();
        german.click(100);
        assert_eq!(german.click(16).unwrap().text, "@");
        let mut intl = database.keyboard(&Layout::new("us", "intl")).unwrap();
        assert!(intl.click(40).unwrap().composing);
        assert_eq!(intl.click(18).unwrap().text, "é");
        keyboard.reset();
        keyboard.sync_locks(true, false);
        assert_eq!(keyboard.click(a).unwrap().text, "A");
        keyboard.click(shift);
        assert_eq!(keyboard.click(a).unwrap().text, "a");
        keyboard.reset();
        assert_eq!(keyboard.click(a).unwrap().text, "A");
    }

    #[test]
    fn every_registered_layout_compiles_for_all_three_types() {
        let database = Database::load().unwrap();
        for layout in &database.layouts {
            let keyboard = database.keyboard(layout).unwrap_or_else(|e| panic!("{e}"));
            for kind in KeyboardType::ALL {
                let keys = keyboard.keycaps(kind);
                assert!(
                    keys.len() >= 50,
                    "{} / {}: {kind:?}",
                    layout.name,
                    layout.variant
                );
                // NoSymbol の空白キーも有効な配列なので、文字の有無ではなく物理領域を検証する。
                for key in keys {
                    assert!(
                        key.x >= 0.
                            && key.y >= 0.
                            && key.x + key.width <= kind.width()
                            && key.y + key.height <= kind.height(),
                        "{} / {}: {}",
                        layout.name,
                        layout.variant,
                        key.name
                    );
                }
            }
        }
    }
}
