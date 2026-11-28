//! 層を tiny-skia で描く。層のマスクと型（アルファマット）による切り抜きは、1 枚のマスクに掛け合わせて、描くときに渡す
//! （層ごとに別の画像へ描いて重ねるより速い）。別の画像に描くのは、型の層だけ。

use std::collections::HashMap;

use kurbo::{Affine, Shape, Size};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Mask, MaskType, Paint, PathBuilder, Pixmap, Transform,
};

use crate::model::{Cap, Join, Layer, LayerKind, Matte};
use crate::scene::{self, Op};

pub(crate) struct Ctx<'a> {
    pub(crate) assets: &'a HashMap<String, Vec<Layer>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// `layers` を奥から順に `target` へ描く。`base` は合成の座標から描く先の座標への変形、`clip` は親から受け継いだ切り抜き。
pub(crate) fn layers(
    ctx: &Ctx,
    layers: &[Layer],
    frame: f64,
    base: Affine,
    target: &mut Pixmap,
    clip: Option<&Mask>,
) {
    for i in (0..layers.len()).rev() {
        let layer = &layers[i];
        if layer.is_matte || !layer.visible(frame) {
            continue;
        }
        let mut own = combine(masks(ctx, layer, frame, base), clip);
        let matte = layer
            .matte
            .zip(i.checked_sub(1).map(|m| &layers[m]).filter(|m| m.is_matte));
        if let Some((kind, source)) = matte {
            let Some(mut alpha) = Pixmap::new(ctx.width, ctx.height) else {
                continue;
            };
            if source.visible(frame) {
                let source_clip = masks(ctx, source, frame, base);
                draw(ctx, source, frame, base, &mut alpha, source_clip.as_ref());
            }
            let mut mask = Mask::from_pixmap(alpha.as_ref(), MaskType::Alpha);
            if kind == Matte::AlphaInverted {
                mask.invert();
            }
            own = combine(Some(mask), own.as_ref());
        }
        draw(ctx, layer, frame, base, target, own.as_ref());
    }
}

/// 層のマスク（足し合わせ）。マスクがなければ `None`。
fn masks(ctx: &Ctx, layer: &Layer, frame: f64, base: Affine) -> Option<Mask> {
    if layer.masks.is_empty() {
        return None;
    }
    let mut mask = Mask::new(ctx.width, ctx.height)?;
    let m = base * layer.transform.matrix(frame - layer.start);
    for shape in &layer.masks {
        let mut builder = PathBuilder::new();
        scene::push_skia(
            &mut builder,
            &(m * shape.path.at(frame - layer.start).to_bez()),
        );
        if let Some(path) = builder.finish() {
            mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        }
    }
    Some(mask)
}

/// 2 つの切り抜きを掛け合わせる（両方に入る範囲だけを残す）。
fn combine(a: Option<Mask>, b: Option<&Mask>) -> Option<Mask> {
    match (a, b) {
        (Some(mut a), Some(b)) => {
            for (x, y) in a.data_mut().iter_mut().zip(b.data()) {
                *x = ((*x as u16 * *y as u16 + 127) / 255) as u8;
            }
            Some(a)
        }
        (a, b) => a.or_else(|| b.cloned()),
    }
}

/// 層の中身を、切り抜き `clip` を掛けて描く。
fn draw(
    ctx: &Ctx,
    layer: &Layer,
    frame: f64,
    base: Affine,
    target: &mut Pixmap,
    clip: Option<&Mask>,
) {
    let local = frame - layer.start;
    let m = base * layer.transform.matrix(local);
    match &layer.kind {
        LayerKind::Shapes(items) => {
            for op in scene::group(items, local, m).0 {
                paint(&op, target, clip);
            }
        }
        // ponytail: プリコンポの範囲（w, h）で切り抜かない。nyaw のアニメーションは合成と同じ大きさのプリコンポしか使わない
        LayerKind::Precomp { id } => {
            if let Some(children) = ctx.assets.get(id) {
                layers(ctx, children, local, m, target, clip);
            }
        }
    }
}

fn paint(op: &Op, target: &mut Pixmap, clip: Option<&Mask>) {
    if op.color[3] <= 0. || op.paths.is_empty() {
        return;
    }
    // 拡大率が 0 の部品は、全部の点が重なる。端が butt の線は何も描かれず、tiny-skia は輪郭を作れないと警告するので足さない。
    let butt = matches!(op.paint, scene::Paint::Stroke { cap: Cap::Butt, .. });
    let mut builder = PathBuilder::new();
    for p in &op.paths {
        if !(butt && p.bounding_box().size() == Size::ZERO) {
            scene::push_skia(&mut builder, p);
        }
    }
    let Some(path) = builder.finish() else { return };
    let mut brush = Paint::default();
    let [r, g, b, a] = op.color.map(|c| c.clamp(0., 1.) as f32);
    brush.set_color_rgba8(
        (r * 255.).round() as u8,
        (g * 255.).round() as u8,
        (b * 255.).round() as u8,
        (a * 255.).round() as u8,
    );
    brush.anti_alias = true;
    match op.paint {
        scene::Paint::Fill { even_odd } => {
            let rule = if even_odd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
            target.fill_path(&path, &brush, rule, Transform::identity(), clip);
        }
        scene::Paint::Stroke { width, cap, join } => {
            if width <= 0. {
                return;
            }
            let stroke = tiny_skia::Stroke {
                width: width as f32,
                line_cap: match cap {
                    Cap::Butt => LineCap::Butt,
                    Cap::Round => LineCap::Round,
                    Cap::Square => LineCap::Square,
                },
                line_join: match join {
                    Join::Miter => LineJoin::Miter,
                    Join::Round => LineJoin::Round,
                    Join::Bevel => LineJoin::Bevel,
                },
                ..Default::default()
            };
            target.stroke_path(&path, &brush, &stroke, Transform::identity(), clip);
        }
    }
}

/// 押した位置 `point`（合成の座標）にある、`cl` の付いた塗りか線を、手前から探す。
pub(crate) fn hit<'a>(
    assets: &'a HashMap<String, Vec<Layer>>,
    layers: &'a [Layer],
    frame: f64,
    base: Affine,
    point: kurbo::Point,
) -> Option<String> {
    for layer in layers {
        if layer.is_matte || !layer.visible(frame) {
            continue;
        }
        let local = frame - layer.start;
        let m = base * layer.transform.matrix(local);
        let found = match &layer.kind {
            LayerKind::Shapes(items) => scene::group(items, local, m)
                .0
                .into_iter()
                .rev()
                .find(|op| op.class.is_some() && scene::contains(op, point))
                .and_then(|op| op.class),
            LayerKind::Precomp { id } => assets
                .get(id)
                .and_then(|children| hit(assets, children, local, m, point)),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}
