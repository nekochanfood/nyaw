//! OS キーボード（仮想キーボードから OS の入力先へのキー・文字列の送信）。表示サーバーごとに、X11 は XTest、
//! wlroots は仮想キーボード（zwp_virtual_keyboard_v1）、その他の Wayland は portal の RemoteDesktop で送る。
//! 配列の検出は `os::keyboard` が行い、ここは送信と入力先の把握だけを担う。

use std::{
    collections::HashMap,
    io::{self, Write as _},
    os::fd::AsFd,
    sync::Arc,
    time::Instant,
};

use os::keyboard::{KeyEvent, Output, Settings};
use rustix::{fs, mm};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    protocol::{wl_keyboard, wl_registry, wl_seat},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as manager, zwp_virtual_keyboard_v1 as virtual_keyboard,
};
use x11rb::{
    connection::Connection as _,
    protocol::{
        xproto::{self, AtomEnum, ConnectionExt as _},
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
};

use super::{Backend, portal};

/// OS キーボードの送信先。最初の [`os::keyboard::Host::settings`]（キーボードの監視スレッド）で表示サーバーに接続する。
pub(crate) struct Keyboard {
    base: Settings,
    sender: Option<Result<Sender, String>>,
}

enum Sender {
    X11(Box<X11>),
    Wlr(Box<Wlr>),
    Portal(Arc<portal::Portal>),
}

/// 入力先を特定できない表示サーバー（Wayland）で、入力先として報告する値（0 は「送信不可」）。
const FOCUSED: u64 = 1;

impl Keyboard {
    pub fn new() -> Self {
        Self {
            base: Settings::default(),
            sender: None,
        }
    }

    fn sender(&mut self) -> Result<&mut Sender, String> {
        if self.sender.is_none() {
            // 配列だけを使う（os の Linux の実装は送信できない旨のエラーを持つ）。
            self.base = Settings {
                error: None,
                foreground: 0,
                ..os::keyboard::detect()
            };
            let opened =
                super::backend()
                    .map_err(|e| e.to_string())
                    .and_then(|backend| match backend {
                        Backend::X11 => X11::new().map(|x| Sender::X11(Box::new(x))),
                        Backend::Wlr => Wlr::new().map(|w| Sender::Wlr(Box::new(w))),
                        Backend::Portal => portal::shared(None)
                            .map_err(|e| e.to_string())
                            .and_then(|p| {
                                if p.keyboard() {
                                    Ok(Sender::Portal(p))
                                } else {
                                    Err("the desktop portal did not allow the keyboard".into())
                                }
                            }),
                    });
            if let Err(error) = &opened {
                logger::warn!("cannot send keys to the OS", error = error);
            }
            self.sender = Some(opened);
        }
        self.sender
            .as_mut()
            .unwrap()
            .as_mut()
            .map_err(|e| e.clone())
    }
}

impl os::keyboard::Host for Keyboard {
    fn settings(&mut self) -> Settings {
        let state = self.sender().map(|sender| match sender {
            Sender::X11(x) => x.state(),
            Sender::Wlr(_) | Sender::Portal(_) => Focus {
                window: FOCUSED,
                own_process: false,
                caps_lock: false,
                num_lock: false,
            },
        });
        match state {
            Ok(focus) => Settings {
                foreground: focus.window,
                own_process: focus.own_process,
                caps_lock: focus.caps_lock,
                num_lock: focus.num_lock,
                ..self.base.clone()
            },
            Err(error) => Settings {
                foreground: 0,
                error: Some(error),
                ..self.base.clone()
            },
        }
    }

    fn send(&mut self, output: Output) -> io::Result<()> {
        let sender = self.sender().map_err(io::Error::other)?;
        match sender {
            Sender::X11(x) => x.send(output),
            Sender::Wlr(w) => w.send(output),
            Sender::Portal(p) => match output {
                Output::Keys(keys) => p.keys(&keys),
                // NotifyKeyboardKeysym は Shift などの段を付けない（KDE で「H」が「h」になった）。配列でキーと段を引いて
                // キーコードで送り、配列に無い文字だけ keysym で送る。
                Output::Text(text) => {
                    let layout = compositor_keymap().ok().and_then(|k| Layout::parse(&k));
                    for c in text.chars() {
                        let sym = keysym(c);
                        match layout.as_ref().and_then(|l| l.find(sym)) {
                            Some((code, level)) => p.keys(&level_tap(code, level))?,
                            None => p.keysym(sym)?,
                        }
                    }
                    Ok(())
                }
            },
        }
    }
}

/// OS の入力先と、ロックキーの状態。
struct Focus {
    window: u64,
    own_process: bool,
    caps_lock: bool,
    num_lock: bool,
}

/// 文字の keysym（X11 の `keysymdef.h`）。Latin-1 はそのままの値、それ以外は Unicode の keysym。
pub(super) fn keysym(c: char) -> u32 {
    match c as u32 {
        cp @ (0x20..=0x7e | 0xa0..=0xff) => cp,
        cp => 0x0100_0000 | cp,
    }
}

/// 配列での文字の位置（keysym → evdev のキーコードと段）。
pub(super) struct Layout(HashMap<u32, (u16, u32)>);

impl Layout {
    /// XKB の配列（テキスト）から、最初の配列の各キーの段 0〜3 の文字を引けるようにする。同じ文字は低い段を優先する。
    pub fn parse(keymap: &str) -> Option<Self> {
        use xkbcommon::xkb;
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_string(
            &context,
            keymap.to_owned(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )?;
        let mut found: HashMap<u32, (u16, u32)> = HashMap::new();
        for raw in keymap.min_keycode().raw().max(8)..=keymap.max_keycode().raw() {
            let key = xkb::Keycode::new(raw);
            for level in 0..keymap.num_levels_for_key(key, 0).min(4) {
                for sym in keymap.key_get_syms_by_level(key, 0, level) {
                    let place = ((raw - 8) as u16, level);
                    found
                        .entry(sym.raw())
                        .and_modify(|p| {
                            if level < p.1 {
                                *p = place
                            }
                        })
                        .or_insert(place);
                }
            }
        }
        Some(Self(found))
    }

    pub fn find(&self, keysym: u32) -> Option<(u16, u32)> {
        self.0.get(&keysym).copied()
    }
}

/// 段 `level` の文字を出す押下と解放（段 1 は Shift、段 2 は AltGr、段 3 は両方）。
pub(super) fn level_tap(code: u16, level: u32) -> Vec<KeyEvent> {
    let mut held = Vec::new();
    if level & 1 != 0 {
        held.push(42);
    }
    if level & 2 != 0 {
        held.push(100);
    }
    os::keyboard::chord(&held, code)
}

/// コンポジタの配列（wl_keyboard.keymap）。フォーカスが無くても、wl_keyboard を作ると届く。
fn compositor_keymap() -> Result<String, String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = WlrState::default();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    let seat = state.seat.clone().ok_or("the compositor has no seat")?;
    let keyboard = seat.get_keyboard(&qh, ());
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    keyboard.release();
    let _ = conn.flush();
    state
        .keymap
        .take()
        .ok_or_else(|| "the compositor sent no keymap".to_string())
}

// ---- X11 ----

const SHIFT_L: u32 = 0xffe1;

struct X11 {
    conn: RustConnection,
    root: xproto::Window,
    min: u8,
    max: u8,
    per: u8,
    keysyms: Vec<u32>,
    /// 配列に無い文字を割り当てた空きのキーコード（割り当ては残し、同じ文字に使い回す）。
    assigned: HashMap<u32, u8>,
    spare: Vec<u8>,
    active: xproto::Atom,
    pid: xproto::Atom,
}

impl X11 {
    fn new() -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        conn.xtest_get_version(2, 2)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|_| "X11: XTEST is unavailable".to_string())?;
        let atom = |name: &[u8]| -> Result<xproto::Atom, String> {
            Ok(conn
                .intern_atom(false, name)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .atom)
        };
        let (active, pid) = (atom(b"_NET_ACTIVE_WINDOW")?, atom(b"_NET_WM_PID")?);
        let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let mut x11 = Self {
            conn,
            root,
            min,
            max,
            per: 0,
            keysyms: Vec::new(),
            assigned: HashMap::new(),
            spare: Vec::new(),
            active,
            pid,
        };
        x11.read_mapping()?;
        // 何も割り当てられていないキーコードを、配列に無い文字に使う（大きい番号から）。
        x11.spare = (x11.min..=x11.max)
            .rev()
            .filter(|&k| x11.syms(k).iter().all(|&s| s == 0))
            .collect();
        Ok(x11)
    }

    fn read_mapping(&mut self) -> Result<(), String> {
        let count = self.max - self.min + 1;
        let reply = self
            .conn
            .get_keyboard_mapping(self.min, count)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        self.per = reply.keysyms_per_keycode;
        self.keysyms = reply.keysyms;
        Ok(())
    }

    fn syms(&self, keycode: u8) -> &[u32] {
        let per = self.per as usize;
        let i = (keycode - self.min) as usize * per;
        self.keysyms.get(i..i + per).unwrap_or(&[])
    }

    /// `keysym` を出すキーコードと、Shift が要るか。
    fn find(&self, keysym: u32) -> Option<(u8, bool)> {
        (self.min..=self.max).find_map(|k| match self.syms(k) {
            [plain, ..] if *plain == keysym => Some((k, false)),
            [_, shifted, ..] if *shifted == keysym => Some((k, true)),
            _ => None,
        })
    }

    fn state(&mut self) -> Focus {
        let window = self
            .conn
            .get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            .filter(|&w| w != 0)
            .or_else(|| {
                // ウィンドウマネージャが無ければ、入力のフォーカスを入力先とする。
                let focus = self.conn.get_input_focus().ok()?.reply().ok()?.focus;
                (focus > 1).then_some(focus)
            })
            .unwrap_or(FOCUSED as u32);
        let own_process = self
            .conn
            .get_property(false, window, self.pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            .is_some_and(|pid| pid == std::process::id());
        let mask = self
            .conn
            .query_pointer(self.root)
            .ok()
            .and_then(|c| c.reply().ok())
            .map_or(0, |r| u16::from(r.mask));
        Focus {
            window: window.into(),
            own_process,
            caps_lock: mask & u16::from(xproto::KeyButMask::LOCK) != 0,
            num_lock: mask & u16::from(xproto::KeyButMask::MOD2) != 0,
        }
    }

    fn key(&self, keycode: u8, down: bool) -> io::Result<()> {
        let kind = if down {
            xproto::KEY_PRESS_EVENT
        } else {
            xproto::KEY_RELEASE_EVENT
        };
        self.conn
            .xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, self.root, 0, 0, 0)
            .map_err(io::Error::other)?;
        Ok(())
    }

    /// 配列に無い文字を、空きのキーコードに割り当てる（空きが尽きたら最も古い割り当てを使い回す）。
    fn assign(&mut self, keysym: u32) -> io::Result<u8> {
        if let Some(&k) = self.assigned.get(&keysym) {
            return Ok(k);
        }
        let Some(keycode) = self.spare.pop() else {
            return Err(io::Error::other("X11: no free keycode for the character"));
        };
        // 使い終えた番号は先頭へ戻し、尽きたら最も古いものから使い回す。
        self.spare.insert(0, keycode);
        self.assigned.retain(|_, k| *k != keycode);
        let syms = vec![keysym; self.per.max(1) as usize];
        self.conn
            .change_keyboard_mapping(1, keycode, self.per.max(1), &syms)
            .map_err(io::Error::other)?;
        self.assigned.insert(keysym, keycode);
        Ok(keycode)
    }

    fn send(&mut self, output: Output) -> io::Result<()> {
        match output {
            Output::Keys(keys) => {
                for KeyEvent { code, down } in keys {
                    let keycode = u8::try_from(code + 8)
                        .map_err(|_| io::Error::other(format!("unsupported evdev key {code}")))?;
                    self.key(keycode, down)?;
                }
            }
            Output::Text(text) => {
                self.read_mapping().map_err(io::Error::other)?;
                let shift = self.find(SHIFT_L).map(|(k, _)| k);
                for c in text.chars() {
                    let sym = keysym(c);
                    let (keycode, shifted) = match self.find(sym) {
                        Some(found) => found,
                        None => (self.assign(sym)?, false),
                    };
                    let shift = shifted.then_some(shift).flatten();
                    if let Some(s) = shift {
                        self.key(s, true)?;
                    }
                    self.key(keycode, true)?;
                    self.key(keycode, false)?;
                    if let Some(s) = shift {
                        self.key(s, false)?;
                    }
                }
            }
        }
        // 送り終えるまで待ち、続く操作（次のフレームの判定）が送った後の状態を見るようにする。
        self.conn
            .get_input_focus()
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok(())
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        // 割り当てた文字を外す。
        let empty = vec![0u32; self.per.max(1) as usize];
        for &keycode in self.assigned.values() {
            let _ = self
                .conn
                .change_keyboard_mapping(1, keycode, self.per.max(1), &empty);
        }
        let _ = self.conn.flush();
    }
}

