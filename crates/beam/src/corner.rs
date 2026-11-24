//! 角を囲む線（[`Pointer::Corner`]）。角で大きさを変えられるパネルの角を指している間と、掴んで大きさを変えている間に出る。
//! はまる所の見本と同じ角枠（[`crate::bracket`]）で、パネル面から同じだけ離して角を囲む。掴んでいる間は強調色にする。
//! 描くのは右下の角だけで、配置・回転（ほかの角）・フェードはオーバーレイ層が決める。

use design::{BRACKET_PAD_PX, CORNER_HANDLE_PX, Theme};
use gpui::{Context, Window, div, prelude::*, px};
use kernel::{Hand, PanelId, Pointer, Pose, Vec2};
use ui::PanelSpec;

use crate::bracket::{self, Corner};

/// 角枠で囲む角丸矩形の一辺（px）。辺に沿って伸ばす線が、矩形の大きさで短くならない長さにする。
const FRAME_PX: f32 = 2.0 * CORNER_HANDLE_PX as f32;

/// `hand` の角の線の起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    let px = CORNER_HANDLE_PX;
    PanelSpec::root(Pose::IDENTITY, |panel, _, _| CornerHandle { panel })
        .sized(Vec2::splat(0.24), (px, px))
        .pointer(hand, Pointer::Corner)
}

/// 窓の中心がパネル面の角（丸める前の角）に来る。その右下の角を囲む線を描く。
pub struct CornerHandle {
    panel: PanelId,
}

impl Render for CornerHandle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let held = cx
            .try_global::<ui::PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .is_some_and(|s| s.status.corner_held);
        let color = bracket::color(&Theme::current(cx), held);
        // 右下の隅が、窓の中心（パネル面の角）から外へ BRACKET_PAD_PX の所に来る角丸矩形。
        let at = CORNER_HANDLE_PX as f32 / 2.0 + BRACKET_PAD_PX - FRAME_PX;
        div().size_full().child(
            bracket::frame(&[Corner::BottomRight], color)
                .absolute()
                .left(px(at))
                .top(px(at))
                .w(px(FRAME_PX))
                .h(px(FRAME_PX)),
        )
    }
}
