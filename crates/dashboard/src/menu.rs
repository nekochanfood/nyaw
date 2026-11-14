//! メニューのポップオーバー（[`Menu`]。デバッグ・設定・終了）と、終了を確かめるダイアログ。

use super::*;

/// メニューのポップオーバーの大きさと解像度（2 行の一覧が収まる）。
pub(crate) const MENU_PX: (u32, u32) = (
    320,
    (MENU_ROWS * components::MENU_ITEM_HEIGHT
        + 2.0 * components::MENU_PADDING
        + 2.0 * components::PANEL_MARGIN) as u32,
);

/// メニューの行の数（デバッグ・設定・終了。デバッグはリリースビルドでは出さない）。
const MENU_ROWS: f32 = if DEBUG_MENU { 3.0 } else { 2.0 };

/// メニューにデバッグ（Testing）を出すか。開発用の画面なので、リリースビルドでは開けないようにする。
const DEBUG_MENU: bool = cfg!(debug_assertions);

/// 終了の確かめのダイアログの、視界の中での置き場所。
const QUIT_DIALOG_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

pub(crate) const MENU_M: Vec2 = Vec2::new(
    MENU_PX.0 as f32 * SIZE_M.x / RES_PX.0 as f32,
    MENU_PX.1 as f32 * SIZE_M.x / RES_PX.0 as f32,
);

/// メニューを、はまっている画面よりどれだけ手前に出すか（m）。
const MENU_LIFT: f32 = 0.03;

/// メニューのポップオーバーの、バーから見た置き場所: バーのカプセルの左端（窓の左から `bar_left` px）の上に、
/// はめた画面と同じく垂直に立て、画面より少し手前に出す。
pub(crate) fn menu_offset(bar_left: f32) -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let rotation = Quat::from_rotation_x(BAR_TILT);
    // ポップオーバーの窓はカードの外に影の余白を持つので、その分だけ左にはみ出させて、カードの左端をカプセルに揃える。
    let shift = (bar_left - components::PANEL_MARGIN).max(0.0) * BAR_M.x / BAR_PX.0 as f32;
    let top = Vec3::new(
        (MENU_M.x - BAR_M.x) / 2.0 + shift,
        BAR_M.y / 2.0 - margin(BAR_M, BAR_PX),
        0.0,
    );
    let lift = kernel::overlay::SLOT_GAP + MENU_M.y / 2.0 - margin(MENU_M, MENU_PX);
    Pose::new(top + rotation * Vec3::new(0.0, lift, MENU_LIFT), rotation)
}

/// メニューのポップオーバー: デバッグ・設定（選ぶとその画面をはめる）と、一番下にアプリの終了（確かめのダイアログを出す）。
/// 選ぶと閉じる。背景は敷かず、一覧のカードがそのままポップオーバーになる。
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
            i18n::t(cx, "dashboard.quit"),
        )
        .text_color(rgba(theme.danger_text))
        .on_click(move |_, window, cx| {
            close(panel, cx);
            let spec = quit_dialog_spec(panel, window, cx);
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

/// アプリを終了するかを確かめるダイアログの起動仕様。視界の中央に出す。
fn quit_dialog_spec(from: PanelId, window: &Window, cx: &App) -> PanelSpec {
    let width = components::DIALOG_WIDTH;
    let height = components::dialog_height(
        window,
        &i18n::t(cx, "dashboard.quit_title"),
        &i18n::t(cx, "dashboard.quit_body"),
    );
    PanelSpec::spawn(from, |panel, _, _| QuitDialog { panel })
        .sized(
            Vec2::new(width as f32 / 1000.0, height as f32 / 1000.0),
            (width, height),
        )
        // 開いた元のメニューはすぐ閉じるので、その手前ではなく視界の中央に出す。
        .in_view(QUIT_DIALOG_POSE)
        // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
        .without_nav_bar()
}

/// アプリを終了するかを確かめるダイアログ。終了を選ぶとアプリを終える。
struct QuitDialog {
    panel: PanelId,
}

impl Render for QuitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        components::dialog(
            &theme,
            "quit-dialog",
            &icons::POWER,
            i18n::t(cx, "dashboard.quit_title"),
            i18n::t(cx, "dashboard.quit_body"),
            [
                components::button(
                    &theme,
                    "quit-cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| close(panel, cx)),
                components::button(
                    &theme,
                    "quit-confirm",
                    i18n::t(cx, "dashboard.quit_confirm"),
                    Variant::Danger,
                )
                .on_click(|_, _, cx| cx.default_global::<PanelControl>().quit = true),
            ],
        )
        // 電源のアイコンは塗りだと形が潰れて分かりにくいので、線で描く。
        .weight(design::Weight::Regular)
    }
}

#[cfg(test)]
mod tests;
