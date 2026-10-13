//! Layer 6 画面: コントローラのビーム。手ごとに、手元から伸びるレーザーと、指しているパネルに貼り付く
//! 小さな円（カーソル）と、ドラッグ中の要素の見た目（中身は GPUI が描く `ui::DragPreview`）の 3 枚。ここは見た目だけを描き、置き場所・長さ・表示はオーバーレイ層が毎 tick 決める。

use gpui::{Context, Window, div, prelude::*, px, rgba};
use kernel::overlay::LASER_MAX;
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

const COLOR: u32 = 0xffffffd0;
const OUTLINE: u32 = 0x000000a0;

/// レーザーのテクスチャ。横方向がレーザーの向きで、オーバーレイ層が幅 = 長さに伸ばす。高さは長さによらず
/// 作ったとき（長さ LASER_MAX の 1/16）のままで、線はその中央 1/32 なので太さは約 3mm。
const LASER_PX: (u32, u32) = (1024, 64);
const LINE_PX: f32 = 2.0;
/// カーソルの直径（m）。頭から 1m 先にあるときの大きさで、離れるほど大きくして見た目の大きさを保つ。
const CURSOR_M: f32 = 0.012;
/// Windows の非表示ウィンドウには最小の大きさがあるので小さくしすぎない。
const CURSOR_PX: (u32, u32) = (160, 160);
/// ドラッグの見た目を描くウィンドウ。中心がカーソル。幅（m）はドラッグを始めたパネルの縮尺で UI 層が決める。
const DRAG_PX: (u32, u32) = (400, 400);

/// 両手のレーザーとカーソルとドラッグの見た目。
pub fn specs() -> Vec<PanelSpec> {
    [Hand::Left, Hand::Right]
        .into_iter()
        .flat_map(|hand| {
            [
                PanelSpec::new(Pose::IDENTITY, |_, _, _| Laser)
                    .sized(Vec2::new(LASER_MAX, LASER_MAX / 16.0), LASER_PX)
                    .pointer(hand, Pointer::Laser),
                PanelSpec::new(Pose::IDENTITY, |_, _, _| Cursor)
                    .sized(Vec2::splat(CURSOR_M), CURSOR_PX)
                    .pointer(hand, Pointer::Cursor),
                PanelSpec::new(Pose::IDENTITY, |_, _, _| DragLayer)
                    .sized(Vec2::splat(0.4), DRAG_PX)
                    .pointer(hand, Pointer::Drag),
            ]
        })
        .collect()
}

pub struct Laser;

impl Render for Laser {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .child(div().w_full().h(px(LINE_PX)).rounded_full().bg(rgba(COLOR)))
    }
}

pub struct Cursor;

impl Render for Cursor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // 白いパネルの上でも見えるよう縁取る。
        div().size_full().p_4().child(
            div()
                .size_full()
                .rounded_full()
                .bg(rgba(COLOR))
                .border_8()
                .border_color(rgba(OUTLINE)),
        )
    }
}

/// 空。ドラッグ中は GPUI がドラッグの見た目をマウス位置（= 中心）に重ねて描く。
pub struct DragLayer;

impl Render for DragLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}
