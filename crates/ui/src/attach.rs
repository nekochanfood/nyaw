//! ナビゲーションバーの「…」から開くポップオーバー（[`NavMenu`]。手に持つ・はめる場所から外す/戻す）と、手に持っている間は
//! ダッシュボードが出ているときだけ掴めるようにするロック（[`hold`]）。

use super::*;
use design::{Theme, components, font_size, icons, spacing};

/// ポップオーバーの窓の幅（px。バーと同じ密度）。「ダッシュボードから外す」が省略されずに入る幅。
const MENU_WIDTH: f32 = 380.0;
/// ポップオーバーを持ち主の面（とバー）よりどれだけ手前に浮かせるか（m。1m 先での値で、バーと同じく距離に合わせて伸ばす）。
/// 持ち主に重なる所に出すので、同じ面だとめり込んで見える。
const MENU_LIFT: f32 = 0.03;

/// `panel` を持っている手（手に取り付いていなければ `None`）。
pub(crate) fn in_hand(panel: PanelId, cx: &App) -> Option<Hand> {
    let state = cx.try_global::<PanelControl>()?.panels.get(&panel)?;
    match state.status.attached {
        Some(Device::Controller(hand)) => Some(hand),
        _ => None,
    }
}

/// 手に持っている `owner` を、ダッシュボードが出ている間だけ掴めるようにする（置き場所・向き・大きさを変えられる）。掴んだまま
/// ダッシュボードが消えたら、ロックがその掴みを取り消して元へ戻す（wrist と同じ）。手から外れたらロックも外す。
/// `locked` は今かけてあるか（変わるときだけ積む。積むとまた呼ばれる）。持ち主のナビゲーションバーが状態の変わるたびに呼ぶ。
pub(crate) fn hold(owner: PanelId, locked: &mut bool, cx: &mut App) {
    let want = in_hand(owner, cx).is_some() && dock_slot(cx).is_none();
    if want != *locked {
        *locked = want;
        command(owner, PanelCommand::Lock(want), cx);
    }
}

/// `owner` を `hand` に持つ。親（はめた先のダッシュボードなど）に付いていれば外して独立させる（子は親に付いて動くため）。
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

/// はめる場所に対する `owner` の今: `Some(true)` = はまっている（外せる）、`Some(false)` = 外れていて場所が開いている（戻せる）。
/// どちらでもなければ `None`。
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

/// 「…」のポップオーバーの起動仕様。`owner` のナビゲーションバーの上に、カードの右端を帯の右端（「…」の側）に揃えて
/// 少し手前に浮かせて出す。バーと同じ密度で書き、バーと同じく頭からの距離に合わせて大きくする（[`PanelSpec::over_nav_bar`]）。
/// `dockable` なら、はめる場所から外す・戻す行も出す（できるときだけ）。
pub(crate) fn menu_spec(owner: PanelId, dockable: bool, cx: &App) -> PanelSpec {
    let hand = in_hand(owner, cx);
    let docked = dockable.then(|| dock_state(owner, cx)).flatten();
    let rows = 2 + hand.is_some() as usize + docked.is_some() as usize;
    let margin = components::PANEL_MARGIN;
    let height =
        rows as f32 * components::MENU_ITEM_HEIGHT + 2.0 * (components::MENU_PADDING + margin);
    let bar = Vec2::new(NAV_BAR_PX.0 as f32, NAV_BAR_PX.1 as f32);
    // バーの窓の中心から見た、ポップオーバーの窓の中心（px。上が +）。窓の右端をそろえるとカードの右端が帯の右端にそろい、
    // カードの下端は帯の上端の `SM` 上に来る。
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

/// 「…」のポップオーバー: 左手・右手に持つ（持っている手に印）と、持っていれば手から外す（その場に残す）、はめる場所から
/// 外す・戻す。選ぶと閉じる。
struct NavMenu {
    panel: PanelId,
    owner: PanelId,
    /// 開いたときに持っていた手。
    hand: Option<Hand>,
    /// 開いたときの [`dock_state`]（外す・戻す行を出さないなら `None`）。
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
