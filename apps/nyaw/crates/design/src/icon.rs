//! Phosphor アイコン。SVG の path を gpui の `Path` に変換して描画するため、色・グラデーション・不透明度・回転・反転を自由に適用できる。
//!
//! ```ignore
//! icon(&icons::HEART).weight(Weight::Fill).size(px(20.)).color(rgba(theme.danger_solid))
//! ```
//!
//! - アイコンは `icons::HEART` のような定数（build.rs が生成）。**ソースで使用されているもののみ**がビルドに含まれる。
//!   Phosphor に存在しない名前はビルドエラーになる。
//! - ウェイトは Phosphor の 6 種類すべて（[`Weight`]）。同様にソースに記述されたもののみが含まれる
//!   （含まれていないウェイトは描画されない）。実行時に選択する場合は `Weight::ALL` をどこかに記述すれば全種類含まれる。
//! - 色を省略すると周囲の文字色（SVG の `currentColor`）。[`IconView::color`] は [`Background`] を受け取るため、
//!   グラデーション等の塗りでアイコン形状をマスクすることも可能。duotone の薄い面は塗りの不透明度 0.2 倍。

use gpui::{
    App, Background, Bounds, FillOptions, Path, PathBuilder, PathStyle, Pixels, Point, Window,
    canvas, point, prelude::*, px,
};

/// path の 1 セグメント。座標は Phosphor の 256×256 座標系。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// 1 枚の塗りレイヤー。duotone の薄い面は `opacity` が 0.2。
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub opacity: f32,
    pub segs: &'static [Seg],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weight {
    Thin,
    Light,
    #[default]
    Regular,
    Bold,
    Fill,
    Duotone,
}

impl Weight {
    pub const ALL: [Weight; 6] = [
        Weight::Thin,
        Weight::Light,
        Weight::Regular,
        Weight::Bold,
        Weight::Fill,
        Weight::Duotone,
    ];
}

/// 生成された 1 アイコン分のデータ。ウェイトごとに [`Layer`] の列を保持する（未使用ウェイトは空）。
#[derive(Debug)]
pub struct Icon {
    pub name: &'static str,
    pub thin: &'static [Layer],
    pub light: &'static [Layer],
    pub regular: &'static [Layer],
    pub bold: &'static [Layer],
    pub fill: &'static [Layer],
    pub duotone: &'static [Layer],
}

impl Icon {
    pub fn layers(&self, weight: Weight) -> &'static [Layer] {
        match weight {
            Weight::Thin => self.thin,
            Weight::Light => self.light,
            Weight::Regular => self.regular,
            Weight::Bold => self.bold,
            Weight::Fill => self.fill,
            Weight::Duotone => self.duotone,
        }
    }
}

const VIEW_BOX: f32 = 256.0;

/// 描画するアイコン。`RenderOnce` のため `.child(icon(&icons::HEART))` のように配置する。
#[derive(IntoElement)]
pub struct IconView {
    icon: &'static Icon,
    weight: Weight,
    size: Pixels,
    color: Option<Background>,
    opacity: f32,
    rotate: f32,
    flip: (bool, bool),
}

pub fn icon(icon: &'static Icon) -> IconView {
    IconView {
        icon,
        weight: Weight::Regular,
        size: px(16.0),
        color: None,
        opacity: 1.0,
        rotate: 0.0,
        flip: (false, false),
    }
}

impl IconView {
    pub fn weight(mut self, weight: Weight) -> Self {
        self.weight = weight;
        self
    }

    /// 一辺の長さ（正方形）を設定する。
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// 塗り。単色（`rgba(..)` / `Hsla`）でもグラデーションでもよい。省略時は周囲の文字色。
    pub fn color(mut self, color: impl Into<Background>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// 中心を軸に時計回りに回転させる（度）。
    pub fn rotate(mut self, degrees: f32) -> Self {
        self.rotate = degrees;
        self
    }

    pub fn flip_x(mut self) -> Self {
        self.flip.0 = !self.flip.0;
        self
    }

    pub fn flip_y(mut self) -> Self {
        self.flip.1 = !self.flip.1;
        self
    }

    /// 左上が `origin` のときの塗り（`Path` とその不透明度）。描画と同一のデータを返す。
    // ponytail: 描画のたびに三角形分割する（アイコン 1 個あたり数十 µs）。数百個を毎フレーム描画するなら (アイコン, サイズ, 回転) で Path をキャッシュする。
    pub fn paths(&self, origin: Point<Pixels>) -> Vec<(Path<Pixels>, f32)> {
        let size = f32::from(self.size);
        let s = size / VIEW_BOX;
        let (sin, cos) = self.rotate.to_radians().sin_cos();
        let (fx, fy) = (
            if self.flip.0 { -1.0 } else { 1.0 },
            if self.flip.1 { -1.0 } else { 1.0 },
        );
        let at = |x: f32, y: f32| {
            let (u, v) = ((x - VIEW_BOX / 2.0) * fx, (y - VIEW_BOX / 2.0) * fy);
            point(
                px(f32::from(origin.x) + size / 2.0 + (u * cos - v * sin) * s),
                px(f32::from(origin.y) + size / 2.0 + (u * sin + v * cos) * s),
            )
        };
        self.icon
            .layers(self.weight)
            .iter()
            .filter_map(|layer| {
                // 塗りルールは SVG デフォルトの nonzero（lyon のデフォルトは evenodd）
                let mut b =
                    PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()));
                for seg in layer.segs {
                    match *seg {
                        Seg::M(x, y) => b.move_to(at(x, y)),
                        Seg::L(x, y) => b.line_to(at(x, y)),
                        Seg::Q(x1, y1, x, y) => b.curve_to(at(x, y), at(x1, y1)),
                        Seg::C(x1, y1, x2, y2, x, y) => {
                            b.cubic_bezier_to(at(x, y), at(x1, y1), at(x2, y2))
                        }
                        Seg::Z => b.close(),
                    }
                }
                b.build().ok().map(|p| (p, layer.opacity))
            })
            .collect()
    }
}

impl RenderOnce for IconView {
    fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // アイコンは文字と並ぶため、文字サイズ倍率に追従する（`.text_exact()` 内では等倍）。
        self.size *= gpui::text_scale();
        let size = self.size;
        canvas(
            |_, _, _| (),
            move |bounds: Bounds<Pixels>, _, window, _| {
                let color = self
                    .color
                    .unwrap_or_else(|| window.text_style().color.into());
                for (path, layer_opacity) in self.paths(bounds.origin) {
                    window.paint_path(path, color.opacity(self.opacity * layer_opacity));
                }
            },
        )
        .size(size)
        .flex_none()
    }
}
