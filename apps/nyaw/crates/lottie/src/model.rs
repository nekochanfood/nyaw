//! Lottie の JSON から読む構造。対応しない機能は、黙って無視せず [`Error::Unsupported`] にする（描画が静かに崩れないように）。

use std::collections::HashMap;

use kurbo::{Affine, BezPath, Point, Vec2};
use serde_json::Value;

use crate::Error;
use crate::value::{Prop, cubic_bezier};

/// 層とシェイプのグループが持つ変形（アンカー、位置、拡大率 %、回転 度）。不透明度は扱わない。
#[derive(Clone, Debug)]
pub(crate) struct Transform {
    anchor: Prop,
    position: Prop,
    scale: Prop,
    rotation: Prop,
}

impl Transform {
    fn parse(v: &Value) -> Result<Self, Error> {
        let prop = |name: &str, default: Vec<f64>| match v.get(name) {
            Some(p) => Prop::parse(p),
            None => Ok(Prop::Static(default)),
        };
        Ok(Transform {
            anchor: prop("a", vec![0., 0.])?,
            position: prop("p", vec![0., 0.])?,
            scale: prop("s", vec![100., 100.])?,
            rotation: prop("r", vec![0.])?,
        })
    }

    /// 位置へ移し、回転し、拡大し、アンカーを原点へ寄せる変形。
    pub(crate) fn matrix(&self, frame: f64) -> Affine {
        let a = self.anchor.at(frame);
        let p = self.position.at(frame);
        let s = self.scale.at(frame);
        let r = self.rotation.scalar(frame);
        Affine::translate(Vec2::new(p[0], p[1]))
            * Affine::rotate(r.to_radians())
            * Affine::scale_non_uniform(s[0] / 100., s.get(1).copied().unwrap_or(s[0]) / 100.)
            * Affine::translate(Vec2::new(-a[0], -a[1]))
    }
}

/// パスの形（頂点と、頂点からの相対の制御点）。
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    closed: bool,
    v: Vec<[f64; 2]>,
    i: Vec<[f64; 2]>,
    o: Vec<[f64; 2]>,
}

impl Shape {
    fn parse(v: &Value) -> Result<Self, Error> {
        let points = |name: &str| -> Result<Vec<[f64; 2]>, Error> {
            v.get(name)
                .and_then(Value::as_array)
                .ok_or(Error::Format("パスの頂点がない"))?
                .iter()
                .map(|p| {
                    match p
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
                    {
                        Some(a) if a.len() >= 2 => Ok([a[0], a[1]]),
                        _ => Err(Error::Format("パスの頂点が数の組でない")),
                    }
                })
                .collect()
        };
        let shape = Shape {
            closed: v.get("c").and_then(Value::as_bool).unwrap_or(false),
            v: points("v")?,
            i: points("i")?,
            o: points("o")?,
        };
        if shape.i.len() != shape.v.len() || shape.o.len() != shape.v.len() {
            return Err(Error::Format("パスの頂点と制御点の数が違う"));
        }
        Ok(shape)
    }

    pub(crate) fn to_bez(&self) -> BezPath {
        let mut path = BezPath::new();
        let Some(&first) = self.v.first() else {
            return path;
        };
        let pt = |p: [f64; 2]| Point::new(p[0], p[1]);
        let rel = |p: [f64; 2], d: [f64; 2]| Point::new(p[0] + d[0], p[1] + d[1]);
        path.move_to(pt(first));
        let n = self.v.len();
        let last = if self.closed { n } else { n - 1 };
        for k in 0..last {
            let (from, to) = (k, (k + 1) % n);
            path.curve_to(
                rel(self.v[from], self.o[from]),
                rel(self.v[to], self.i[to]),
                pt(self.v[to]),
            );
        }
        if self.closed {
            path.close_path();
        }
        path
    }

    fn lerp(&self, to: &Shape, u: f64) -> Shape {
        let mix = |a: &[[f64; 2]], b: &[[f64; 2]]| {
            a.iter()
                .zip(b)
                .map(|(a, b)| [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u])
                .collect()
        };
        Shape {
            closed: self.closed,
            v: mix(&self.v, &to.v),
            i: mix(&self.i, &to.i),
            o: mix(&self.o, &to.o),
        }
    }
}

/// 時間で変わるパスの形。キーフレームの間は、頂点の数が同じときだけ補間する（違えば前の形を保つ）。
#[derive(Clone, Debug)]
pub(crate) enum ShapeProp {
    Static(Shape),
    Keyed(Vec<(f64, Shape, bool, [f64; 4])>),
}

