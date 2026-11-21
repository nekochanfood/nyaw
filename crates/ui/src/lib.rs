//! Layer 4: UI 層。各オーバーレイパネルを GPUI の非表示ウィンドウで描画し、
//! レーザーイベントを GPUI のマウス入力へ変換して、描画結果をカーネルに返す。

pub use kernel::rendering::gpui::{
    Frame, capture, collect, read_back, render_image, set_render_lod,
};
mod cue;
pub mod keyboard;
mod link;
pub mod mirror;
pub mod notification;
mod panel_window;
pub mod prefs;
mod quit;
pub mod swipe;
pub use panel_window::{open_panel_window, panel_window_options};
pub use quit::{QuitDialog, quit_dialog};

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{
    AnyView, AnyWindowHandle, App, Context, Entity, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent,
    TouchPhase, Window, div, prelude::*, px, rgb,
};
use kernel::subsystems::input::ClickButton;
use kernel::vr::{Hand, Quat, Vec3};
use kernel::{
    Anchor, Device, Kernel, Language, PanelCommand, PanelId, PanelStatus, Pointer, PointerEvent,
    PointerMode, Pose, Request, Setting, Settings, ThemeMode, Vec2, VrBackend,
};
use profiler::{Profiler, ProfilerView, Stages};

mod attach;
mod control;
mod drag;
mod nav_bar;
mod pointer;
mod profiler_panel;
mod run;
mod spec;

pub use control::*;
pub use drag::*;
pub use nav_bar::*;
pub use pointer::*;
use profiler_panel::*;
pub use run::*;
pub use spec::*;

#[cfg(test)]
mod test_util;

const PANEL_PX: (u32, u32) = (800, 600);

/// 入力用パネル（[`PanelSpec::typing`]）を配置する基準点。開いた瞬間の頭からの前方距離と、
/// コントローラ不検出時の高さ（m）。左右は呼び出し元に合わせるため `x` は未使用。
const TYPING_AT: Vec3 = Vec3::new(0.0, -0.4, -0.8);
/// 入力も状態変化もないパネルの再描画間隔。時計のように自発的に更新するビューはこの周期で反映される。
// ponytail: GPUI がウィンドウの dirty フラグを公開していないため推測で再描画する。公開されれば dirty 時のみに限定可能。
pub(crate) const IDLE_REDRAW: Duration = Duration::from_millis(250);
/// 同時に開けるパネルの上限（閉じたパネルは除外）。OpenVR ランタイム全体の上限 128 枚から
/// レーザーや他アプリのオーバーレイ分を確保する。ランタイム固有の上限がこれより小さい場合はそちらを適用し、
/// 超過分は開かずエラーとする。
pub const MAX_PANELS: usize = 64;
