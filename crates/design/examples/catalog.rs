//! design のカタログを単独のウィンドウで開く: `cargo run -p design --example catalog`

use design::catalog::Catalog;
use gpui::{AppContext, WindowOptions};

fn main() {
    gpui_platform::application().run(|cx| {
        cx.set_global(design::Motion::default());
        design::font::install(cx);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Catalog::default())
        })
        .expect("open window");
        cx.activate(true);
    });
}