// ---- wlroots ----

/// 修飾キー（evdev）と、標準の XKB の配列での修飾子のビット。
const MODIFIERS: [(u16, u32); 9] = [
    (42, 1 << 0),  // Shift_L
    (54, 1 << 0),  // Shift_R
    (29, 1 << 2),  // Control_L
    (97, 1 << 2),  // Control_R
    (56, 1 << 3),  // Alt_L
    (125, 1 << 6), // Super_L
    (126, 1 << 6), // Super_R
    (100, 1 << 7), // ISO_Level3_Shift（AltGr）
    (184, 1 << 7), // ISO_Level3_Shift
];
const CAPS_LOCK: (u16, u32) = (58, 1 << 1);

#[derive(Default)]
struct WlrState {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<manager::ZwpVirtualKeyboardManagerV1>,
    /// コンポジタの配列（wl_keyboard.keymap）。
    keymap: Option<String>,
}

struct Wlr {
    conn: Connection,
    queue: EventQueue<WlrState>,
    keyboard: virtual_keyboard::ZwpVirtualKeyboardV1,
    /// コンポジタの配列（キーはこの配列で送る）。
    system: String,
    /// 押している修飾キー（evdev）と、Caps Lock の状態。
    held: Vec<u16>,
    locked: u32,
    started: Instant,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WlrState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "zwp_virtual_keyboard_manager_v1" => {
                    state.manager = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WlrState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap {
            format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
            fd,
            size,
        } = event
        {
            let len = size as usize;
            let Ok(ptr) = (unsafe {
                mm::mmap(
                    std::ptr::null_mut(),
                    len,
                    mm::ProtFlags::READ,
                    mm::MapFlags::PRIVATE,
                    &fd,
                    0,
                )
            }) else {
                return;
            };
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
            let text = bytes.split(|&b| b == 0).next().unwrap_or(&[]);
            state.keymap = Some(String::from_utf8_lossy(text).into_owned());
            let _ = unsafe { mm::munmap(ptr, len) };
        }
    }
}

