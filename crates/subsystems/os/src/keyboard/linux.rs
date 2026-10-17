use super::{Output, Settings};
use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xkb::{ConnectionExt as _, ID},
        xproto::{AtomEnum, ConnectionExt},
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
};

pub struct Backend {
    connection: Option<(RustConnection, usize)>,
    desktop: Desktop,
    last_desktop: Option<Instant>,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            connection: x11rb::connect(None).ok(),
            desktop: Desktop::default(),
            last_desktop: None,
        }
    }

    pub fn settings(&mut self) -> Settings {
        if self
            .last_desktop
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(400))
        {
            self.desktop = desktop_settings();
            self.last_desktop = Some(Instant::now());
        }
        let mut settings = self.x11_settings();
        if self.desktop.layout.is_some() {
            settings.layout = self.desktop.layout.clone();
        }
        settings.layout_description = self.desktop.description.clone();
        if !self.desktop.ime.is_empty() {
            settings.input_method = self.desktop.ime.clone();
        }
        if let Some((foreground, own)) = self.desktop.foreground {
            settings.foreground = foreground;
            settings.own_process = own;
        }
        if settings.layout.is_some() || settings.layout_description.is_some() {
            settings.error = None;
        }
        settings
    }

    fn x11_settings(&self) -> Settings {
        let Some((connection, screen)) = &self.connection else {
            return Settings {
                error: Some("Keyboard monitoring requires a desktop session".into()),
                ..Settings::default()
            };
        };
        let result = (|| -> Result<Settings, Box<dyn std::error::Error>> {
            let root = connection.setup().roots[*screen].root;
            let atom = |name: &str| -> Result<u32, Box<dyn std::error::Error>> {
                Ok(connection
                    .intern_atom(false, name.as_bytes())?
                    .reply()?
                    .atom)
            };
            let property = |window, name: &str| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
                Ok(connection
                    .get_property(false, window, atom(name)?, AtomEnum::ANY, 0, 4096)?
                    .reply()?
                    .value)
            };
            let names = property(root, "_XKB_RULES_NAMES")?;
            let names: Vec<_> = names
                .split(|b| *b == 0)
                .map(String::from_utf8_lossy)
                .collect();
            let state = connection
                .xkb_get_state(ID::USE_CORE_KBD.into())?
                .reply()
                .ok();
            let group = state
                .as_ref()
                .map(|s| u8::from(s.group) as usize)
                .unwrap_or(0);
            let layout = names
                .get(2)
                .and_then(|s| s.split(',').nth(group))
                .filter(|s| !s.is_empty());
            let variant = names
                .get(3)
                .and_then(|s| s.split(',').nth(group))
                .unwrap_or("");
            let active = property(root, "_NET_ACTIVE_WINDOW")?;
            let window = active
                .get(..4)
                .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
                .filter(|w| *w != 0)
                .unwrap_or(connection.get_input_focus()?.reply()?.focus);
            let process = property(window, "_NET_WM_PID").unwrap_or_default();
            let own_process = process
                .get(..4)
                .is_some_and(|b| u32::from_ne_bytes(b.try_into().unwrap()) == std::process::id());
            Ok(Settings {
                layout: layout.map(|s| (s.to_owned(), variant.to_owned())),
                input_method: format!("XKB group {}", group + 1),
                layout_description: None,
                foreground: u64::from(window),
                own_process,
                caps_lock: state
                    .as_ref()
                    .is_some_and(|s| u16::from(s.locked_mods) & 2 != 0),
                num_lock: state
                    .as_ref()
                    .is_some_and(|s| u16::from(s.locked_mods) & 16 != 0),
                error: None,
            })
        })();
        result.unwrap_or_else(|e| Settings {
            error: Some(e.to_string()),
            ..Settings::default()
        })
    }

    pub fn send(&mut self, output: Output) -> io::Result<()> {
        let (connection, _) = self
            .connection
            .as_ref()
            .ok_or_else(|| io::Error::other("No X11 connection"))?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            match output {
                Output::Keys(keys) => {
                    for key in keys {
                        let code = u8::try_from(key.code + 8)?;
                        connection
                            .xtest_fake_input(if key.down { 2 } else { 3 }, code, 0, 0, 0, 0, 0)?
                            .check()?;
                    }
                }
                Output::Text(text) => {
                    let min = connection.setup().min_keycode;
                    let max = connection.setup().max_keycode;
                    let mapping = connection
                        .get_keyboard_mapping(min, max - min + 1)?
                        .reply()?;
                    let stride = mapping.keysyms_per_keycode as usize;
                    let spare = mapping
                        .keysyms
                        .chunks(stride)
                        .enumerate()
                        .rev()
                        .find(|(_, s)| s.iter().all(|k| *k == 0))
                        .map(|(i, _)| min + i as u8)
                        .ok_or("No unused X11 keycode for Unicode input")?;
                    restore_after(
                        || -> Result<(), Box<dyn std::error::Error>> {
                            for c in text.chars() {
                                let symbol = if u32::from(c) < 256 {
                                    u32::from(c)
                                } else {
                                    0x0100_0000 | u32::from(c)
                                };
                                connection
                                    .change_keyboard_mapping(1, spare, 1, &[symbol])?
                                    .check()?;
                                connection
                                    .xtest_fake_input(2, spare, 0, 0, 0, 0, 0)?
                                    .check()?;
                                connection
                                    .xtest_fake_input(3, spare, 0, 0, 0, 0, 0)?
                                    .check()?;
                            }
                            Ok(())
                        },
                        || -> Result<(), Box<dyn std::error::Error>> {
                            connection
                                .change_keyboard_mapping(1, spare, stride as u8, &vec![0; stride])?
                                .check()?;
                            Ok(())
                        },
                    )?;
                }
            }
            connection.flush()?;
            Ok(())
        })();
        result.map_err(|e| io::Error::other(e.to_string()))
    }
}

