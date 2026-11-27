//! 仮想キーボードから OS のフォアグラウンドアプリへの入力（デスクトップキャプチャのタッチと同様、UI はカーネルに委譲するのみ）。
//! フォアグラウンドアプリのレイアウト・IME の監視は OS への定期ポーリングのため、使用開始時に起動する。
use super::*;
use std::time::{Duration, Instant};
use subsystems::keyboard::{Layout, Stroke};
use subsystems::os::keyboard::{Output, Settings, chord};

impl<B: VrBackend> Kernel<B> {
    /// OS のフォアグラウンドアプリのキーボード状態（レイアウト・IME・ロックキー・入力先）。初回呼び出し時に監視を開始する。
    pub fn os_keyboard(&mut self) -> Settings {
        self.os_keyboard
            .get_or_insert_with(desktop::os_keyboard)
            .settings()
    }

    /// ミラーリング中のパネルからの入力は、対象ウィンドウがフォアグラウンドになってから送信する（別アプリへの誤入力を防止）。
    /// フォアグラウンドでなければウィンドウをフォアグラウンドに昇格させ、[`PENDING_TTL`] 間のみ順序付きで待機させる。送信先もそのウィンドウに固定するため、
    /// 配送までにフォアグラウンドが変わっても誤送信しない。
    pub(super) fn type_to_os(&mut self, panel: PanelId, stroke: Stroke, layout: Layout) {
        let settings = self.os_keyboard();
        let Some(output) = os_output(stroke, &layout, &settings) else {
            return;
        };
        let Some(window) = self.mirrored_window(panel) else {
            let watcher = self.os_keyboard.as_ref().expect("started above");
            if let Err(e) = watcher.send(settings.foreground, output) {
                logger::warn!("failed to send keys to the OS", error = e);
            }
            return;
        };
        if window != settings.foreground && !self.pending_os_input.iter().any(|k| k.panel == panel)
        {
            self.focus_desktop(panel);
        }
        if self.pending_os_input.len() < PENDING_MAX {
            self.pending_os_input.push(PendingKey {
                panel,
                window,
                output,
                since: Instant::now(),
            });
        }
        self.flush_os_input();
    }

    /// 待機中の入力を、フォアグラウンドになったものから順に送信する。タイムアウトまたはミラーリング解除されたウィンドウ宛てのものは破棄する。
    /// 同一パネルの入力は順序を保持する（先頭がフォアグラウンド待ちの間は後続も待機する）。
    pub(super) fn flush_os_input(&mut self) {
        if self.pending_os_input.is_empty() {
            return;
        }
        let foreground = self.os_keyboard().foreground;
        let (now, pending) = (Instant::now(), std::mem::take(&mut self.pending_os_input));
        let mut waiting: Vec<PanelId> = Vec::new();
        for key in pending {
            if now.saturating_duration_since(key.since) > PENDING_TTL
                || self.mirrored_window(key.panel) != Some(key.window)
            {
                logger::warn!(
                    "dropped OS input for a window that never came to the front",
                    panel = key.panel
                );
            } else if waiting.contains(&key.panel) || key.window != foreground {
                waiting.push(key.panel);
                self.pending_os_input.push(key);
            } else if let Some(watcher) = &self.os_keyboard
                && let Err(e) = watcher.send(key.window, key.output)
            {
                logger::warn!("failed to send keys to the OS", error = e);
            }
        }
    }
}

/// ミラーリング中のウィンドウがフォアグラウンドになるのを待機している OS 入力 1 件。
pub(super) struct PendingKey {
    panel: PanelId,
    window: u64,
    output: Output,
    since: Instant,
}

/// フォアグラウンド待機のタイムアウト。フォアグラウンド昇格要求は非同期で、判定も `os::keyboard::Watcher` のポーリング周期で更新される。
const PENDING_TTL: Duration = Duration::from_secs(1);
/// 待機入力の上限数（フォアグラウンドに昇格できないウィンドウへの蓄積を防止する）。
const PENDING_MAX: usize = 64;

