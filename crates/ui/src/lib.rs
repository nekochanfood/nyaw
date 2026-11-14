//! Layer 5: UI 層。各オーバーレイパネルを GPUI の非表示ウィンドウで描画する。
//! レーザーイベントを GPUI のマウス入力に変換し、描画を読み戻してカーネルへ返す。

pub use kernel::rendering::gpui::{
    Frame, capture, collect, read_back, render_image, set_render_lod,
};
mod cue;
pub mod keyboard;
mod link;
pub mod mirror;
mod panel_window;
pub mod prefs;
pub use panel_window::{open_panel_window, panel_window_options};

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
    PointerMode, Pose, Setting, Settings, ThemeMode, Vec2, VrBackend,
};
use profiler::{Profiler, ProfilerView, Stages};

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

/// 打ちやすい位置に出すパネル（[`PanelSpec::typing`]。キーボード）の、開いた瞬間の頭から見た前への距離と、手が分からないときの
/// 高さ（m。手を軽く前に出したあたり）。左右は呼び出し元に合わせるので `x` は使わない。
const TYPING_AT: Vec3 = Vec3::new(0.0, -0.4, -0.8);
/// 入力も状態の変化も無いパネルを描き直す間隔。時計のようにビューが自分で描き直すものはこれで反映される。
// ponytail: GPUI はウィンドウの dirty を公開していないので推測で描く。公開されたら dirty なときだけにする。
pub(crate) const IDLE_REDRAW: Duration = Duration::from_millis(250);
/// 同時に開いておけるパネルの数（閉じたものは数えない）。OpenVR はランタイム全体で 128 枚までなので、
/// レーザーや他のアプリのオーバーレイの分を残す。ランタイムの上限（OpenXR のレイヤー数など）が小さければそちら。
/// 超えた分は開かずにエラーを出す。
pub const MAX_PANELS: usize = 64;
