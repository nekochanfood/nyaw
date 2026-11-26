//! ミラーリング画面（配信を映すパネル）の、映像非表示時のプレースホルダー。
//! いずれの状態でも中央に縦方向で「アイコン・タイトル・理由（該当時）・ボタン（必要時）」を配置する。
//! 読み込み中はスピナー、エラーは X アイコン、切断系の状態はプラグアイコンを表示する。
//! ダッシュボードと Testing の確認画面で見た目を統一するため、共通モジュールとして配置する。

use design::{Theme, components, icons};
use gpui::{AnyElement, App, Div, IntoElement, SharedString};
use kernel::desktop::{Error, Reason, Status};

/// アイコン表示領域の一辺（px）。スピナーおよびアイコンはこの枠内に収まる。
pub const ICON_PX: f32 = 48.0;

/// 状態画面に表示するボタン。押下時の処理（再配信等）は呼び出し側が設定する。
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

/// ミラーリング画面の表示状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorState {
    /// 起動したアプリのウィンドウ出現を待機中。
    Launching,
    /// 待機したがウィンドウが検出されなかった。
    LaunchFailed,
    Starting,
    Streaming,
    Minimized,
    Closed,
    Failed(Error),
}

impl MirrorState {
    /// 配信の状態（未受信であれば `None`）から変換する。
    pub fn from_status(status: Option<Status>) -> Self {
        match status {
            None | Some(Status::Starting) => Self::Starting,
            Some(Status::Streaming) => Self::Streaming,
            Some(Status::Minimized) => Self::Minimized,
            Some(Status::Closed) => Self::Closed,
            Some(Status::Failed(error)) => Self::Failed(error),
        }
    }

    /// この状態で表示するボタン。閉じた・失敗時は再試行、最小化時は復元して再配信する。
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

    /// 失敗の理由（ローカライズ済み）。分類不能なエラーは元のメッセージをそのまま表示する。
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

/// `state` に対応する画面を構築する。ボタン（再試行等）は呼び出し側が `action` で指定する。
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
