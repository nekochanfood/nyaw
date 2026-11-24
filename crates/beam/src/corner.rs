//! 角を囲む線（[`Pointer::Corner`]）。角で大きさを変えられるパネルの角を指している間と、掴んで大きさを変えている間に出る。
//! パネル面の角丸から少し離して、角丸と同じ中心の円弧とそこから辺に沿って伸ばした線を描く。
//! 描くのは右下の角だけで、配置と回転（ほかの角）はオーバーレイ層が決める。

use design::{CORNER_HANDLE_PX, radius};
use gpui::{
    Bounds, Context, PathBuilder, Pixels, Window, canvas, div, point, prelude::*, px, rgba,
};
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

use super::{COLOR, OUTLINE};

/// 線の太さと、パネル面の角丸から線までの隙間、円弧の両端から辺に沿って伸ばす長さ（px。パネルの 1px と同じ）。
const LINE_PX: f32 = 6.0;
const GAP_PX: f32 = 10.0;
const ARM_PX: f32 = 56.0;
/// 白い面の上でも見えるよう、線の下に敷く縁取りの幅（片側、px）。
const OUTLINE_PX: f32 = 3.0;
/// 円弧を近似する線分の数。
const STEPS: usize = 16;

/// `hand` の角の線の起動仕様。
pub fn spec(hand: Hand) -> PanelSpec {
    let px = CORNER_HANDLE_PX;
    PanelSpec::root(Pose::IDENTITY, |_, _, _| CornerHandle)
        .sized(Vec2::splat(0.24), (px, px))
        .pointer(hand, Pointer::Corner)
}

/// 窓の中心がパネル面の角（丸める前の角）に来る。その右下の角を囲む線を描く。
pub struct CornerHandle;

impl Render for CornerHandle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    line(bounds, LINE_PX + 2.0 * OUTLINE_PX, OUTLINE, window);
                    line(bounds, LINE_PX, COLOR, window);
                },
            )
            .size_full(),
        )
    }
}

/// 角丸の中心を中心とする 4 分の 1 の円弧と、その両端から上・左へ伸ばした線を、太さ `width`、色 `color` で描く。
fn line(bounds: Bounds<Pixels>, width: f32, color: u32, window: &mut Window) {
    let c = bounds.center();
    let (cx, cy) = (f32::from(c.x) - radius::XL, f32::from(c.y) - radius::XL);
    let r = radius::XL + GAP_PX + LINE_PX / 2.0;
    let mut b = PathBuilder::stroke(px(width));
    b.move_to(point(px(cx + r), px(cy - ARM_PX)));
    for i in 0..=STEPS {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
        b.line_to(point(px(cx + r * a.cos()), px(cy + r * a.sin())));
    }
    b.line_to(point(px(cx - ARM_PX), px(cy + r)));
    if let Ok(path) = b.build() {
        window.paint_path(path, rgba(color));
    }
}
