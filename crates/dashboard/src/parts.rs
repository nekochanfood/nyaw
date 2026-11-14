//! 画面どうしで使う小さな部品: 見出し付きの区切り・グリッド・タイル・時計、パネルを閉じる・配信を開く操作。

use super::*;

/// グリッドの列の数。
const COLUMNS: u16 = 4;

/// 見出しの行（右に操作を足せる）。高さはボタンと同じにして、操作の有無で一覧の位置が変わらないようにする。
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

pub(crate) fn grid() -> Div {
    div().grid().grid_cols(COLUMNS).gap(px(spacing::SM))
}

/// タイルの高さ。アイコンと 1 行の文字を入れる箱なので、文字の大きさの倍率に付いていく。
pub(crate) fn tile_height() -> gpui::Pixels {
    design::font::scaled(TILE_PX)
}

/// タイルに載せる画像のアイコンの一辺。隣に並ぶ Phosphor のアイコン（`design::icon`）と同じく、文字の大きさの倍率に付いていく。
pub(crate) fn tile_icon_size() -> gpui::Pixels {
    design::font::scaled(TILE_ICON_PX)
}

/// アプリのアイコンの画像。無ければ `glyph`。
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

/// タイルの文字（1 行。長ければ省略）。
pub(crate) fn tile_label(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .text_center()
        .truncate()
        .child(text.into())
}

/// アイコンの RGBA を GPUI の画像にする。
pub(crate) fn render_image(icon: &Icon) -> Option<Arc<RenderImage>> {
    ui::render_image(icon.width, icon.height, icon.rgba.to_vec())
}

/// 時刻（`時:分`）を太字の `size` px で組んだときの、どの数字でも収まる幅。いちばん広い数字（"00:00"〜"99:99"）で測る。
pub(crate) fn clock_width(window: &Window, size: f32) -> gpui::Pixels {
    (0..10)
        .map(|d| design::font::width(format!("{d}{d}:{d}{d}"), size, FontWeight::BOLD, window))
        .fold(px(0.0), |a, b| a.max(b))
}

/// 今の時刻（`時:分`、太字）を、幅 `width` の箱の中央に置く。数字で幅が変わって周りが動かないように。
pub(crate) fn fixed_clock(width: gpui::Pixels) -> Div {
    div()
        .flex_none()
        .w(width)
        .flex()
        .justify_center()
        .font_weight(FontWeight::BOLD)
        .child(chrono::Local::now().format("%H:%M").to_string())
}

/// 再生位置・長さを `分:秒`（1 時間を超えれば `時:分:秒`）で。
pub(crate) fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 1 秒ごとに描き直す（時計・電池・取得元の一覧の到着）。ビューが無くなれば止まる。
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

/// `source` の配信を始める操作。FPS は全体の設定（`mirror_frame_rate`）。
pub(crate) fn stream(source: Source, cx: &mut App) -> PanelCommand {
    PanelCommand::Stream(Some(source), ui::settings(cx).mirror_frame_rate)
}
