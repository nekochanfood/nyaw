//! Layer 6 画面: コントローラのビーム。手ごとに、手元から伸びるレーザーと、指しているパネルに貼り付く
//! 小さな円（カーソル）と、ドラッグ中の要素の見た目（中身は GPUI が描く `ui::DragPreview`）の 3 枚。ここは見た目だけを描き、置き場所・長さ・表示はオーバーレイ層が毎 tick 決める。

use gpui::{
    Bounds, Context, PathBuilder, Pixels, Point, Window, canvas, div, point, prelude::*, px, rgba,
};
use kernel::overlay::LASER_MAX;
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

const COLOR: u32 = 0xffffffd0;
const OUTLINE: u32 = 0x000000a0;

/// レーザーのテクスチャ。横方向がレーザーの向きで、オーバーレイ層が幅 = 長さに伸ばす。高さは長さによらず
/// 作ったとき（長さ LASER_MAX の 1/16）のままで、線はその中央 1/32 なので太さは約 3mm。
const LASER_PX: (u32, u32) = (1024, 64);
const LINE_PX: f32 = 2.0;
/// カーソルのパネルの幅（m）。頭から 1m 先にあるときの大きさで、離れるほど大きくして見た目の大きさを保つ。
/// 中心の丸（直径はこの 1/3 = 12mm）の周りに、長押しの進み具合のリングを描く余白を取ってある。
const CURSOR_M: f32 = 0.036;
/// Windows の非表示ウィンドウには最小の大きさがあるので小さくしすぎない。
const CURSOR_PX: (u32, u32) = (240, 240);
/// 中心の丸の直径（px）と、リングの半径（線の中心）・太さ（px）。丸はリングの穴に収まる。
const DOT_PX: f32 = 80.0;
const RING_RADIUS_PX: f32 = 80.0;
const RING_WIDTH_PX: f32 = 18.0;
/// リングの下地（薄い）と、満ちた分の色。
const RING_TRACK: u32 = 0x00000060;
const RING_FILL: u32 = 0xffffffe0;
/// これより短い押下（タップ）ではリングを出さない（ちらつかないように）。
const RING_SHOW: f32 = 0.15;
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
                PanelSpec::new(Pose::IDENTITY, move |_, _, _| Cursor { hand })
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

/// カーソル。中心の丸と、タッチで長押ししている間はその進み具合を満ちていくリング（ドーナツ型。丸は穴に収まる）で出す。
/// リングが満ちるとマウスの押下になり、ドラッグ＆ドロップができる。
pub struct Cursor {
    pub hand: Hand,
}

impl Render for Cursor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = cx
            .try_global::<ui::PanelControl>()
            .and_then(|c| c.hold.get(&self.hand).copied())
            .filter(|p| *p >= RING_SHOW);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            // 白いパネルの上でも見えるよう縁取る。
            .child(
                div()
                    .size(px(DOT_PX))
                    .rounded_full()
                    .bg(rgba(COLOR))
                    .border_8()
                    .border_color(rgba(OUTLINE)),
            )
            .when_some(progress, |d, p| {
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| ring(bounds, p, window),
                    )
                    .absolute()
                    .size_full(),
                )
            })
    }
}

/// `bounds` の中心に、下地の輪と、12 時から時計回りに `progress`（0〜1）だけ満ちた弧を描く。
fn ring(bounds: Bounds<Pixels>, progress: f32, window: &mut Window) {
    let center = bounds.center();
    let at = |turn: f32| -> Point<Pixels> {
        let a = (turn - 0.25) * std::f32::consts::TAU;
        point(
            center.x + px(RING_RADIUS_PX * a.cos()),
            center.y + px(RING_RADIUS_PX * a.sin()),
        )
    };
    let arc = |to: f32| {
        let mut b = PathBuilder::stroke(px(RING_WIDTH_PX));
        let steps = ((to * 64.0).ceil() as usize).max(1);
        b.move_to(at(0.0));
        for i in 1..=steps {
            b.line_to(at(to * i as f32 / steps as f32));
        }
        b.build()
    };
    if let Ok(track) = arc(1.0) {
        window.paint_path(track, rgba(RING_TRACK));
    }
    if let Ok(fill) = arc(progress) {
        window.paint_path(fill, rgba(RING_FILL));
    }
}

/// 空。ドラッグ中は GPUI がドラッグの見た目をマウス位置（= 中心）に重ねて描く。
pub struct DragLayer;

impl Render for DragLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}
