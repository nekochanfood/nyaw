//! ナビゲーションバーの「…」から開くポップオーバー（[`NavMenu`]）と、手に保持している間のグラブロック（[`hold`]）。
//! ポップオーバーでは手への装着・ドックからの着脱を操作する。

use super::*;
use design::{Theme, components, font_size, icons, spacing};

/// ポップオーバーのウィンドウ幅（px、バーと同一密度）。「ダッシュボードから外す」が省略なく収まる幅。
const MENU_WIDTH: f32 = 380.0;
/// ポップオーバーを持ち主の面（とバー）よりどれだけ手前に浮かせるか（m、1m 先での値で距離に応じて拡大）。
/// 持ち主と重なる位置に表示するため、同一面ではめり込みが生じる。
const MENU_LIFT: f32 = 0.03;

/// `panel` を保持している手を返す（手に装着されていなければ `None`）。
pub(crate) fn in_hand(panel: PanelId, cx: &App) -> Option<Hand> {
    let state = cx.try_global::<PanelControl>()?.panels.get(&panel)?;
    match state.status.attached {
        Some(Device::Controller(hand)) => Some(hand),
        _ => None,
    }
}

/// 手に保持している `owner` を、ダッシュボード非表示時のみグラブロックする（配置・向き・大きさの変更を制限）。
/// グラブ中にダッシュボードが消えると、ロックがグラブを取り消して元の状態に戻す（wrist と同一動作）。
/// 手から外れた場合はロックも解除する。`locked` は現在のロック状態（変更時のみキューイングし、再呼び出しされる）。
/// 持ち主のナビゲーションバーが状態変化するたびに呼び出す。
pub(crate) fn hold(owner: PanelId, locked: &mut bool, cx: &mut App) {
    let want = in_hand(owner, cx).is_some() && dock_slot(cx).is_none();
    if want != *locked {
        *locked = want;
        command(owner, PanelCommand::Lock(want), cx);
    }
}

/// `owner` を `hand` に装着する。親（ドック先のダッシュボード等）に属している場合は取り外して独立させる（子は親に追従するため）。
fn attach(owner: PanelId, hand: Hand, cx: &mut App) {
    let ctl = cx.global::<PanelControl>();
    if ctl
        .panels
        .get(&owner)
        .is_some_and(|p| p.status.parent.is_some())
    {
        undock(owner, cx);
    }
    command(owner, PanelCommand::AttachToHand(hand), cx);
}

/// `owner` のドック状態。`Some(true)` = ドック中（取り外し可）、`Some(false)` = 未装着でドックが空き（装着可）。
/// どちらにも該当しなければ `None`。
fn dock_state(owner: PanelId, cx: &App) -> Option<bool> {
    let parent = cx
        .try_global::<PanelControl>()?
        .panels
        .get(&owner)
        .and_then(|p| p.status.parent);
    let slot = dock_slot(cx);
    let inside = parent.is_some() && parent == slot.map(|(o, _)| o);
    (inside || slot.is_some()).then_some(inside)
}

/// 「…」ポップオーバーの起動仕様。`owner` のナビゲーションバー上に、カード右端を帯の右端（「…」側）に揃え、
/// やや手前に浮かせて表示する。バーと同一密度で描画し、頭からの距離に応じて拡大する（[`PanelSpec::over_nav_bar`]）。
/// `dockable` の場合はドック着脱行も表示する（操作可能な場合のみ）。
pub(crate) fn menu_spec(owner: PanelId, dockable: bool, cx: &App) -> PanelSpec {
    let hand = in_hand(owner, cx);
    let docked = dockable.then(|| dock_state(owner, cx)).flatten();
    let rows = 2 + hand.is_some() as usize + docked.is_some() as usize;
    let margin = components::PANEL_MARGIN;
    let height =
        rows as f32 * components::MENU_ITEM_HEIGHT + 2.0 * (components::MENU_PADDING + margin);
    let bar = Vec2::new(NAV_BAR_PX.0 as f32, NAV_BAR_PX.1 as f32);
    // バーのウィンドウ中心から見たポップオーバーのウィンドウ中心（px、上が正）。
    // 右端を揃えるとカード右端が帯の右端に合い、カード下端は帯上端の `SM` 上に位置する。
    let at = Vec2::new(
        (bar.x - MENU_WIDTH) / 2.0,
        bar.y / 2.0 - margin + spacing::SM - margin + height / 2.0,
    );
    let m = NAV_BAR_M / bar.x;
    PanelSpec::popover(owner, move |panel, _, _| NavMenu {
        panel,
        owner,
        hand,
        docked,
    })
    .sized(
        Vec2::new(MENU_WIDTH, height) * m,
        (MENU_WIDTH as u32, height as u32),
    )
    .offset(Pose::new((at * m).extend(MENU_LIFT), Quat::IDENTITY))
    .over_nav_bar()
    .without_resize()
}

/// 「…」ポップオーバー。左手・右手への装着（装着中の手にマーク）、手からの取り外し（その場に残す）、ドック着脱を操作する。
/// 項目選択で自動的に閉じる。
struct NavMenu {
    panel: PanelId,
    owner: PanelId,
    /// ポップオーバーを開いた時点で装着していた手。
    hand: Option<Hand>,
    /// ポップオーバーを開いた時点の [`dock_state`]（着脱行を表示しない場合は `None`）。
    docked: Option<bool>,
}

impl Render for NavMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (panel, owner) = (self.panel, self.owner);
        let item = |id, hand, key| {
            let on = self.hand == Some(hand);
            components::menu_item(&theme, id, &icons::HAND, i18n::t(cx, key))
                .children(on.then(|| design::icon(&icons::CHECK).size(px(font_size::MD))))
                .on_click(move |_, _, cx| {
                    if !on {
                        attach(owner, hand, cx);
                    }
                    command(panel, PanelCommand::Close, cx);
                })
        };
        let left = item("hand-left", Hand::Left, "hand.left");
        let right = item("hand-right", Hand::Right, "hand.right");
        let detach = self.hand.map(|_| {
            components::menu_item(
                &theme,
                "hand-detach",
                &icons::LINK_BREAK,
                i18n::t(cx, "hand.detach"),
            )
            .on_click(move |_, _, cx| {
                command(owner, PanelCommand::Detach, cx);
                command(panel, PanelCommand::Close, cx);
            })
        });
        let docking = self.docked.map(|inside| {
            let (id, glyph, key) = if inside {
                ("nav-undock", &icons::ARROW_SQUARE_OUT, "dock.undock")
            } else {
                ("nav-dock", &icons::CARET_LINE_DOWN, "dock.dock")
            };
            components::menu_item(&theme, id, glyph, i18n::t(cx, key)).on_click(move |_, _, cx| {
                if inside {
                    undock(owner, cx)
                } else {
                    dock(owner, cx)
                }
                dock_sound(owner, !inside, cx);
                command(panel, PanelCommand::Close, cx);
            })
        });
        components::card_panel(&theme)
            .rounded(px(components::MENU_RADIUS))
            .child(components::menu(
                &theme,
                [left, right].into_iter().chain(detach).chain(docking),
            ))
    }
}

#[cfg(test)]
mod tests;
