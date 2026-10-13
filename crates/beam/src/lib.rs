//! Layer 6 画面: コントローラのビーム。手ごとに、手元から伸びるレーザーと、指しているパネルに貼り付く
//! 小さな円（カーソル）の 2 枚。ここは見た目だけを描き、置き場所・長さ・表示はオーバーレイ層が毎 tick 決める。

use gpui::{Context, Window, div, prelude::*, px, rgba};
use kernel::overlay::LASER_MAX;
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

const COLOR: u32 = 0xffffffd0;
const OUTLINE: u32 = 0x000000a0;

/// レーザーのテクスチャ。横方向がレーザーの向きで、オーバーレイ層が幅 = 長さに伸ばす（高さは縦横比で
/// 長さの 1/16、線はその中央 1/16 なので太さは長さの 1/256: 1.5m で約 6mm）。
// ponytail: 太さが長さに比例する。気になるなら VrBackend に高さも渡せるようにする。
const LASER_PX: (u32, u32) = (1024, 64);
const LINE_PX: f32 = 4.0;
/// カーソルの直径（m）。
const CURSOR_M: f32 = 0.012;
/// Windows の非表示ウィンドウには最小の大きさがあるので小さくしすぎない。
const CURSOR_PX: (u32, u32) = (160, 160);

/// 両手のレーザーとカーソル。
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
