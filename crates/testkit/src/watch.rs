//! A window for watching E2E tests. When the environment variable `NYAW_E2E_WATCH` is set, shows the panels the kit draws every frame
//! in one OS window per panel. The value is the display time per frame (ms; 100 if not a number).
//!
//! ```text
//! NYAW_E2E_WATCH=300 cargo test -p testkit --test testing -- --test-threads=1
//! ```
//!
//! What you see is the image painted by [`crate::raster`] (rectangles only; no text).

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

    /// Shows the image of panel `panel`. A window that has been closed is not reopened.
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
                    .map_err(|e| logger::warn!("failed to open E2E window", error = e))
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
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]])),
        );
        let _ = window.update_with_buffer(&self.buf, frame.width as usize, frame.height as usize);
    }

    /// Call once after all panels have been shown.
    pub(crate) fn wait(&self) {
        std::thread::sleep(self.delay);
    }
}

/// The test name (the test harness runs each test on a thread with that name).
fn test_name() -> String {
    std::thread::current().name().unwrap_or("e2e").to_owned()
}
