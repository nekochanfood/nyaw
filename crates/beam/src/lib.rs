//! Layer 6 パネル: コントローラのビーム。手ごとに、手元から伸びるレーザー、指示先パネルに貼り付く
//! カーソル（小円）、ドラッグ中の要素の外観（中身は GPUI が描画する `ui::DragPreview`）、カーソル子要素のツールチップ（[`tooltip`]）、
//! ドッキング可能パネル把持中のスロットゴースト（[`dock`]。角枠は [`bracket`]）、角で大きさを変えられるパネルの角を囲む線（[`corner`]）の計 6 枚。描画のみを担い、配置・長さ・表示制御はオーバーレイ層が毎 tick 決定する。

use gpui::{
    Bounds, Context, PathBuilder, Pixels, Point, Window, canvas, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgba,
};
use kernel::overlay::{LASER_LENGTH, LASER_MAX};
use kernel::{Hand, Pointer, Pose, Vec2};
use ui::PanelSpec;

pub mod bracket;
pub mod corner;
pub mod dock;
pub mod tooltip;
pub use corner::CornerHandle;
pub use dock::DockGhost;
pub use tooltip::Tooltip;

const COLOR: u32 = 0xffffffd0;
const OUTLINE: u32 = 0x000000a0;

/// レーザーのテクスチャ。横方向がレーザーの向き（左端 = 手元）で、オーバーレイ層が幅を長さに伸張し、
/// 高さは長さによらず [`LASER_THICK_M`] に固定する。線をテクスチャ全面に描画するため線の縁 = クアッドの縁となり、
/// 拡大しても滲まない（細い線を透明領域内に描くと、拡大時の補間で縁がぼける）。
/// 縦の画素数は Windows の非表示ウィンドウの最小サイズ制約による。
pub const LASER_PX: (u32, u32) = (1024, 64);
/// レーザーの太さ（m 単位）。
const LASER_THICK_M: f32 = 0.003;
/// レーザーの不透明度の分割数。各区間を線形グラデーションで接続し、曲線を近似する。
const LASER_FADE_STEPS: usize = 16;
/// カーソルパネルの幅（m）。頭部から 1m 先での基準サイズで、距離に比例して拡大し見かけの大きさを維持する。
/// 中心の丸（直径はこの 1/3 = 12mm）の周囲に、長押し進行リングの描画余白を確保してある。
const CURSOR_M: f32 = 0.036;
/// Windows の非表示ウィンドウの最小サイズ制約があるため、これ以上小さくしない。
const CURSOR_PX: (u32, u32) = (240, 240);
/// 中心の丸の直径（px）とリングの半径（線の中心）・太さ（px）。丸はリングの内径に収まる。
const DOT_PX: f32 = 80.0;
const RING_RADIUS_PX: f32 = 80.0;
const RING_WIDTH_PX: f32 = 18.0;
/// リングのトラック色（薄い）。充填部分はテーマの強調色（`accent-solid`。白では白い面上で不可視になるため）。
const RING_TRACK: u32 = 0x00000060;
/// この秒数より短い押下（タップ）ではリングを表示しない（ちらつき防止）。
const RING_SHOW: f32 = 0.15;
/// ドラッグプレビューを描画するウィンドウ。中心がカーソル位置。幅（m）はドラッグ開始時のパネルスケールに基づき UI 層が決定する。
const DRAG_PX: (u32, u32) = (400, 400);
/// ツールチップのパネル。中心がカーソル位置で、カーソルと同様に頭部から 1m 先での基準サイズ（m）。
/// オーバーレイ層が距離に比例して拡大するため、距離によらず見かけの大きさが一定になる。
/// ボックス高さ（`TOOLTIP_HEIGHT` px）が `TOOLTIP_HEIGHT_M`（カーソル丸 2 個分の高さ）に対応するスケールに設定。
const TOOLTIP_PX: (u32, u32) = (640, 256);
const TOOLTIP_M: f32 =
    TOOLTIP_PX.0 as f32 * design::components::TOOLTIP_HEIGHT_M / design::components::TOOLTIP_HEIGHT;

/// 両手のレーザー・カーソル・ドラッグプレビューの各パネル仕様。
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
                dock::spec(hand),
                corner::spec(hand),
            ]
        })
        .collect()
}

/// レーザー。手元では [`COLOR`] の濃度で、先端（カーソル方向）へ向けて減衰し、先端で完全に消える。
pub struct Laser;

