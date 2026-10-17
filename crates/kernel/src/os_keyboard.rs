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

    pub(super) fn type_to_os(&mut self, stroke: Stroke, layout: Layout) {
        let settings = self.os_keyboard();
        let Some(output) = os_output(stroke, &layout, &settings) else {
            return;
        };
        let watcher = self.os_keyboard.as_ref().expect("started above");
        if let Err(e) = watcher.send(settings.foreground, output) {
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
}
