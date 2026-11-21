//! Linux: キー配列（XKB）の検出のみ。OS への入力送信は未対応（送信は常にエラー）。
use super::{Output, Settings};
use std::io;

pub struct Backend {
    settings: Settings,
}

impl Backend {
    /// 配列は起動時に一度だけ調べる（`settings` は監視スレッドから 150ms ごとに呼ばれるため、外部コマンドを毎回実行しない）。
    pub fn new() -> Self {
        let layout = detect_layout();
        Self {
            settings: Settings {
                input_method: match &layout {
                    Some((name, variant)) if variant.is_empty() => format!("XKB {name}"),
                    Some((name, variant)) => format!("XKB {name}({variant})"),
                    None => "XKB unknown".into(),
                },
                layout,
                // ponytail: 入力送信（uinput 等）を実装するまで、送信先（フォアグラウンド）は常に「取得不可」。
                foreground: 0,
                error: Some("sending keys to other apps is not supported on Linux".into()),
                ..Settings::default()
            },
        }
    }

    pub fn settings(&mut self) -> Settings {
        self.settings.clone()
    }

    pub fn send(&mut self, _output: Output) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sending keys to other apps is not supported on Linux",
        ))
    }
}

/// XKB の（配列, バリアント）。`XKB_DEFAULT_LAYOUT` > `setxkbmap -query`（X11 / XWayland） > `localectl status` の順に探す。
fn detect_layout() -> Option<(String, String)> {
    if let Ok(layout) = std::env::var("XKB_DEFAULT_LAYOUT")
        && !layout.is_empty()
    {
        let variant = std::env::var("XKB_DEFAULT_VARIANT").unwrap_or_default();
        return first_layout(&layout, &variant);
    }
    let run = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    run("setxkbmap", &["-query"])
        .and_then(|out| parse_fields(&out, "layout", "variant"))
        .or_else(|| {
            run("localectl", &["status"])
                .and_then(|out| parse_fields(&out, "X11 Layout", "X11 Variant"))
        })
}

/// `key: value` 形式の出力から配列とバリアントを読む。`(unset)` は未設定。
fn parse_fields(out: &str, layout_key: &str, variant_key: &str) -> Option<(String, String)> {
    let field = |key: &str| {
        out.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
            .filter(|v| !v.is_empty() && v != "(unset)")
    };
    first_layout(&field(layout_key)?, &field(variant_key).unwrap_or_default())
}

/// 複数配列（`jp,us`）は先頭（既定の配列）を使う。バリアントも同じ位置のもの。
fn first_layout(layouts: &str, variants: &str) -> Option<(String, String)> {
    let name = layouts.split(',').next()?.trim();
    if name.is_empty() {
        return None;
    }
    let variant = variants.split(',').next().unwrap_or("").trim();
    Some((name.to_owned(), variant.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_setxkbmap_query() {
        let out = "rules:      evdev\nmodel:      pc105\nlayout:     jp\nvariant:    OADG109A\n";
        assert_eq!(
            parse_fields(out, "layout", "variant"),
            Some(("jp".into(), "OADG109A".into()))
        );
    }

    #[test]
    fn reads_localectl_and_ignores_unset() {
        let out = "System Locale: LANG=en_GB.UTF-8\n    VC Keymap: jp106\n   X11 Layout: (unset)\n";
        assert_eq!(parse_fields(out, "X11 Layout", "X11 Variant"), None);
        let out = "   X11 Layout: us,jp\n  X11 Variant: dvorak,\n";
        assert_eq!(
            parse_fields(out, "X11 Layout", "X11 Variant"),
            Some(("us".into(), "dvorak".into()))
        );
    }

    #[test]
    fn sending_is_unsupported() {
        let mut backend = Backend {
            settings: Settings::default(),
        };
        assert_eq!(
            backend.send(Output::Text("a".into())).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