fn restore_after<T, E>(
    send: impl FnOnce() -> Result<T, E>,
    restore: impl FnOnce() -> Result<(), E>,
) -> Result<T, E> {
    let sent = send();
    restore()?;
    sent
}

#[derive(Default)]
struct Desktop {
    layout: Option<(String, String)>,
    description: Option<String>,
    ime: String,
    foreground: Option<(u64, bool)>,
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut output = String::new();
                child.stdout.take()?.read_to_string(&mut output).ok()?;
                return Some(output.trim().to_owned());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn source_layout(source: &str) -> Option<(String, String)> {
    let source = source.strip_prefix("keyboard-").unwrap_or(source);
    let (layout, variant) = source
        .split_once('+')
        .or_else(|| source.split_once('-'))
        .unwrap_or((source, ""));
    if layout.is_empty() || !layout.bytes().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some((layout.into(), variant.into()))
}

fn desktop_settings() -> Desktop {
    let mut state = Desktop::default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    if desktop.contains("gnome") || desktop.contains("unity") {
        let sources = command(
            "gsettings",
            &["get", "org.gnome.desktop.input-sources", "mru-sources"],
        );
        if let Some(sources) = sources {
            let strings: Vec<_> = sources.split('\'').skip(1).step_by(2).collect();
            if strings.first() == Some(&"xkb") {
                state.layout = strings.get(1).and_then(|s| source_layout(s));
            } else if strings.first() == Some(&"ibus") {
                state.ime = strings.get(1).unwrap_or(&"").to_string();
            }
        }
    }
    if desktop.contains("kde") {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
            });
        if let Some(config) = root.and_then(|r| std::fs::read_to_string(r.join("kxkbrc")).ok()) {
            let index = command(
                "gdbus",
                &[
                    "call",
                    "--session",
                    "--dest",
                    "org.kde.keyboard",
                    "--object-path",
                    "/Layouts",
                    "--method",
                    "org.kde.KeyboardLayouts.getCurrentLayout",
                ],
            )
            .and_then(|s| {
                s.split_whitespace()
                    .nth(1)
                    .map(|n| n.trim_end_matches([',', ')']).to_owned())
            })
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
            let value = |name: &str| {
                config
                    .lines()
                    .find_map(|line| line.strip_prefix(name))
                    .and_then(|s| s.split(',').nth(index))
                    .unwrap_or("")
            };
            let layout = value("LayoutList=");
            if !layout.is_empty() {
                state.layout = Some((layout.into(), value("VariantList=").into()));
            }
        }
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        if let Some(inputs) = command("swaymsg", &["-t", "get_inputs", "-r"])
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            state.description = inputs.as_array().and_then(|v| {
                v.iter().find_map(|i| {
                    i.get("xkb_active_layout_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
            });
        }
        if let Some(tree) = command("swaymsg", &["-t", "get_tree", "-r"])
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            state.foreground = focused_sway(&tree);
        }
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Some(devices) = command("hyprctl", &["-j", "devices"])
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            state.description = devices
                .get("keyboards")
                .and_then(|v| v.as_array())
                .and_then(|v| {
                    v.iter()
                        .find(|k| k.get("main").and_then(|b| b.as_bool()) == Some(true))
                })
                .and_then(|k| k.get("active_keymap"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
        }
        if let Some(window) = command("hyprctl", &["-j", "activewindow"])
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            state.foreground = window
                .get("address")
                .and_then(|v| v.as_str())
                .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .map(|id| {
                    (
                        id,
                        window.get("pid").and_then(|v| v.as_u64())
                            == Some(u64::from(std::process::id())),
                    )
                });
        }
    }
    for (program, args) in [
        ("fcitx5-remote", &["-n"][..]),
        ("fcitx-remote", &["-n"][..]),
        ("ibus", &["engine"][..]),
    ] {
        if let Some(engine) = command(program, args).filter(|s| !s.is_empty()) {
            if engine.starts_with("keyboard-") {
                state.layout = source_layout(&engine);
            }
            if let Some(source) = engine.strip_prefix("xkb:") {
                let parts: Vec<_> = source.split(':').collect();
                if parts.len() >= 2 {
                    state.layout = Some((parts[0].into(), parts[1].into()));
                }
            }
            state.ime = engine;
            break;
        }
    }
    state
}

