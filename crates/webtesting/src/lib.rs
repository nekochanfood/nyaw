//! Layer 6 画面（Web）: Testing の Web 版。Web UI 層（`webui`）の機能を 1 ページで試せる。
//! レーザー（VR）でもマウス（novr）でも同じ React 画面（`web/Testing.tsx`）が動く。
//!
//! E2E のため、Tauri のイベント `webtesting:probe` を受けると、状態と要素の位置を `webtesting:state` で返す。

use kernel::vr::{Quat, Vec3};
use kernel::{Pose, Vec2};
use webui::{Page, WebPanelSpec};

/// 問い合わせ（Rust → ページ）。中身は返事に添える番号。
pub const PROBE_EVENT: &str = "webtesting:probe";
/// 返事（ページ → Rust）。[`PROBE_EVENT`] の番号と、状態・要素の位置。
pub const STATE_EVENT: &str = "webtesting:state";

/// 開いた瞬間の目の前 1m。
const IN_VIEW: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

pub fn page() -> Page {
    Page::new("webtesting")
}

/// 目の前に出す Testing（Web 版）のパネル。
pub fn spec() -> WebPanelSpec {
    WebPanelSpec::new(Pose::IDENTITY, page())
        .sized(Vec2::new(0.8, 0.6), (800, 600))
        .in_view(IN_VIEW)
}