/// 仮想キーボードの 1 操作を OS 送信形式に変換する。OS と同一レイアウトならキーコードを、異なれば文字列を送信する（OS 側レイアウトでの文字化けを防止）。
/// 異なるレイアウトでの変換中（未確定文字）は送信しない。
fn os_output(stroke: Stroke, layout: &Layout, settings: &Settings) -> Option<Output> {
    let native = settings
        .layout
        .as_ref()
        .is_some_and(|(name, variant)| *name == layout.name && *variant == layout.variant);
    if !native
        && !stroke.shortcut()
        && !stroke.text.is_empty()
        && !stroke.text.chars().any(char::is_control)
    {
        Some(Output::Text(stroke.text))
    } else if !native && stroke.composing {
        None
    } else {
        Some(Output::Keys(chord(&stroke.held, stroke.code)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subsystems::os::keyboard::Watcher;

    #[test]
    fn same_layout_sends_keys_and_other_layouts_send_text() {
        let stroke = Stroke {
            code: 30,
            text: "a".into(),
            ..Stroke::default()
        };
        let us = Layout::new("us", "");
        let os = |layout: Option<(&str, &str)>| Settings {
            layout: layout.map(|(n, v)| (n.into(), v.into())),
            ..Settings::default()
        };
        assert!(matches!(
            os_output(stroke.clone(), &us, &os(Some(("us", "")))),
            Some(Output::Keys(_))
        ));
        assert!(matches!(
            os_output(stroke.clone(), &us, &os(Some(("jp", "")))),
            Some(Output::Text(t)) if t == "a"
        ));
        let composing = Stroke {
            text: String::new(),
            composing: true,
            ..stroke
        };
        assert!(os_output(composing, &us, &os(None)).is_none());
    }

    struct FakeOs {
        foreground: std::sync::Arc<std::sync::atomic::AtomicU64>,
        sent: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl subsystems::os::keyboard::Host for FakeOs {
        fn settings(&mut self) -> Settings {
            Settings {
                foreground: self.foreground.load(std::sync::atomic::Ordering::SeqCst),
                ..Settings::default()
            }
        }
        fn send(&mut self, _: Output) -> std::io::Result<()> {
            self.sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    /// ミラーリング中パネルからの入力は、フォアグラウンドでなければウィンドウを昇格させて待機し、フォアグラウンドになったら順に送信する。
    #[test]
    fn mirrored_window_input_waits_for_the_foreground_then_resends_in_order() {
        use std::sync::{Arc, atomic::AtomicU64, atomic::AtomicUsize, atomic::Ordering};
        let mut kernel = Kernel::with_backend(vr::mock::MockBackend::default(), Default::default());
        let panel = kernel
            .add_panel("mirror", Pose::IDENTITY, Vec2::ONE, (4, 2))
            .unwrap();
        let (stream, control) = desktop::mock::window_stream(77);
        kernel.set_desktop_stream(panel, stream).unwrap();
        let (foreground, sent) = (Arc::new(AtomicU64::new(9)), Arc::new(AtomicUsize::new(0)));
        kernel.os_keyboard = Some(Watcher::with_host(FakeOs {
            foreground: foreground.clone(),
            sent: sent.clone(),
        }));
        let key = || Stroke {
            code: 30,
            text: "a".into(),
            ..Stroke::default()
        };
        let settle = || std::thread::sleep(std::time::Duration::from_millis(400));
        kernel.os_keyboard();
        settle();
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        settle();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            0,
            "フォアグラウンドでない間は送信しない"
        );
        assert_eq!(control.focuses(), 1, "フォアグラウンド昇格要求は 1 回のみ");
        foreground.store(77, Ordering::SeqCst);
        settle();
        kernel.flush_os_input();
        settle();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            2,
            "フォアグラウンドになったら待機分を送信する"
        );
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        settle();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            3,
            "フォアグラウンドなら即時送信する"
        );
        assert_eq!(control.focuses(), 0);
    }

    /// フォアグラウンドに昇格しないままミラーリング対象ウィンドウが変更された場合、待機中の入力は破棄する（別ウィンドウへの誤入力を防止）。
    #[test]
    fn pending_input_is_dropped_when_the_panel_stops_mirroring_that_window() {
        use std::sync::{Arc, atomic::AtomicU64, atomic::AtomicUsize, atomic::Ordering};
        let mut kernel = Kernel::with_backend(vr::mock::MockBackend::default(), Default::default());
        let panel = kernel
            .add_panel("mirror", Pose::IDENTITY, Vec2::ONE, (4, 2))
            .unwrap();
        let (stream, _control) = desktop::mock::window_stream(77);
        kernel.set_desktop_stream(panel, stream).unwrap();
        let (foreground, sent) = (Arc::new(AtomicU64::new(9)), Arc::new(AtomicUsize::new(0)));
        kernel.os_keyboard = Some(Watcher::with_host(FakeOs {
            foreground: foreground.clone(),
            sent: sent.clone(),
        }));
        kernel.type_to_os(
            panel,
            Stroke {
                code: 30,
                text: "a".into(),
                ..Stroke::default()
            },
            Layout::new("us", ""),
        );
        let (other, _other) = desktop::mock::window_stream(88);
        kernel.set_desktop_stream(panel, other).unwrap();
        foreground.store(88, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(400));
        kernel.flush_os_input();
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(sent.load(Ordering::SeqCst), 0);
        assert!(kernel.pending_os_input.is_empty());
    }
}
