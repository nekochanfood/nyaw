//! 指定フレームのシェイプを、描く順（奥から手前）の塗りと線の並びにする。
//!
//! Lottie の規則: 塗りや線は、同じグループの中でそれより前にある形（入れ子のグループの形を含む）に掛かる。
//! トリムも、それより前にある形を切る。項目は前にあるものほど手前に描く。

use kurbo::{
    Affine, BezPath, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, Rect, Shape as _,
};

use crate::model::{Cap, Item, Join};

/// 1 回の塗りか線。形はすべて描く先の座標にしてある。
#[derive(Clone, Debug)]
pub(crate) struct Op {
    pub(crate) paths: Vec<BezPath>,
    pub(crate) paint: Paint,
    /// 色 RGBA（0〜1。不透明度を掛けた後）。
    pub(crate) color: [f64; 4],
    /// JSON の `cl`。押した位置の判定に使う。
    pub(crate) class: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Paint {
    Fill { even_odd: bool },
    Stroke { width: f64, cap: Cap, join: Join },
}

/// 曲線の近似の許容誤差（描く先の座標、ピクセル）。
const TOLERANCE: f64 = 0.05;

/// グループ（または層）の項目を、親の変形 `parent` のもとで組み立てる。
/// 戻り値は、奥から手前の順の描画と、このグループの形すべて（親の塗りや線が使う）。
pub(crate) fn group(items: &[Item], frame: f64, parent: Affine) -> (Vec<Op>, Vec<BezPath>) {
    let m = parent
        * items
            .iter()
            .find_map(|i| match i {
                Item::Transform(t) => Some(t.matrix(frame)),
                _ => None,
            })
            .unwrap_or(Affine::IDENTITY);
    let n = items.len();
    let mut geometry: Vec<Vec<BezPath>> = vec![Vec::new(); n];
    let mut nested: Vec<Vec<Op>> = vec![Vec::new(); n];
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Path(shape) => geometry[i] = vec![m * shape.at(frame).to_bez()],
            Item::Ellipse { center, size } => {
                let (c, s) = (center.at(frame), size.at(frame));
                let ellipse =
                    kurbo::Ellipse::new(Point::new(c[0], c[1]), (s[0] / 2., s[1] / 2.), 0.);
                geometry[i] = vec![m * ellipse.to_path(TOLERANCE)];
            }
            Item::Rect { center, size } => {
                let (c, s) = (center.at(frame), size.at(frame));
                let rect = Rect::from_center_size(Point::new(c[0], c[1]), (s[0], s[1]));
                geometry[i] = vec![m * rect.to_path(TOLERANCE)];
            }
            Item::Group { items } => (nested[i], geometry[i]) = group(items, frame, m),
            Item::Trim { start, end, offset } => {
                let (s, e, o) = (
                    start.scalar(frame) / 100.,
                    end.scalar(frame) / 100.,
                    offset.scalar(frame) / 360.,
                );
                for paths in &mut geometry[..i] {
                    for path in paths.iter_mut() {
                        *path = trim(path, s + o, e + o);
                    }
                }
            }
            _ => {}
        }
    }
    let scale = m.determinant().abs().sqrt();
    let mut ops = Vec::new();
    for i in (0..n).rev() {
        let above = || geometry[..i].iter().flatten().cloned().collect::<Vec<_>>();
        match &items[i] {
            Item::Group { .. } => ops.append(&mut nested[i]),
            Item::Fill {
                color,
                opacity,
                even_odd,
                class,
            } => {
                ops.push(Op {
                    paths: above(),
                    paint: Paint::Fill {
                        even_odd: *even_odd,
                    },
                    color: rgba(color.at(frame), opacity.scalar(frame)),
                    class: class.clone(),
                });
            }
            Item::Stroke {
                color,
                opacity,
                width,
                cap,
                join,
                class,
            } => ops.push(Op {
                paths: above(),
                paint: Paint::Stroke {
                    width: width.scalar(frame) * scale,
                    cap: *cap,
                    join: *join,
                },
                color: rgba(color.at(frame), opacity.scalar(frame)),
                class: class.clone(),
            }),
            _ => {}
        }
    }
    (ops, geometry.into_iter().flatten().collect())
}

fn rgba(c: Vec<f64>, opacity: f64) -> [f64; 4] {
    let get = |k: usize, d: f64| c.get(k).copied().unwrap_or(d);
    [
        get(0, 0.),
        get(1, 0.),
        get(2, 0.),
        get(3, 1.) * (opacity / 100.).clamp(0., 1.),
    ]
}

