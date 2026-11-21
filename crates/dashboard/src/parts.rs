//! 画面間で共用する小部品: 見出し付きセクション・グリッド・タイル・時計、パネルのクローズ・配信開始の操作。

use super::*;

/// 見出し行（右側に操作ボタンを追加可能）。高さをボタンと同一にし、操作の有無でリスト位置が変動しないようにする。
pub(crate) fn section(theme: &Theme, title: gpui::SharedString) -> Div {
    div()
        .h(px(components::NAV_BAR_HEIGHT - 2.0 * spacing::SM))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(spacing::SM))
        .text_size(px(font_size::MD))
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(theme.neutral_text_low))
        .child(title)
}

/// タイルの高さ。アイコンと 1 行のテキストを格納する領域のため、フォントサイズのスケーリングに追従する。
pub(crate) fn tile_height() -> gpui::Pixels {
    design::font::scaled(TILE_PX)
}

/// タイルに表示する画像アイコンの一辺。隣接する Phosphor アイコン（`design::icon`）と同様にフォントサイズのスケーリングに追従する。
pub(crate) fn tile_icon_size() -> gpui::Pixels {
    design::font::scaled(TILE_ICON_PX)
}

/// アプリのアイコン画像。取得できなければ `glyph` で代替する。
pub(crate) fn tile_image(
    icon: Option<Arc<RenderImage>>,
    glyph: &'static design::Icon,
) -> gpui::AnyElement {
    match icon {
        Some(icon) => img(icon).size(tile_icon_size()).into_any_element(),
        None => tile_icon(glyph),
    }
}

pub(crate) fn tile_icon(glyph: &'static design::Icon) -> gpui::AnyElement {
    design::icon(glyph)
        .size(px(TILE_ICON_PX))
        .into_any_element()
}

/// タイルのラベル（1 行。長い場合は省略表示）。
pub(crate) fn tile_label(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .text_center()
        .truncate()
        .child(text.into())
}

/// アイコンの RGBA データを GPUI の画像に変換する。
pub(crate) fn render_image(icon: &Icon) -> Option<Arc<RenderImage>> {
    ui::render_image(icon.width, icon.height, icon.rgba.to_vec())
}

/// 時刻（`時:分`）を太字 `size` px で組んだときの最大幅。すべての数字の組み合わせ（"00:00"〜"99:99"）で計測する。
pub(crate) fn clock_width(window: &Window, size: f32) -> gpui::Pixels {
    (0..10)
        .map(|d| design::font::width(format!("{d}{d}:{d}{d}"), size, FontWeight::BOLD, window))
        .fold(px(0.0), |a, b| a.max(b))
}

/// 現在時刻（`時:分`、太字）を幅 `width` の固定領域内に中央配置する。数字による幅変動で周囲がずれるのを防ぐ。
pub(crate) fn fixed_clock(width: gpui::Pixels) -> Div {
    div()
        .flex_none()
        .w(width)
        .flex()
        .justify_center()
        .font_weight(FontWeight::BOLD)
        .child(chrono::Local::now().format("%H:%M").to_string())
}

/// 再生位置・長さを `分:秒` 形式で返す（1 時間以上の場合は `時:分:秒`）。
pub(crate) fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 1 秒間隔で再描画する（時計・電池・ソース一覧の到着確認用）。ビューが破棄されると停止する。
pub(crate) fn tick_every_second<V: 'static>(panel: PanelId, cx: &mut Context<V>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| ui::notify(panel, cx)).is_err() {
                return;
            }
        }
    })
    .detach();
}

pub(crate) fn close(panel: PanelId, cx: &mut App) {
    commands(panel, [PanelCommand::Close], cx);
}

/// `source` の配信開始コマンド。FPS はグローバル設定（`mirror_frame_rate`）に従う。
pub(crate) fn stream(source: Source, cx: &mut App) -> PanelCommand {
    PanelCommand::Stream(Some(source), ui::settings(cx).mirror_frame_rate)
}
