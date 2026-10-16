use super::{Output, Settings};
use std::io;
use windows::Win32::{
    Foundation::*,
    System::Threading::GetCurrentProcessId,
    UI::{
        Input::{Ime::*, KeyboardAndMouse::*},
        WindowsAndMessaging::*,
    },
};

pub struct Backend;

impl Backend {
    pub fn new() -> Self {
        Self
    }

    pub fn settings(&mut self) -> Settings {
        // SAFETY: OS から借用したハンドルだけを使い、固定長バッファを API に渡す。
        unsafe {
            let window = GetForegroundWindow();
            let mut process = 0;
            let thread = GetWindowThreadProcessId(window, Some(&mut process));
            let layout = GetKeyboardLayout(thread);
            let previous = ActivateKeyboardLayout(layout, ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0)).ok();
            let mut name = [0u16; 9];
            let valid_name = GetKeyboardLayoutNameW(&mut name).is_ok();
            if let Some(previous) = previous {
                let _ = ActivateKeyboardLayout(previous, ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0));
            }
            let klid = if valid_name {
                String::from_utf16_lossy(&name[..8])
            } else {
                format!("{:08x}", layout.0 as usize & 0xffff)
            };
            let mut open = 0usize;
            let mut mode = 0usize;
            let ime = ImmGetDefaultIMEWnd(window);
            if !ime.is_invalid() {
                let _ = SendMessageTimeoutW(
                    ime,
                    WM_IME_CONTROL,
                    WPARAM(5),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    30,
                    Some(&mut open),
                );
                let _ = SendMessageTimeoutW(
                    ime,
                    WM_IME_CONTROL,
                    WPARAM(1),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    30,
                    Some(&mut mode),
                );
            }
            let mut mapped = map_layout(&klid, layout.0 as usize as u16);
            if let Some((name, variant)) = &mut mapped {
                if name == "jp" && open != 0 && mode & 1 != 0 && mode & 16 == 0 {
                    *variant = "kana".into();
                }
            }
            Settings {
                layout: mapped,
                input_method: format!(
                    "{klid} · IME {} · {mode:04x}",
                    if open != 0 { "on" } else { "off" }
                ),
                layout_description: None,
                foreground: window.0 as usize as u64,
                own_process: process == GetCurrentProcessId(),
                caps_lock: GetKeyState(0x14) & 1 != 0,
                num_lock: GetKeyState(0x90) & 1 != 0,
                error: None,
            }
        }
    }

    pub fn send(&mut self, output: Output) -> io::Result<()> {
        let inputs: Vec<INPUT> = match output {
            Output::Text(text) => text
                .encode_utf16()
                .flat_map(|unit| {
                    [
                        input(0, unit, KEYEVENTF_UNICODE),
                        input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
                    ]
                })
                .collect(),
            Output::Keys(keys) => keys
                .into_iter()
                .map(|key| {
                    let (scan, vk) = scan_code(key.code).ok_or_else(|| {
                        io::Error::other(format!("Unsupported evdev key {}", key.code))
                    })?;
                    let flags = if scan & 0xff00 != 0 {
                        KEYEVENTF_EXTENDEDKEY
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    } | if key.down {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    } | if vk == 0 {
                        KEYEVENTF_SCANCODE
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    };
                    Ok(input(vk, scan & 0xff, flags))
                })
                .collect::<io::Result<_>>()?,
        };
        // SAFETY: INPUT の配列と正確な構造体サイズを渡す。
        let count = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
        if count == inputs.len() {
            return Ok(());
        }
        // 部分的な注入でも、押した可能性のあるキーを解放する。
        let releases: Vec<_> = inputs
            .iter()
            .take(count)
            .rev()
            .filter_map(|event| unsafe {
                let mut key = event.Anonymous.ki;
                if key.dwFlags.contains(KEYEVENTF_KEYUP) {
                    return None;
                }
                key.dwFlags |= KEYEVENTF_KEYUP;
                Some(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: key },
                })
            })
            .collect();
        if !releases.is_empty() {
            unsafe {
                SendInput(&releases, size_of::<INPUT>() as i32);
            }
        }
        Err(io::Error::other(
            "SendInput failed (the target may have a higher integrity level)",
        ))
    }
}

fn input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn scan_code(code: u16) -> Option<(u16, u16)> {
    Some(match code {
        1..=83 | 86..=88 => (code, 0),
        89 => (0x73, 0),
        90 => (0, 0x15),
        91 => (0, 0x19),
        92 => (0x79, 0),
        93 => (0x70, 0),
        94 => (0x7b, 0),
        95 => (0x7e, 0),
        96 => (0xe01c, 0),
        97 => (0xe01d, 0),
        98 => (0xe035, 0),
        99 => (0xe037, 0),
        100 => (0xe038, 0),
        102 => (0xe047, 0),
        103 => (0xe048, 0),
        104 => (0xe049, 0),
        105 => (0xe04b, 0),
        106 => (0xe04d, 0),
        107 => (0xe04f, 0),
        108 => (0xe050, 0),
        109 => (0xe051, 0),
        110 => (0xe052, 0),
        111 => (0xe053, 0),
        119 => (0, 0x13),
        122 => (0, 0x15),
        123 => (0, 0x19),
        124 => (0x7d, 0),
        125 => (0xe05b, 0),
        126 => (0xe05c, 0),
        127 => (0xe05d, 0),
        _ => return None,
    })
}