fn focused_sway(node: &serde_json::Value) -> Option<(u64, bool)> {
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        return node.get("id").and_then(|v| v.as_u64()).map(|id| {
            (
                id,
                node.get("pid").and_then(|v| v.as_u64()) == Some(u64::from(std::process::id())),
            )
        });
    }
    ["nodes", "floating_nodes"]
        .into_iter()
        .filter_map(|name| node.get(name).and_then(|v| v.as_array()))
        .flatten()
        .find_map(focused_sway)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn restores_mapping_even_if_input_fails() {
        let restored = Cell::new(false);
        let result = restore_after(
            || Err::<(), _>("injection failed"),
            || {
                restored.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Err("injection failed"));
        assert!(restored.get());
        assert_eq!(
            restore_after(|| Ok::<_, &str>(()), || Err("restore failed")),
            Err("restore failed")
        );
    }

    #[test]
    fn desktop_layouts_and_focus_are_decoded() {
        assert_eq!(
            source_layout("us+dvorak"),
            Some(("us".into(), "dvorak".into()))
        );
        assert_eq!(
            source_layout("keyboard-us-intl"),
            Some(("us".into(), "intl".into()))
        );
        assert_eq!(source_layout("de"), Some(("de".into(), String::new())));
        let tree = serde_json::json!({"nodes":[{"focused":false,"nodes":[{"focused":true,"id":9,"pid":17}]}]});
        assert_eq!(focused_sway(&tree), Some((9, false)));
    }
}