/// パスの長さのうち、割合 `from`〜`to`（0〜1、超えた分は先頭へ回り込む）の部分だけを残す。
pub(crate) fn trim(path: &BezPath, from: f64, to: f64) -> BezPath {
    if (to - from).abs() >= 1. - 1e-9 && from <= to {
        return path.clone();
    }
    if to <= from {
        return BezPath::new();
    }
    let segments: Vec<PathSeg> = path.segments().collect();
    let lengths: Vec<f64> = segments.iter().map(|s| s.arclen(1e-3)).collect();
    let total: f64 = lengths.iter().sum();
    if total <= 0. {
        return BezPath::new();
    }
    // 0〜1 を超えた範囲は、長さの上で 2 つに分けて先頭へ回り込ませる
    let (a, b) = (from.rem_euclid(1.), from.rem_euclid(1.) + (to - from));
    let mut out = BezPath::new();
    for (lo, hi) in [(a, b.min(1.)), (0., (b - 1.).max(0.))] {
        if hi > lo {
            append_range(&mut out, &segments, &lengths, lo * total, hi * total);
        }
    }
    out
}

/// 長さの上で `lo`〜`hi` の範囲の部分パスを `out` に足す。
fn append_range(out: &mut BezPath, segments: &[PathSeg], lengths: &[f64], lo: f64, hi: f64) {
    let mut at = 0.;
    let mut started = false;
    for (seg, &len) in segments.iter().zip(lengths) {
        let (s0, s1) = (at, at + len);
        at = s1;
        if s1 <= lo || s0 >= hi || len <= 0. {
            continue;
        }
        let t0 = if lo > s0 {
            seg.inv_arclen(lo - s0, 1e-3)
        } else {
            0.
        };
        let t1 = if hi < s1 {
            seg.inv_arclen(hi - s0, 1e-3)
        } else {
            1.
        };
        let part = seg.subsegment(t0..t1);
        if !started {
            out.move_to(part.start());
            started = true;
        }
        out.push(part.as_path_el());
    }
}

/// 点が描画の範囲に入るか（押した位置の判定）。
pub(crate) fn contains(op: &Op, point: Point) -> bool {
    match op.paint {
        Paint::Fill { even_odd } => {
            let winding: i32 = op.paths.iter().map(|p| p.winding(point)).sum();
            if even_odd {
                winding % 2 != 0
            } else {
                winding != 0
            }
        }
        Paint::Stroke { width, cap, join } => {
            let style = kurbo::Stroke::new(width)
                .with_caps(match cap {
                    Cap::Butt => kurbo::Cap::Butt,
                    Cap::Round => kurbo::Cap::Round,
                    Cap::Square => kurbo::Cap::Square,
                })
                .with_join(match join {
                    Join::Miter => kurbo::Join::Miter,
                    Join::Round => kurbo::Join::Round,
                    Join::Bevel => kurbo::Join::Bevel,
                });
            op.paths.iter().any(|p| {
                let outline =
                    kurbo::stroke(p.iter(), &style, &kurbo::StrokeOpts::default(), TOLERANCE);
                outline.winding(point) != 0
            })
        }
    }
}

/// パスの要素を、tiny-skia のパスに足す。
pub(crate) fn push_skia(builder: &mut tiny_skia::PathBuilder, path: &BezPath) {
    let f = |p: Point| (p.x as f32, p.y as f32);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = f(p);
                builder.move_to(x, y);
            }
            PathEl::LineTo(p) => {
                let (x, y) = f(p);
                builder.line_to(x, y);
            }
            PathEl::QuadTo(a, p) => {
                let ((ax, ay), (x, y)) = (f(a), f(p));
                builder.quad_to(ax, ay, x, y);
            }
            PathEl::CurveTo(a, b, p) => {
                let ((ax, ay), (bx, by), (x, y)) = (f(a), f(b), f(p));
                builder.cubic_to(ax, ay, bx, by, x, y);
            }
            PathEl::ClosePath => builder.close(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Line;

    fn line(len: f64) -> BezPath {
        Line::new((0., 0.), (len, 0.)).to_path(0.1)
    }

    fn length(p: &BezPath) -> f64 {
        p.segments().map(|s| s.arclen(1e-6)).sum()
    }

    #[test]
    fn trim_keeps_the_requested_share_of_length() {
        assert!((length(&trim(&line(100.), 0.2, 0.7)) - 50.).abs() < 1e-6);
        assert!(trim(&line(100.), 0.5, 0.5).elements().is_empty());
        assert!((length(&trim(&line(100.), 0., 1.)) - 100.).abs() < 1e-6);
    }

    #[test]
    fn trim_past_the_end_wraps_to_the_start() {
        let p = trim(&line(100.), 0.8, 1.1);
        assert!((length(&p) - 30.).abs() < 1e-6);
    }
}
