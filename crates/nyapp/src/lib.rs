//! Layer 6 横: ニャプリ（nyapp）の基盤。構想と思想は `AGENT.md`。
//!
//! - [`manifest`] — マニフェスト（逆ドメインの ID・名前・権限）
//! - [`package`] — `.nyapp` パッケージ（zip）
//! - [`installed`] — 入っているニャプリ（`data_dir()/nyapps/<id>.nyapp`）と保存領域
//! - [`sandbox`] — wasm を wasmi で動かすサンドボックスとホスト関数
//! - [`view`] — 画面の木を `design` の部品で描くパネル・強制終了のダイアログ
//! - [`activity`] — 進行中の作業の知らせ（ダッシュボードのバーに出る。閉じても残る）
//! - [`icon`] — パッケージのアイコンと、無いときの代わりのアイコン
//! - [`update`] — 更新情報から最新版を確かめ、落として入れ替える
//! - [`system`] — システムのニャプリ（本体に組み込まれた wrist・ダッシュボード・ランチャー・デバッグ・設定）
//! - [`Running`] — 起動中のニャプリの表（パネル → ID）

pub mod activity;
pub mod icon;
pub mod installed;
pub mod manifest;
pub mod package;
pub mod sandbox;
pub mod system;
pub mod update;
pub mod view;

use std::collections::HashMap;
use std::fmt;

use gpui::App;
use kernel::PanelId;
use ui::PanelControl;

pub use installed::Installed;
pub use manifest::{Manifest, Permission};
pub use nyapp_sdk::Activity;
pub use sandbox::Sandbox;
pub use view::{NyappView, spec};

/// ニャプリまわりの失敗（読めない・壊れている・trap した など）。理由の文だけを持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

macro_rules! from_error {
    ($($t:ty),*) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Self(e.to_string())
            }
        }
    )*};
}

from_error!(
    std::io::Error,
    zip::result::ZipError,
    toml::de::Error,
    serde_json::Error,
    wasmi::Error,
    wasmi::errors::LinkerError,
    ureq::Error
);

/// 起動中のニャプリ: パネル → ニャプリの ID（システムのものも含む）。Android の ActivityManager の役。
#[derive(Default)]
pub struct Running(HashMap<PanelId, String>);

impl gpui::Global for Running {}

/// パネル `panel` をニャプリ `id` のものとして覚える（開いたときに呼ぶ）。
pub fn opened(id: &str, panel: PanelId, cx: &mut App) {
    cx.default_global::<Running>()
        .0
        .insert(panel, id.to_owned());
}

/// ニャプリ `id` の、閉じていないパネル（いくつかあれば最初に開いたもの）。
pub fn panel_of(id: &str, cx: &App) -> Option<PanelId> {
    let closed = |panel: &PanelId| {
        cx.try_global::<PanelControl>()
            .and_then(|c| c.panels.get(panel))
            .is_some_and(|p| p.status.closed)
    };
    cx.try_global::<Running>()?
        .0
        .iter()
        .filter(|(panel, app)| app.as_str() == id && !closed(panel))
        .map(|(panel, _)| *panel)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開いたパネルはニャプリの ID で引け、閉じたものは引けない。
    #[gpui::test]
    fn running_table_forgets_closed_panels(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            opened(system::WRIST.id, 0, cx);
            opened("net.bettaworx.clock", 4, cx);
            opened("net.bettaworx.clock", 7, cx);
            assert_eq!(panel_of(system::WRIST.id, cx), Some(0));
            assert_eq!(panel_of("net.bettaworx.clock", cx), Some(4));
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .closed = true;
            assert_eq!(panel_of("net.bettaworx.clock", cx), Some(7));
            assert_eq!(panel_of("net.bettaworx.calculator", cx), None);
        });
    }
}
