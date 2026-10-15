//! Phosphor アイコン。SVG の path を gpui の `Path` にして描くので、色・グラデーション・不透明度・回転・反転が自由に効く。
//!
//! ```ignore
//! icon(&icons::HEART).weight(Weight::Fill).size(px(20.)).color(rgba(theme.danger_solid))
//! ```
//!
//! - アイコンは `icons::HEART` のような定数（build.rs が生成）。**ソースで使われているものだけ**がビルドに入る。
//!   Phosphor に無い名前はビルドが落ちる。
//! - ウェイトは Phosphor の 6 種類すべて（[`Weight`]）。これも `Weight::Bold` のようにソースに書かれたものだけが入る
//!   （入っていないウェイトは何も描かれない）。実行時に選ぶ場合は `Weight::ALL` をどこかに書けば全部入る。
//! - 色を省くと周りの文字色（SVG の `currentColor`）。[`IconView::color`] は [`Background`] を取るので、
//!   グラデーションなどの塗りでアイコンの形に抜く（マスク）こともできる。duotone の薄い面は塗りの不透明度 0.2 倍。

use gpui::{
    App, Background, Bounds, FillOptions, Path, PathBuilder, PathStyle, Pixels, Point, Window,
    canvas, point, prelude::*, px,
};

/// path の 1 区間。座標は Phosphor の 256×256 の座標系。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// 1 枚の塗り。duotone の薄い面は `opacity` が 0.2。
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

/// 生成された 1 アイコン分のデータ。ウェイトごとに [`Layer`] の列を持つ（使われていないウェイトは空）。
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

/// 描くアイコン。`RenderOnce` なので `.child(icon(&icons::HEART))` のように置く。
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

    /// 一辺の長さ（正方形）。
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// 塗り。単色（`rgba(..)` / `Hsla`）でもグラデーションでもよい。省くと周りの文字色。
    pub fn color(mut self, color: impl Into<Background>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// 中心まわりに時計回りへ回す（度）。
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

    /// 左上が `origin` のときの塗り（`Path` と、その不透明度）。描画に使うのと同じもの。
    // ponytail: 描くたびに三角形分割する(アイコン 1 つ数十 µs)。数百個を毎フレーム描くなら (アイコン, サイズ, 回転) で Path を使い回す。
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
                // 塗りの規則は SVG の既定の nonzero（lyon の既定は evenodd）
                let mut b = PathBuilder::fill()
                    .with_style(PathStyle::Fill(FillOptions::non_zero()));
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
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
