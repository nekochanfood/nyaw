//! 仮想キーボードから OS の前面のアプリへの入力（デスクトップ配信のタッチと同じく、UI はカーネルに頼むだけ）。
//! 前面のアプリの配列・IME の監視は OS へ定期的に問い合わせるので、使い始めたときに起こす。
use super::*;
use subsystems::keyboard::{Layout, Stroke};
use subsystems::os::keyboard::{Output, Settings, Watcher, chord};

impl<B: VrBackend> Kernel<B> {
    /// OS の前面のアプリのキーボードの状態（配列・IME・ロック・入力先）。最初に呼んだときに監視を始める。
    pub fn os_keyboard(&mut self) -> Settings {
        self.os_keyboard.get_or_insert_with(Watcher::new).settings()
    }

    /// ウィンドウを映すパネルからの入力は、そのウィンドウが前面のときだけ送る（別のアプリへ入らないように）。
    /// 前面でなければ入力は捨て、ウィンドウを前に出す。送る先もそのウィンドウに固定するので、配送までに前面が移っても入らない。
    pub(super) fn type_to_os(&mut self, panel: PanelId, stroke: Stroke, layout: Layout) {
        let settings = self.os_keyboard();
        let Some(output) = os_output(stroke, &layout, &settings) else {
            return;
        };
        let target = self.mirrored_window(panel);
        if let Some(window) = target
            && window != settings.foreground
        {
            logger::warn!(
                "mirrored window is not in the foreground; input dropped",
                panel = panel
            );
            self.focus_desktop(panel);
            return;
        }
        let watcher = self.os_keyboard.as_ref().expect("started above");
        if let Err(e) = watcher.send(target.unwrap_or(settings.foreground), output) {
            logger::warn!("failed to send keys to the OS", error = e);
        }
    }
}

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

    /// ウィンドウを映すパネルからの入力は、そのウィンドウが前面のときだけ OS へ送る。
    /// 前面でなければ捨てて、ウィンドウを前に出すよう頼む。
    #[test]
    fn mirrored_window_receives_keys_only_while_in_the_foreground() {
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
        let wait = |count: usize| {
            for _ in 0..200 {
                if sent.load(Ordering::SeqCst) == count {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // 余計に送られていないことも確かめるため、少し待つ。
            std::thread::sleep(std::time::Duration::from_millis(400));
            assert_eq!(sent.load(Ordering::SeqCst), count);
        };
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        wait(0);
        assert_eq!(control.focuses(), 1, "前面でなければウィンドウを前に出す");
        foreground.store(77, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(400));
        kernel.type_to_os(panel, key(), Layout::new("us", ""));
        wait(1);
        assert_eq!(control.focuses(), 0);
    }
}
