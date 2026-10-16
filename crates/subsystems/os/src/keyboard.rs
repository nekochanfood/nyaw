//! フォアグラウンドの配列・IME の監視と OS 入力。ディスク / IPC / 待ちは専用スレッドに閉じる。
use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

#[cfg(windows)]
#[path = "keyboard/windows.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "keyboard/linux.rs"]
mod platform;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub layout: Option<(String, String)>,
    pub input_method: String,
    pub layout_description: Option<String>,
    /// OS の入力先。0 は取得できなかった場合。
    pub foreground: u64,
    pub own_process: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Linux evdev の番号（XKB の +8 を含まない）。
    pub code: u16,
    pub down: bool,
}

#[derive(Clone, Debug)]
pub enum Output {
    Keys(Vec<KeyEvent>),
    Text(String),
}

enum Command {
    Send { target: u64, output: Output },
}

/// 監視・配送を同じ所有者で実行する。テストは実 OS の代わりに記録するホストを渡せる。
pub trait Host: Send + 'static {
    fn settings(&mut self) -> Settings;
    fn send(&mut self, output: Output) -> io::Result<()>;
}

impl Host for platform::Backend {
    fn settings(&mut self) -> Settings {
        self.settings()
    }
    fn send(&mut self, output: Output) -> io::Result<()> {
        self.send(output)
    }
}

pub struct Watcher {
    state: Arc<Mutex<Settings>>,
    commands: mpsc::Sender<Command>,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Watcher {
    pub fn new() -> Self {
        Self::start(|| Box::new(platform::Backend::new()))
    }

    pub fn with_host(host: impl Host) -> Self {
        Self::start(move || Box::new(host))
    }

    fn start(create: impl FnOnce() -> Box<dyn Host> + Send + 'static) -> Self {
        let state = Arc::new(Mutex::new(Settings::default()));
        let shared = state.clone();
        let (commands, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut backend = create();
            let mut last_error = None;
            loop {
                let mut settings = backend.settings();
                settings.error = last_error.clone().or(settings.error);
                *shared.lock().unwrap() = settings;
                match receiver.recv_timeout(Duration::from_millis(150)) {
                    Ok(Command::Send { target, output }) => {
                        let settings = backend.settings();
                        // 遅延中に他のアプリへ切り替わった操作は配送しない。
                        let result =
                            if target == 0 || target != settings.foreground || settings.own_process
                            {
                                Err(io::Error::other(
                                    "OS input target changed or is unavailable",
                                ))
                            } else {
                                backend.send(output)
                            };
                        let mut updated = settings;
                        last_error = result.err().map(|e| e.to_string());
                        updated.error = last_error.clone().or(updated.error);
                        *shared.lock().unwrap() = updated;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { state, commands }
    }

    pub fn settings(&self) -> Settings {
        self.state.lock().unwrap().clone()
    }

    pub fn send(&self, target: u64, output: Output) -> io::Result<()> {
        self.commands
            .send(Command::Send { target, output })
            .map_err(|_| io::Error::other("keyboard worker stopped"))
    }
}

/// 押下と解放を一括で送り、パネルを閉じても OS に押しっぱなしを残さない。
pub fn chord(held: &[u16], key: u16) -> Vec<KeyEvent> {
    let locks: Vec<_> = held
        .iter()
        .copied()
        .filter(|k| matches!(k, 58 | 69 | 70))
        .collect();
    let mut keys: Vec<_> = held
        .iter()
        .copied()
        .filter(|k| !matches!(k, 58 | 69 | 70))
        .collect();
    if !keys.contains(&key) {
        keys.push(key);
    }
    let tap = |&code| {
        [
            KeyEvent { code, down: true },
            KeyEvent { code, down: false },
        ]
    };
    locks
        .iter()
        .flat_map(tap)
        .chain(
            keys.iter()
                .map(|&code| KeyEvent { code, down: true })
                .chain(
                    keys.iter()
                        .rev()
                        .map(|&code| KeyEvent { code, down: false }),
                ),
        )
        .chain(locks.iter().rev().flat_map(tap))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    struct RecordingHost {
        settings: Arc<Mutex<Settings>>,
        output: mpsc::Sender<Output>,
    }
    impl Host for RecordingHost {
        fn settings(&mut self) -> Settings {
            self.settings.lock().unwrap().clone()
        }
        fn send(&mut self, output: Output) -> io::Result<()> {
            self.output.send(output).unwrap();
            Ok(())
        }
    }

    #[test]
    fn delayed_input_cannot_reach_a_different_or_own_process() {
        let settings = Arc::new(Mutex::new(Settings {
            foreground: 7,
            ..Settings::default()
        }));
        let (output, received) = mpsc::channel();
        let watcher = Watcher::with_host(RecordingHost {
            settings: settings.clone(),
            output,
        });
        let wait = |predicate: &dyn Fn(&Settings) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !predicate(&watcher.settings()) {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        };
        wait(&|s| s.foreground == 7);
        settings.lock().unwrap().foreground = 9;
        watcher.send(7, Output::Text("stale".into())).unwrap();
        wait(&|s| s.error.is_some());
        assert!(received.try_recv().is_err());
        watcher.send(9, Output::Text("current".into())).unwrap();
        assert!(
            matches!(received.recv_timeout(Duration::from_secs(1)).unwrap(),Output::Text(text) if text=="current")
        );
        wait(&|s| s.error.is_none());
        settings.lock().unwrap().own_process = true;
        watcher.send(9, Output::Text("own process".into())).unwrap();
        wait(&|s| s.error.is_some());
        assert!(received.try_recv().is_err());
    }
    #[test]
    fn chord_releases_in_reverse_order_without_duplicates() {
        assert_eq!(
            chord(&[29, 42], 30),
            vec![
                KeyEvent {
                    code: 29,
                    down: true
                },
                KeyEvent {
                    code: 42,
                    down: true
                },
                KeyEvent {
                    code: 30,
                    down: true
                },
                KeyEvent {
                    code: 30,
                    down: false
                },
                KeyEvent {
                    code: 42,
                    down: false
                },
                KeyEvent {
                    code: 29,
                    down: false
                }
            ]
        );
        assert_eq!(chord(&[15], 15).len(), 2);
        let caps = chord(&[58], 30);
        assert_eq!(
            caps.iter().map(|e| (e.code, e.down)).collect::<Vec<_>>(),
            [
                (58, true),
                (58, false),
                (30, true),
                (30, false),
                (58, true),
                (58, false)
            ]
        );
    }
}
