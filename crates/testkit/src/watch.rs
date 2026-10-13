//! E2E を目で見るための窓。環境変数 `NYAW_E2E_WATCH` があるとき、キットが毎フレーム描いたパネルを
//! パネルごとの OS のウィンドウに出す。値は 1 フレームの表示時間（ms、数値でなければ 100）。
//!
//! ```text
//! NYAW_E2E_WATCH=300 cargo test -p testkit --test testing -- --test-threads=1
//! ```
//!
//! 見えるのは [`crate::raster`] が塗った絵（矩形だけ。文字は出ない）。

use std::time::Duration;

use minifb::{Window, WindowOptions};
use ui::Frame;

pub(crate) struct Watch {
    delay: Duration,
    windows: Vec<Option<Window>>,
    buf: Vec<u32>,
}

impl Watch {
    pub(crate) fn from_env() -> Option<Self> {
        let v = std::env::var("NYAW_E2E_WATCH").ok()?;
        Some(Self {
            delay: Duration::from_millis(v.parse().unwrap_or(100)),
            windows: Vec::new(),
            buf: Vec::new(),
        })
    }

    /// パネル `panel` の絵を出す。閉じられたウィンドウは開き直さない。
    pub(crate) fn show(&mut self, panel: usize, frame: &Frame) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        while self.windows.len() <= panel {
            let i = self.windows.len();
            let title = format!("nyaw e2e: {} / panel {i}", test_name());
            let opts = WindowOptions {
                resize: true,
                ..Default::default()
            };
            self.windows.push(
                Window::new(&title, frame.width as usize, frame.height as usize, opts)
                    .map_err(|e| logger::warn!("E2E の窓を開けない", error = e))
                    .ok(),
            );
        }
        let Some(window) = self.windows[panel].as_mut().filter(|w| w.is_open()) else {
            return;
        };
        self.buf.clear();
        self.buf.extend(
            frame
                .rgba
                .chunks_exact(4)
                .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]])),
        );
        let _ = window.update_with_buffer(&self.buf, frame.width as usize, frame.height as usize);
    }

    /// 全パネルを出し終えたあとに 1 回呼ぶ。
    pub(crate) fn wait(&self) {
        std::thread::sleep(self.delay);
    }
}

/// テスト名（テストハーネスはテストごとにその名前のスレッドで流す）。
fn test_name() -> String {
    std::thread::current().name().unwrap_or("e2e").to_owned()
}
