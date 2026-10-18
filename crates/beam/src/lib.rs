//! Layer 6 画面: コントローラのビーム。手ごとに、手元から伸びるレーザーと、指しているパネルに貼り付く
//! 小さな円（カーソル）と、ドラッグ中の要素の見た目（中身は GPUI が描く `ui::DragPreview`）と、カーソルの子のツールチップ（[`tooltip`]）の 4 枚。ここは見た目だけを描き、置き場所・長さ・表示はオーバーレイ層が毎 tick 決める。

use gpui::{
    Bounds, Context, PathBuilder, Pixels, Point, Window, canvas, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgba,
};
use kernel::overlay::LASER_MAX;
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

pub mod tooltip;
pub use tooltip::Tooltip;

const COLOR: u32 = 0xffffffd0;
const OUTLINE: u32 = 0x000000a0;

/// レーザーのテクスチャ。横方向がレーザーの向き（左端 = 手元）で、オーバーレイ層が幅 = 長さに伸ばし、
/// 高さは長さによらず [`LASER_THICK_M`] に固める。線はテクスチャいっぱいに塗るので、線の縁 = クアッドの縁になり、
/// 拡大してもにじまない（細い線を透明の中に描くと、拡大の補間で縁がぼける）。
/// 縦の画素数は Windows の非表示ウィンドウの最小の大きさのため。
const LASER_PX: (u32, u32) = (1024, 64);
/// レーザーの太さ（m）。
const LASER_THICK_M: f32 = 0.003;
/// レーザーの不透明度の段数。段ごとに線形のグラデーションでつなぎ、曲線に沿わせる。
const LASER_FADE_STEPS: usize = 16;
/// カーソルのパネルの幅（m）。頭から 1m 先にあるときの大きさで、離れるほど大きくして見た目の大きさを保つ。
/// 中心の丸（直径はこの 1/3 = 12mm）の周りに、長押しの進み具合のリングを描く余白を取ってある。
const CURSOR_M: f32 = 0.036;
/// Windows の非表示ウィンドウには最小の大きさがあるので小さくしすぎない。
const CURSOR_PX: (u32, u32) = (240, 240);
/// 中心の丸の直径（px）と、リングの半径（線の中心）・太さ（px）。丸はリングの穴に収まる。
const DOT_PX: f32 = 80.0;
const RING_RADIUS_PX: f32 = 80.0;
const RING_WIDTH_PX: f32 = 18.0;
/// リングの下地（薄い）。満ちた分はテーマの強調色（`accent-solid`。白だと白い面の上で見えない）。
const RING_TRACK: u32 = 0x00000060;
/// これより短い押下（タップ）ではリングを出さない（ちらつかないように）。
const RING_SHOW: f32 = 0.15;
/// ドラッグの見た目を描くウィンドウ。中心がカーソル。幅（m）はドラッグを始めたパネルの縮尺で UI 層が決める。
const DRAG_PX: (u32, u32) = (400, 400);
/// ツールチップのパネル。中心がカーソルで、カーソルと同じく頭から 1m 先での大きさ（m）。
/// オーバーレイ層が距離に比例して大きくするので、近くても遠くても見た目の大きさは変わらない。
/// 箱の高さ（`TOOLTIP_HEIGHT` px）が `TOOLTIP_HEIGHT_M`（カーソルの丸 2 個分の高さ）に見える縮尺にする。
const TOOLTIP_PX: (u32, u32) = (640, 256);
const TOOLTIP_M: f32 =
    TOOLTIP_PX.0 as f32 * design::components::TOOLTIP_HEIGHT_M / design::components::TOOLTIP_HEIGHT;

/// 両手のレーザーとカーソルとドラッグの見た目。
pub fn specs() -> Vec<PanelSpec> {
    [Hand::Left, Hand::Right]
        .into_iter()
        .flat_map(|hand| {
            [
                PanelSpec::root(Pose::IDENTITY, |_, _, _| Laser)
                    .sized(Vec2::new(LASER_MAX, LASER_THICK_M), LASER_PX)
                    .pointer(hand, Pointer::Laser),
                PanelSpec::root(Pose::IDENTITY, move |_, _, _| Cursor { hand })
                    .sized(Vec2::splat(CURSOR_M), CURSOR_PX)
                    .pointer(hand, Pointer::Cursor),
                PanelSpec::root(Pose::IDENTITY, |_, _, _| DragLayer)
                    .sized(Vec2::splat(0.4), DRAG_PX)
                    .pointer(hand, Pointer::Drag),
                tooltip::spec(hand),
            ]
        })
        .collect()
}

/// レーザー。手元では [`COLOR`] の濃さで、先（カーソル）へ向けて薄くなり、先で消える。
pub struct Laser;

/// 手元から `t`（0 = 手元、1 = 先）の不透明度の割合: 1 − sine-out。手元はしっかり見え、早めに薄れて先では 0。
/// 線の長さ（見える範囲）はオーバーレイ層の `laser_length` で決まり、この曲線はその長さに沿って伸び縮みする。
fn laser_fade(t: f32) -> f32 {
    1.0 - (t * std::f32::consts::FRAC_PI_2).sin()
}

impl Render for Laser {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let alpha = (COLOR & 0xff) as f32 / 255.0;
        let white = |t: f32| hsla(0.0, 0.0, 1.0, alpha * laser_fade(t));
        let n = LASER_FADE_STEPS as f32;
        div()
            .size_full()
            .flex()
            .children((0..LASER_FADE_STEPS).map(|i| {
                let (a, b) = (i as f32 / n, (i + 1) as f32 / n);
                div().flex_1().h_full().bg(linear_gradient(
                    90.0,
                    linear_color_stop(white(a), 0.0),
                    linear_color_stop(white(b), 1.0),
                ))
            }))
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
        let fill = design::Theme::current(cx).accent_solid;
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
                        move |bounds, _, window, _| ring(bounds, p, fill, window),
                    )
                    .absolute()
                    .size_full(),
                )
            })
    }
}

/// `bounds` の中心に、下地の輪と、12 時から時計回りに `progress`（0〜1）だけ満ちた弧（色 `fill`）を描く。
fn ring(bounds: Bounds<Pixels>, progress: f32, fill: u32, window: &mut Window) {
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
    if let Ok(fill_path) = arc(progress) {
        window.paint_path(fill_path, rgba(fill));
    }
}

/// 空。ドラッグ中は GPUI がドラッグの見た目をマウス位置（= 中心）に重ねて描く。
pub struct DragLayer;

impl Render for DragLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laser_fades_out_toward_the_cursor_along_sine_out() {
        assert_eq!(laser_fade(0.0), 1.0);
        assert!(laser_fade(1.0).abs() < 1e-6);
        // sine-out: 前半で大きく薄れる（半分の所で 3 割弱）。
        assert!((laser_fade(0.5) - (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-6);
        assert!((1..=10).all(|i| laser_fade(i as f32 / 10.0) < laser_fade((i - 1) as f32 / 10.0)));
    }

    #[test]
    fn tooltip_box_is_two_cursor_dots_tall() {
        // カーソルの中心の丸の直径（m、1m 先で）。
        const CURSOR_DOT_M: f32 = CURSOR_M * DOT_PX / CURSOR_PX.0 as f32;
        let box_m = design::components::TOOLTIP_HEIGHT * TOOLTIP_M / TOOLTIP_PX.0 as f32;
        assert!((box_m - 2.0 * CURSOR_DOT_M).abs() < 1e-6, "{box_m}");
    }
}
