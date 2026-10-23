//! 仮想キーボードから OS の前面のアプリへの入力（デスクトップ配信のタッチと同じく、UI はカーネルに頼むだけ）。
//! 前面のアプリの配列・IME の監視は OS へ定期的に問い合わせるので、使い始めたときに起こす。
use super::*;
use std::time::{Duration, Instant};
use subsystems::keyboard::{Layout, Stroke};
use subsystems::os::keyboard::{Output, Settings, Watcher, chord};

impl<B: VrBackend> Kernel<B> {
    /// OS の前面のアプリのキーボードの状態（配列・IME・ロック・入力先）。最初に呼んだときに監視を始める。
    pub fn os_keyboard(&mut self) -> Settings {
        self.os_keyboard.get_or_insert_with(Watcher::new).settings()
    }

    /// ウィンドウを映すパネルからの入力は、そのウィンドウが前面になってから送る（別のアプリへ入らないように）。
    /// 前面でなければウィンドウを前に出し、[`PENDING_TTL`] の間だけ順に待たせる。送る先もそのウィンドウに固定するので、
    /// 配送までに前面が移っても入らない。
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

    /// 待たせている入力を、前面になったものから順に送る。時間切れや、もう映していないウィンドウ宛てのものは捨てる。
    /// 同じパネルの入力は順序を守る（先頭が前面待ちの間は後ろも待つ）。
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

/// 映しているウィンドウが前面になるのを待っている、OS への入力一つ。
pub(super) struct PendingKey {
    panel: PanelId,
    window: u64,
    output: Output,
    since: Instant,
}

/// 前面になるのを待つ長さ。前に出す頼みは非同期で、前面の判定も [`Watcher`] の周期で更新される。
const PENDING_TTL: Duration = Duration::from_secs(1);
/// 待たせる入力の上限（出られないウィンドウに溜め続けない）。
const PENDING_MAX: usize = 64;

/// 仮想キーボードの一操作を OS へ送る形にする。OS と同じ配列ならキーを、違えば文字を送る（OS 側の配列で別の文字にならない）。
/// 違う配列の変換中（文字が決まっていない）は送らない。
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

    /// ウィンドウを映すパネルからの入力は、前面でなければウィンドウを前に出して待たせ、前面になったら順に送る。
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
        assert_eq!(sent.load(Ordering::SeqCst), 0, "前面でない間は送らない");
        assert_eq!(control.focuses(), 1, "前に出す頼みは一度だけ");
        foreground.store(77, Ordering::SeqCst);
        settle();
        kernel.flush_os_input();
        settle();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            2,
            "前面になったら待たせた分を送る"
        );
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        settle();
        assert_eq!(sent.load(Ordering::SeqCst), 3, "前面なら待たずに送る");
        assert_eq!(control.focuses(), 0);
    }

    /// 前面にならないまま映しているウィンドウが変わったら、待たせた入力は捨てる（別のウィンドウへ入らない）。
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
