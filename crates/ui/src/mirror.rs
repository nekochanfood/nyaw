//! ミラーリング画面（配信を映すパネル）の、映像が出ていないときの表示。
//! どの状態も、上下左右中央に縦へ「アイコン・タイトル・（あれば）理由・（必要なら）ボタン」。
//! 読み込み中はスピナー、エラーは X のアイコン、切断に近い状態はコンセントが抜けたアイコン。
//! ダッシュボードの実物と Testing の確認画面が同じ見た目になるよう、ここに置く。

use design::{Theme, components, icons};
use gpui::{AnyElement, App, Div, IntoElement, SharedString};
use kernel::desktop::{Error, Reason, Status};

/// 表示の一辺（px）。スピナーとアイコンはこの枠に収まる。
pub const ICON_PX: f32 = 48.0;

/// 状態の画面に出すボタン。押したときの処理（映し直す）は呼び出し側が足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorAction {
    Retry,
    Restore,
}

impl MirrorAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Retry => "mirror-retry",
            Self::Restore => "mirror-restore",
        }
    }

    pub fn label(self, cx: &App) -> SharedString {
        i18n::t(
            cx,
            match self {
                Self::Retry => "desktop.retry",
                Self::Restore => "desktop.restore",
            },
        )
    }
}

/// ミラーリング画面の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorState {
    /// 起動したアプリのウィンドウを待っている。
    Launching,
    /// 待ったがウィンドウが出なかった。
    LaunchFailed,
    Starting,
    Streaming,
    Minimized,
    Closed,
    Failed(Error),
}

impl MirrorState {
    /// 配信の状態（まだ何も届いていなければ `None`）から。
    pub fn from_status(status: Option<Status>) -> Self {
        match status {
            None | Some(Status::Starting) => Self::Starting,
            Some(Status::Streaming) => Self::Streaming,
            Some(Status::Minimized) => Self::Minimized,
            Some(Status::Closed) => Self::Closed,
            Some(Status::Failed(error)) => Self::Failed(error),
        }
    }

    /// この状態で出すボタン。閉じた・失敗は再試行、最小化は最小化を戻して映し直す。
    pub fn action(&self) -> Option<MirrorAction> {
        match self {
            Self::Closed | Self::Failed(_) => Some(MirrorAction::Retry),
            Self::Minimized => Some(MirrorAction::Restore),
            _ => None,
        }
    }

    pub fn title(&self, cx: &App) -> SharedString {
        i18n::t(
            cx,
            match self {
                Self::Launching => "desktop.launching",
                Self::LaunchFailed => "desktop.launch_failed",
                Self::Starting => "desktop.starting",
                Self::Streaming => "desktop.streaming",
                Self::Minimized => "desktop.minimized",
                Self::Closed => "desktop.closed",
                Self::Failed(_) => "desktop.failed_title",
            },
        )
    }

    /// 失敗の理由（ローカライズ済み）。分類できないものは元のメッセージをそのまま出す。
    pub fn reason(&self, cx: &App) -> Option<SharedString> {
        let Self::Failed(error) = self else {
            return None;
        };
        Some(match error.reason() {
            Reason::WindowUnavailable => i18n::t(cx, "desktop.reason.window_unavailable"),
            Reason::Unsupported => i18n::t(cx, "desktop.reason.unsupported"),
            Reason::NoPixels => i18n::t(cx, "desktop.reason.no_pixels"),
            Reason::InvalidBounds => i18n::t(cx, "desktop.reason.invalid_bounds"),
            Reason::Graphics => i18n::t(cx, "desktop.reason.graphics"),
            Reason::Other => error.to_string().into(),
        })
    }

    fn icon(&self, theme: &Theme) -> AnyElement {
        match self {
            Self::Launching | Self::Starting => {
                components::spinner(theme, ICON_PX).into_any_element()
            }
            Self::LaunchFailed | Self::Failed(_) => {
                components::status_icon(&icons::X_CIRCLE, ICON_PX).into_any_element()
            }
            Self::Streaming => components::status_icon(&icons::MONITOR, ICON_PX).into_any_element(),
            Self::Minimized | Self::Closed => {
                components::status_icon(&icons::PLUGS, ICON_PX).into_any_element()
            }
        }
    }
}

/// `state` の画面。ボタン（再試行など）は呼び出し側が `action` で渡す。
pub fn mirror_screen(
    theme: &Theme,
    state: &MirrorState,
    action: Option<AnyElement>,
    cx: &App,
) -> Div {
    components::status_screen(
        theme,
        state.icon(theme),
        state.title(cx),
        state.reason(cx),
        action,
    )
}