fn map_layout(klid: &str, language: u16) -> Option<(String, String)> {
    let special = match klid.to_ascii_lowercase().as_str() {
        "00010409" => Some(("us", "dvorak")),
        "00020409" => Some(("us", "intl")),
        "00030409" => Some(("us", "dvorak-l")),
        "00040409" => Some(("us", "dvorak-r")),
        "00000809" => Some(("gb", "")),
        "00000409" => Some(("us", "")),
        "00010407" => Some(("de", "nodeadkeys")),
        "0001040a" => Some(("es", "nodeadkeys")),
        "00010416" => Some(("br", "nativo")),
        "00010419" => Some(("ru", "typewriter")),
        "0001041f" => Some(("tr", "f")),
        "00010405" => Some(("cz", "qwerty")),
        "0000080a" => Some(("latam", "")),
        "00001009" => Some(("ca", "multix")),
        "00000c0c" => Some(("ca", "fr")),
        "00000813" => Some(("be", "")),
        "00000807" => Some(("ch", "")),
        "0000100c" => Some(("ch", "fr")),
        _ => None,
    };
    if let Some((name, variant)) = special {
        return Some((name.into(), variant.into()));
    }
    let layout = match language & 0x3ff {
        0x01 => "ara",
        0x02 => "bg",
        0x03 => "es",
        0x04 => "us",
        0x05 => "cz",
        0x06 => "dk",
        0x07 => "de",
        0x08 => "gr",
        0x09 => "us",
        0x0a => "es",
        0x0b => "fi",
        0x0c => "fr",
        0x0d => "il",
        0x0e => "hu",
        0x0f => "is",
        0x10 => "it",
        0x11 => "jp",
        0x12 => "kr",
        0x13 => "nl",
        0x14 => "no",
        0x15 => "pl",
        0x16 => {
            if language == 0x416 {
                "br"
            } else {
                "pt"
            }
        }
        0x18 => "ro",
        0x19 => "ru",
        0x1a => "hr",
        0x1b => "sk",
        0x1c => "al",
        0x1d => "se",
        0x1e => "th",
        0x1f => "tr",
        0x20 => "pk",
        0x21 => "id",
        0x22 => "ua",
        0x23 => "by",
        0x24 => "si",
        0x25 => "ee",
        0x26 => "lv",
        0x27 => "lt",
        0x29 => "ir",
        0x2a => "vn",
        0x2b => "am",
        0x2f => "mk",
        0x37 => "ge",
        0x39 => "in",
        0x3f => "kz",
        0x40 => "kg",
        0x43 => "uz",
        0x44 => "ru",
        0x45 => "bd",
        0x46 => "in",
        0x49 => "in",
        0x4a => "in",
        0x4b => "in",
        0x4c => "in",
        0x4e => "in",
        0x50 => "mn",
        0x53 => "kh",
        0x54 => "la",
        0x55 => "mm",
        0x61 => "np",
        _ => return None,
    };
    Some((layout.into(), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_layouts_and_extended_keys() {
        assert_eq!(
            map_layout("00010409", 0x409),
            Some(("us".into(), "dvorak".into()))
        );
        assert_eq!(
            map_layout("00000411", 0x411),
            Some(("jp".into(), String::new()))
        );
        assert_eq!(scan_code(100), Some((0xe038, 0)));
        assert_eq!(scan_code(124), Some((0x7d, 0)));
    }

    #[test]
    #[ignore = "検証用の入力ウィンドウを一時的にフォアグラウンドへ出す"]
    fn native_scan_codes_and_unicode_reach_the_selected_edit_window() {
        use std::{
            thread,
            time::{Duration, Instant},
        };
        use windows::core::w;
        // SAFETY: テスト専用の標準 EDIT ウィンドウを作り、同じスレッドでメッセージを処理する。
        unsafe {
            let previous = GetKeyboardLayout(0);
            let layout = LoadKeyboardLayoutW(w!("00000409"), KLF_ACTIVATE).unwrap();
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                360,
                160,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            struct Cleanup(HWND, HKL);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    unsafe {
                        let _ = DestroyWindow(self.0);
                        let _ = ActivateKeyboardLayout(self.1, ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0));
                    }
                }
            }
            let _cleanup = Cleanup(window, previous);
            assert!(!layout.0.is_null());
            let _ = SetForegroundWindow(window);
            let _ = SetFocus(Some(window));
            let mut backend = Backend::new();
            assert_eq!(backend.settings().foreground, window.0 as usize as u64);
            backend
                .send(Output::Keys(super::super::chord(&[], 30)))
                .unwrap();
            backend
                .send(Output::Keys(super::super::chord(&[42], 48)))
                .unwrap();
            backend.send(Output::Text("Ω🐈".into())).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let mut text = [0u16; 64];
                let count = GetWindowTextW(window, &mut text) as usize;
                if String::from_utf16_lossy(&text[..count]) == "aBΩ🐈" {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "unexpected native input: {:?}",
                    String::from_utf16_lossy(&text[..count])
                );
                thread::sleep(Duration::from_millis(10));
            }
            assert!(GetAsyncKeyState(0xa0) >= 0, "Shift was left pressed");
        }
    }
}