impl ShapeProp {
    fn parse(v: &Value) -> Result<Self, Error> {
        let k = v.get("k").ok_or(Error::Format("パスに k がない"))?;
        if v.get("a").and_then(Value::as_i64) != Some(1) {
            return Ok(ShapeProp::Static(Shape::parse(k)?));
        }
        let keys = k
            .as_array()
            .ok_or(Error::Format("パスのキーフレームが配列でない"))?;
        let keys = keys
            .iter()
            .map(|key| {
                let t = key
                    .get("t")
                    .and_then(Value::as_f64)
                    .ok_or(Error::Format("キーフレームに t がない"))?;
                let s = key
                    .get("s")
                    .and_then(|s| s.get(0))
                    .ok_or(Error::Format("パスのキーフレームに s がない"))?;
                let axis = |name: &str, a: &str, d: f64| {
                    key.get(name)
                        .and_then(|e| e.get(a))
                        .and_then(|a| a.get(0).and_then(Value::as_f64).or(a.as_f64()))
                        .unwrap_or(d)
                };
                let ease = [
                    axis("o", "x", 0.),
                    axis("o", "y", 0.),
                    axis("i", "x", 1.),
                    axis("i", "y", 1.),
                ];
                Ok((
                    t,
                    Shape::parse(s)?,
                    key.get("h").and_then(Value::as_i64) == Some(1),
                    ease,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        if keys.is_empty() {
            return Err(Error::Format("パスのキーフレームが空"));
        }
        Ok(ShapeProp::Keyed(keys))
    }

    pub(crate) fn at(&self, frame: f64) -> Shape {
        match self {
            ShapeProp::Static(s) => s.clone(),
            ShapeProp::Keyed(keys) => {
                let next = keys.partition_point(|k| k.0 <= frame);
                if next == 0 {
                    return keys[0].1.clone();
                }
                let (t, from, hold, ease) = &keys[next - 1];
                match keys.get(next) {
                    Some((t2, to, _, _)) if !hold && to.v.len() == from.v.len() => {
                        from.lerp(to, cubic_bezier(*ease, (frame - t) / (t2 - t)))
                    }
                    _ => from.clone(),
                }
            }
        }
    }
}

/// 線の端と角。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Join {
    Miter,
    Round,
    Bevel,
}

/// シェイプレイヤーの中身の項目。
#[derive(Clone, Debug)]
pub(crate) enum Item {
    /// 変形は、項目の中の [`Item::Transform`]。
    Group {
        items: Vec<Item>,
    },
    Path(ShapeProp),
    Ellipse {
        center: Prop,
        size: Prop,
    },
    Rect {
        center: Prop,
        size: Prop,
    },
    Fill {
        color: Prop,
        opacity: Prop,
        even_odd: bool,
        class: Option<String>,
    },
    Stroke {
        color: Prop,
        opacity: Prop,
        width: Prop,
        cap: Cap,
        join: Join,
        class: Option<String>,
    },
    Trim {
        start: Prop,
        end: Prop,
        offset: Prop,
    },
    /// グループの変形（グループの項目の最後に置かれる）。
    Transform(Transform),
}

fn items(list: &Value) -> Result<Vec<Item>, Error> {
    list.as_array()
        .ok_or(Error::Format("シェイプが配列でない"))?
        .iter()
        .map(item)
        .collect()
}

fn item(v: &Value) -> Result<Item, Error> {
    let ty = v.get("ty").and_then(Value::as_str).unwrap_or("");
    let prop = |name: &str| {
        Prop::parse(
            v.get(name)
                .ok_or(Error::Format("シェイプの項目に値がない"))?,
        )
    };
    let class = v.get("cl").and_then(Value::as_str).map(str::to_owned);
    Ok(match ty {
        "gr" => Item::Group {
            items: items(v.get("it").ok_or(Error::Format("グループに it がない"))?)?,
        },
        "sh" => Item::Path(ShapeProp::parse(
            v.get("ks").ok_or(Error::Format("パスに ks がない"))?,
        )?),
        "el" => Item::Ellipse {
            center: prop("p")?,
            size: prop("s")?,
        },
        "rc" => {
            if v.get("r")
                .is_some_and(|r| Prop::parse(r).map(|r| r.scalar(0.) != 0.).unwrap_or(true))
            {
                return Err(Error::Unsupported("角の丸い矩形".into()));
            }
            Item::Rect {
                center: prop("p")?,
                size: prop("s")?,
            }
        }
        "fl" => Item::Fill {
            color: prop("c")?,
            opacity: prop("o")?,
            even_odd: v.get("r").and_then(Value::as_i64) == Some(2),
            class,
        },
        "st" => Item::Stroke {
            color: prop("c")?,
            opacity: prop("o")?,
            width: prop("w")?,
            cap: match v.get("lc").and_then(Value::as_i64) {
                Some(1) => Cap::Butt,
                Some(3) => Cap::Square,
                _ => Cap::Round,
            },
            join: match v.get("lj").and_then(Value::as_i64) {
                Some(1) => Join::Miter,
                Some(3) => Join::Bevel,
                _ => Join::Round,
            },
            class,
        },
        "tm" => {
            if v.get("m").and_then(Value::as_i64).is_some_and(|m| m != 1) {
                return Err(Error::Unsupported("トリムの「個別に」".into()));
            }
            Item::Trim {
                start: prop("s")?,
                end: prop("e")?,
                offset: prop("o")?,
            }
        }
        "tr" => Item::Transform(Transform::parse(v)?),
        other => return Err(Error::Unsupported(format!("シェイプの種類 {other}"))),
    })
}

/// 層のマスク（足し合わせのみ）。
#[derive(Clone, Debug)]
pub(crate) struct Mask {
    pub(crate) path: ShapeProp,
}

#[derive(Clone, Debug)]
pub(crate) enum LayerKind {
    Shapes(Vec<Item>),
    Precomp { id: String },
}

/// 層の上に置く型（アルファマット）の種類。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Matte {
    Alpha,
    AlphaInverted,
}

#[derive(Clone, Debug)]
pub(crate) struct Layer {
    pub(crate) kind: LayerKind,
    pub(crate) transform: Transform,
    /// この層が、すぐ下の層の型（td）か。
    pub(crate) is_matte: bool,
    /// すぐ上の層を型にして切り抜くか（tt）。
    pub(crate) matte: Option<Matte>,
    pub(crate) masks: Vec<Mask>,
    in_point: f64,
    out_point: f64,
    pub(crate) start: f64,
}

impl Layer {
    fn parse(v: &Value) -> Result<Self, Error> {
        if v.get("parent").is_some() {
            return Err(Error::Unsupported("層の親子関係".into()));
        }
        let kind = match v.get("ty").and_then(Value::as_i64) {
            Some(4) => LayerKind::Shapes(items(
                v.get("shapes")
                    .ok_or(Error::Format("シェイプレイヤーに shapes がない"))?,
            )?),
            Some(0) => LayerKind::Precomp {
                id: v
                    .get("refId")
                    .and_then(Value::as_str)
                    .ok_or(Error::Format("プリコンポに refId がない"))?
                    .to_owned(),
            },
            Some(ty) => return Err(Error::Unsupported(format!("層の種類 {ty}"))),
            None => return Err(Error::Format("層に ty がない")),
        };
        let matte = match v.get("tt").and_then(Value::as_i64) {
            None | Some(0) => None,
            Some(1) => Some(Matte::Alpha),
            Some(2) => Some(Matte::AlphaInverted),
            Some(tt) => return Err(Error::Unsupported(format!("型の種類 {tt}"))),
        };
        let masks = match v.get("masksProperties").and_then(Value::as_array) {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .map(|m| {
                    if m.get("mode").and_then(Value::as_str) != Some("a")
                        || m.get("inv").and_then(Value::as_bool) == Some(true)
                    {
                        return Err(Error::Unsupported("足し合わせ以外のマスク".into()));
                    }
                    Ok(Mask {
                        path: ShapeProp::parse(
                            m.get("pt").ok_or(Error::Format("マスクに pt がない"))?,
                        )?,
                    })
                })
                .collect::<Result<_, _>>()?,
        };
        Ok(Layer {
            kind,
            transform: Transform::parse(v.get("ks").ok_or(Error::Format("層に ks がない"))?)?,
            is_matte: v.get("td").and_then(Value::as_i64) == Some(1),
            matte,
            masks,
            in_point: v.get("ip").and_then(Value::as_f64).unwrap_or(f64::MIN),
            out_point: v.get("op").and_then(Value::as_f64).unwrap_or(f64::MAX),
            start: v.get("st").and_then(Value::as_f64).unwrap_or(0.),
        })
    }

    /// `frame` で表示されるか。
    pub(crate) fn visible(&self, frame: f64) -> bool {
        (self.in_point..self.out_point).contains(&frame)
    }
}

pub(crate) fn layers(list: &Value) -> Result<Vec<Layer>, Error> {
    list.as_array()
        .ok_or(Error::Format("層が配列でない"))?
        .iter()
        .map(Layer::parse)
        .collect()
}

pub(crate) fn assets(root: &Value) -> Result<HashMap<String, Vec<Layer>>, Error> {
    let mut map = HashMap::new();
    for asset in root
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        // 画像などの層を持たない素材は使わない
        if let (Some(id), Some(list)) =
            (asset.get("id").and_then(Value::as_str), asset.get("layers"))
        {
            map.insert(id.to_owned(), layers(list)?);
        }
    }
    Ok(map)
}