/// 手元からの位置 `t`（0 = 手元、1 = 先端）における不透明度: 1 − sine-out。手元は明瞭で、早期に減衰し先端では 0 になる。
/// 可視長はオーバーレイ層の `laser_length` が決定し、この曲線はその長さに沿って伸縮する。
fn laser_fade(t: f32) -> f32 {
    1.0 - (t * std::f32::consts::FRAC_PI_2).sin()
}

impl Render for Laser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let alpha = (COLOR & 0xff) as f32 / 255.0;
        let white = move |t: f32| hsla(0.0, 0.0, 1.0, alpha * laser_fade(t));
        let n = LASER_FADE_STEPS as f32;
        // 手元の端は丸める（半楕円。テクスチャの上では横に縮めて描き、空間で半円に見えるようにする）。
        let cap = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| paint_cap(bounds, white(0.0), window),
        )
        .w(px(cap_px(ui::settings(cx).laser_length)))
        .h_full()
        .flex_none();
        div()
            .size_full()
            .flex()
            .child(cap)
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

/// 手元の端の丸み（半楕円）の横の半径（テクスチャの px）。テクスチャは横だけがレーザーの見える長さ `length`（m）に伸びるので、
/// 太さの半分をその縮尺で割り戻し、空間で半円に見える幅にする。設定が届く前（0）は既定の長さで求める。
// ponytail: 見える長さより近くを指すとレーザーが縮み、丸みも横に縮む。目立つなら今の長さを UI へ渡して描き直す。
fn cap_px(length: f32) -> f32 {
    let length = if length > 0.0 { length } else { LASER_LENGTH };
    (LASER_THICK_M / 2.0 * LASER_PX.0 as f32 / length)
        .round()
        .max(1.0)
}

/// `bounds` の幅を横の半径、高さの半分を縦の半径とする、左向きの半楕円を `color` で塗る。
fn paint_cap(bounds: Bounds<Pixels>, color: gpui::Hsla, window: &mut Window) {
    const STEPS: usize = 16;
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (rx, ry) = (
        f32::from(bounds.size.width),
        f32::from(bounds.size.height) / 2.0,
    );
    let mut path = PathBuilder::fill();
    // 右上から左端を回って右下まで。
    path.move_to(point(px(x + rx), px(y)));
    for i in 1..=STEPS {
        let a = std::f32::consts::PI * i as f32 / STEPS as f32;
        path.line_to(point(px(x + rx - rx * a.sin()), px(y + ry - ry * a.cos())));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// カーソル。中心の丸と、タッチ長押し中はその進行度を充填リング（ドーナツ型。丸は内径に収まる）で表示する。
/// リングが充填完了するとマウス押下に変換され、ドラッグ＆ドロップが可能になる。
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
            // 白いパネル上でも視認できるよう縁取りを付ける。
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

/// `bounds` の中心に、トラック環と 12 時から時計回りに `progress`（0〜1）分充填された弧（色 `fill`）を描画する。
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

/// 空コンテナ。ドラッグ中は GPUI がドラッグプレビューをマウス位置（= 中心）にオーバーレイ描画する。
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
    fn the_laser_cap_looks_round_in_space() {
        // 横の縮尺（見える長さ ÷ 横の画素数）で空間の長さに戻すと、丸みの横の半径は太さの半分になる（空間で半円）。
        let m_per_px = LASER_LENGTH / LASER_PX.0 as f32;
        let rx = cap_px(LASER_LENGTH) * m_per_px;
        assert!((rx - LASER_THICK_M / 2.0).abs() <= m_per_px / 2.0, "{rx}");
        assert_eq!(cap_px(0.0), cap_px(LASER_LENGTH));
    }

    #[test]
    fn laser_fades_out_toward_the_cursor_along_sine_out() {
        assert_eq!(laser_fade(0.0), 1.0);
        assert!(laser_fade(1.0).abs() < 1e-6);
        // sine-out: 前半で大幅に減衰する（中間点で約 3 割弱）。
        assert!((laser_fade(0.5) - (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-6);
        assert!((1..=10).all(|i| laser_fade(i as f32 / 10.0) < laser_fade((i - 1) as f32 / 10.0)));
    }

    #[test]
    fn tooltip_box_is_two_cursor_dots_tall() {
        // カーソル中心丸の直径（m、1m 先での基準値）。
        const CURSOR_DOT_M: f32 = CURSOR_M * DOT_PX / CURSOR_PX.0 as f32;
        let box_m = design::components::TOOLTIP_HEIGHT * TOOLTIP_M / TOOLTIP_PX.0 as f32;
        assert!((box_m - 2.0 * CURSOR_DOT_M).abs() < 1e-6, "{box_m}");
    }
}
