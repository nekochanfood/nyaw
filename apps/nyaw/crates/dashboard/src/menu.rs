//! メニューのポップオーバー（[`Menu`]。デバッグ・設定・終了。終了は確認ダイアログ `ui::QuitDialog` を表示する）。

use super::*;

/// メニューポップオーバーのサイズと解像度（2 行のリストが収まる）。
pub(crate) const MENU_PX: (u32, u32) = (
    320,
    (MENU_ROWS * components::MENU_ITEM_HEIGHT
        + 2.0 * components::MENU_PADDING
        + 2.0 * components::PANEL_MARGIN) as u32,
);

/// メニューの行数（デバッグ・設定・終了。デバッグはリリースビルドでは非表示）。
const MENU_ROWS: f32 = if DEBUG_MENU { 3.0 } else { 2.0 };

/// メニューにデバッグ（Testing）を表示するか。開発用画面のため、リリースビルドでは無効化する。
const DEBUG_MENU: bool = cfg!(debug_assertions);

/// 終了確認ダイアログの視界内における配置位置。
const QUIT_DIALOG_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

pub(crate) const MENU_M: Vec2 = Vec2::new(
    MENU_PX.0 as f32 * SIZE_M.x / RES_PX.0 as f32,
    MENU_PX.1 as f32 * SIZE_M.x / RES_PX.0 as f32,
);

/// メニューをドッキング画面よりどれだけ手前に配置するか（m）。
const MENU_LIFT: f32 = 0.03;

/// メニューポップオーバーのバーからの相対位置: バーのカプセル左端（ウィンドウ左端から `bar_left` px）の上に、
/// ドッキング画面と同じく垂直に立て、画面よりわずかに手前に配置する。
pub(crate) fn menu_offset(bar_left: f32) -> Pose {
    let shift = (bar_left - components::PANEL_MARGIN).max(0.0) * BAR_M.x / BAR_PX.0 as f32;
    popover_offset((MENU_M.x - BAR_M.x) / 2.0 + shift, MENU_M, MENU_PX)
}

/// 配信の一覧ポップオーバーの位置: バーのカプセル右端（ウィンドウ右端から `bar_right_gap` px）に右端を揃える。
pub(crate) fn overflow_offset(bar_right_gap: f32, rows: usize) -> Pose {
    let (size, res) = overflow_size(rows);
    let shift = (bar_right_gap - components::PANEL_MARGIN).max(0.0) * BAR_M.x / BAR_PX.0 as f32;
    popover_offset((BAR_M.x - size.x) / 2.0 - shift, size, res)
}

/// バーの上に垂直に立てるポップオーバー（サイズ `size`・解像度 `res`）の、バー中心からの相対位置（`x` は横方向の位置）。
fn popover_offset(x: f32, size: Vec2, res: (u32, u32)) -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let rotation = Quat::from_rotation_x(BAR_TILT);
    let top = Vec3::new(x, BAR_M.y / 2.0 - margin(BAR_M, BAR_PX), 0.0);
    let lift = kernel::overlay::SLOT_GAP + size.y / 2.0 - margin(size, res);
    Pose::new(top + rotation * Vec3::new(0.0, lift, MENU_LIFT), rotation)
}

/// `rows` 行の一覧ポップオーバーのサイズと解像度。
pub(crate) fn overflow_size(rows: usize) -> (Vec2, (u32, u32)) {
    let height = rows as f32 * components::MENU_ITEM_HEIGHT
        + 2.0 * components::MENU_PADDING
        + 2.0 * components::PANEL_MARGIN;
    let res = (MENU_PX.0, height as u32);
    let scale = SIZE_M.x / RES_PX.0 as f32;
    (Vec2::new(res.0 as f32 * scale, res.1 as f32 * scale), res)
}

/// メニューポップオーバー: デバッグ・設定（選択するとその画面をドッキングする）と、末尾にアプリ終了（確認ダイアログを表示）。
/// 選択するとクローズする。背景は描画せず、リストのカードがそのままポップオーバーとなる。
pub struct Menu {
    pub(crate) panel: PanelId,
    pub(crate) dashboard: WeakEntity<Dashboard>,
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let item = |id, glyph, key, page| {
            let (dashboard, panel) = (self.dashboard.clone(), self.panel);
            components::menu_item(&theme, id, glyph, i18n::t(cx, key)).on_click(move |_, _, cx| {
                close(panel, cx);
                let _ = dashboard.update(cx, |d, cx| d.toggle(page, cx));
            })
        };
        let panel = self.panel;
        let quit = components::menu_item(
            &theme,
            "menu-quit",
            &icons::POWER,
            i18n::t(cx, "quit.label"),
        )
        .text_color(rgba(theme.danger_text))
        .on_click(move |_, window, cx| {
            close(panel, cx);
            // 起動元のメニューは即座にクローズするため、その子ではなく視界の中央に配置する。
            let spec = ui::quit_dialog(window, cx, |build| {
                PanelSpec::spawn(panel, build).in_view(QUIT_DIALOG_POSE)
            });
            cx.default_global::<PanelControl>().spawns.push(spec);
        });
        components::card_panel(&theme)
            .rounded(px(components::MENU_RADIUS))
            .child(components::menu(
                &theme,
                [
                    DEBUG_MENU
                        .then(|| item("menu-debug", &icons::BUG, "dashboard.debug", Page::Debug)),
                    Some(item(
                        "menu-settings",
                        &icons::GEAR,
                        "dashboard.settings",
                        Page::Settings,
                    )),
                    Some(quit),
                ]
                .into_iter()
                .flatten(),
            ))
    }
}

/// バーに並べきれない配信の一覧ポップオーバー。選択するとその配信をドッキング（ドッキング中なら非表示化）してクローズする。
pub struct StreamMenu {
    pub(crate) panel: PanelId,
    pub(crate) dashboard: WeakEntity<Dashboard>,
    /// 並べきれなかった配信。
    pub(crate) streams: Vec<Stream>,
}

impl Render for StreamMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let items = self.streams.iter().map(|stream| {
            let (key, panel, dashboard) = (stream.key.clone(), self.panel, self.dashboard.clone());
            let glyph = match (stream.kind, &stream.icon) {
                (SourceKind::Window, Some(icon)) => {
                    img(icon.clone()).size(px(font_size::LG)).into_any_element()
                }
                (SourceKind::Window, None) => design::icon(&icons::APP_WINDOW)
                    .size(px(font_size::LG))
                    .into_any_element(),
                (SourceKind::Screen, _) => design::icon(&icons::MONITOR)
                    .size(px(font_size::LG))
                    .into_any_element(),
            };
            components::menu_item_with(&theme, key.clone(), glyph, stream.name.clone()).on_click(
                move |_, _, cx| {
                    close(panel, cx);
                    let _ = dashboard.update(cx, |d, cx| d.activate(&key, cx));
                },
            )
        });
        components::card_panel(&theme)
            .rounded(px(components::MENU_RADIUS))
            .child(components::menu(&theme, items))
    }
}

#[cfg(test)]
mod tests;