macro_rules! no_events {
    ($($proxy:ty),*) => {$(
        impl Dispatch<$proxy, ()> for WlrState {
            fn event(_: &mut Self, _: &$proxy, _: <$proxy as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
no_events!(
    wl_seat::WlSeat,
    manager::ZwpVirtualKeyboardManagerV1,
    virtual_keyboard::ZwpVirtualKeyboardV1
);

/// 配列の文字列を、コンポジタへ渡す memfd にする（NUL で終える）。
fn keymap_fd(keymap: &str) -> io::Result<(std::os::fd::OwnedFd, u32)> {
    let fd = fs::memfd_create(c"nyaw-keymap", fs::MemfdFlags::CLOEXEC)?;
    let mut file = std::fs::File::from(fd);
    file.write_all(keymap.as_bytes())?;
    file.write_all(&[0])?;
    Ok((file.into(), keymap.len() as u32 + 1))
}

/// `chars` をキーコード 9 から順に割り当てた配列（文字列の送信用。wtype と同じ作り）。
pub(super) fn text_keymap(chars: &[char]) -> String {
    let mut keycodes = String::new();
    let mut symbols = String::new();
    for (i, c) in chars.iter().enumerate() {
        keycodes.push_str(&format!("<K{i}> = {};\n", i + 9));
        symbols.push_str(&format!("key <K{i}> {{[ U{:04X} ]}};\n", *c as u32));
    }
    format!(
        "xkb_keymap {{\nxkb_keycodes \"(unnamed)\" {{\nminimum = 8;\nmaximum = {};\n{keycodes}}};\n\
         xkb_types \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_compatibility \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_symbols \"(unnamed)\" {{\n{symbols}}};\n}};\n",
        (chars.len() + 9).max(255)
    )
}

impl Wlr {
    fn new() -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut state = WlrState::default();
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        let (Some(seat), Some(manager)) = (state.seat.clone(), state.manager.clone()) else {
            return Err("this compositor has no virtual keyboard".into());
        };
        let system = compositor_keymap()?;
        let virtual_keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
        let wlr = Self {
            conn,
            queue,
            keyboard: virtual_keyboard,
            system,
            held: Vec::new(),
            locked: 0,
            started: Instant::now(),
        };
        wlr.upload(&wlr.system.clone()).map_err(|e| e.to_string())?;
        wlr.conn.flush().map_err(|e| e.to_string())?;
        Ok(wlr)
    }

    fn upload(&self, keymap: &str) -> io::Result<()> {
        let (fd, size) = keymap_fd(keymap)?;
        self.keyboard
            .keymap(wl_keyboard::KeymapFormat::XkbV1.into(), fd.as_fd(), size);
        Ok(())
    }

    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn key(&self, code: u16, down: bool) {
        let state = if down {
            wl_keyboard::KeyState::Pressed
        } else {
            wl_keyboard::KeyState::Released
        };
        self.keyboard.key(self.time(), code.into(), state.into());
    }

    /// 仮想キーボードは修飾キーを押しても修飾子が変わらないため、押している修飾キーから送る。
    fn modifiers(&self) {
        let depressed = self
            .held
            .iter()
            .filter_map(|k| MODIFIERS.iter().find(|(m, _)| m == k).map(|(_, bit)| bit))
            .fold(0, |mods, bit| mods | bit);
        self.keyboard.modifiers(depressed, 0, self.locked, 0);
    }

    fn send(&mut self, output: Output) -> io::Result<()> {
        match output {
            Output::Keys(keys) => {
                for KeyEvent { code, down } in keys {
                    self.key(code, down);
                    if MODIFIERS.iter().any(|(m, _)| *m == code) {
                        self.held.retain(|k| *k != code);
                        if down {
                            self.held.push(code);
                        }
                        self.modifiers();
                    } else if code == CAPS_LOCK.0 && down {
                        self.locked ^= CAPS_LOCK.1;
                        self.modifiers();
                    }
                }
            }
            Output::Text(text) => {
                let mut chars: Vec<char> = Vec::new();
                for c in text.chars() {
                    if !chars.contains(&c) {
                        chars.push(c);
                    }
                }
                self.upload(&text_keymap(&chars))?;
                self.keyboard.modifiers(0, 0, 0, 0);
                for c in text.chars() {
                    let code = chars.iter().position(|x| *x == c).unwrap() as u16 + 1;
                    self.key(code, true);
                    self.key(code, false);
                }
                // 次のキー操作に備えて、コンポジタの配列へ戻す。
                self.upload(&self.system.clone())?;
                self.modifiers();
            }
        }
        self.conn.flush().map_err(io::Error::other)?;
        self.queue
            .roundtrip(&mut WlrState::default())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

impl Drop for Wlr {
    fn drop(&mut self) {
        for code in std::mem::take(&mut self.held) {
            self.key(code, false);
        }
        self.keyboard.destroy();
        let _ = self.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_become_keysyms() {
        assert_eq!(keysym('a'), 0x61);
        assert_eq!(keysym('é'), 0xe9);
        assert_eq!(keysym('あ'), 0x0100_3042);
        assert_eq!(keysym('€'), 0x0100_20ac);
    }

    /// 段 1 の文字は Shift 付き、段 2 は AltGr 付きで押す。配列は外部のデータを使わずに書く。
    #[test]
    fn layouts_find_the_key_and_level_of_each_character() {
        let keymap = r#"xkb_keymap {
            xkb_keycodes "t" { minimum = 8; maximum = 255; <AC01> = 38; <AE01> = 10; };
            xkb_types "t" {
                type "ONE_LEVEL" { modifiers = none; level_name[Level1] = "Any"; };
                type "TWO_LEVEL" { modifiers = Shift; map[Shift] = Level2;
                    level_name[Level1] = "Base"; level_name[Level2] = "Shift"; };
                type "ALPHABETIC" { modifiers = Shift; map[Shift] = Level2;
                    level_name[Level1] = "Base"; level_name[Level2] = "Caps"; };
                type "FOUR_LEVEL" { modifiers = Shift+Mod5; map[Shift] = Level2; map[Mod5] = Level3;
                    map[Shift+Mod5] = Level4; level_name[Level1] = "Base"; level_name[Level2] = "Shift";
                    level_name[Level3] = "AltGr"; level_name[Level4] = "Shift AltGr"; };
            };
            xkb_compatibility "t" { };
            xkb_symbols "t" {
                key <AC01> { [ a, A ] };
                key <AE01> { type = "FOUR_LEVEL", [ 1, exclam, onesuperior, exclamdown ] };
            };
        };"#;
        let layout = Layout::parse(keymap).expect("a valid keymap");
        assert_eq!(layout.find(u32::from(b'a')), Some((30, 0)));
        assert_eq!(layout.find(u32::from(b'A')), Some((30, 1)));
        assert_eq!(layout.find(u32::from(b'!')), Some((2, 1)));
        assert_eq!(layout.find(0xb9), Some((2, 2)));
        assert_eq!(layout.find(keysym('語')), None);
        let shifted: Vec<_> = level_tap(30, 1).iter().map(|k| (k.code, k.down)).collect();
        assert_eq!(shifted, [(42, true), (30, true), (30, false), (42, false)]);
        let altgr: Vec<_> = level_tap(2, 3).iter().map(|k| k.code).collect();
        assert_eq!(altgr, [42, 100, 2, 2, 100, 42]);
    }

    #[test]
    fn text_keymaps_put_each_character_on_its_own_key() {
        let keymap = text_keymap(&['h', 'あ']);
        assert!(keymap.contains("<K0> = 9;") && keymap.contains("<K1> = 10;"));
        assert!(keymap.contains("key <K0> {[ U0068 ]};"));
        assert!(keymap.contains("key <K1> {[ U3042 ]};"));
        assert!(keymap.contains("include \"complete\""));
    }
}
