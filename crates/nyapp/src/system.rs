//! システムのニャプリ。Android の SystemUI と同じく、ほかのニャプリと同じ ID の体系・起動中の表（[`crate::Running`]）に
//! 載るが、本体に組み込まれ（GPUI で直接描くネイティブのもの）、サンドボックスの外でカーネルの状態に触れ、消せない。
//! 同じ ID のパッケージは入れられない。

/// システムのニャプリ 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct System {
    pub id: &'static str,
    /// 名前（i18n のキー）。
    pub name: &'static str,
}

/// 左手首の腕時計（`crates/wrist`）。常駐し、ダッシュボードを出し入れする。
pub const WRIST: System = System {
    id: "net.bettaworx.wrist",
    name: "nyapp.system.wrist",
};
/// ダッシュボードのバーと、そこにはまるクイック設定・再生中（`crates/dashboard`）。
pub const DASHBOARD: System = System {
    id: "net.bettaworx.dashboard",
    name: "nyapp.system.dashboard",
};
/// ランチャー（`crates/dashboard` の `Page::Launcher`）。デスクトップのアプリとニャプリを並べて開く。
pub const LAUNCHER: System = System {
    id: "net.bettaworx.launcher",
    name: "nyapp.system.launcher",
};
/// デバッグパネル（今は `crates/testing` の Testing）。
pub const DEBUG: System = System {
    id: "net.bettaworx.debug",
    name: "nyapp.system.debug",
};
/// 設定画面（今は `crates/testing` の設定画面。中身はダミー）。
pub const SETTINGS: System = System {
    id: "net.bettaworx.settings",
    name: "nyapp.system.settings",
};

pub const ALL: [System; 5] = [WRIST, DASHBOARD, LAUNCHER, DEBUG, SETTINGS];

pub fn is_system(id: &str) -> bool {
    ALL.iter().any(|s| s.id == id)
}
