//! Lottie の再生。[`Animation::from_json`] で読み、[`Animation::render`] で指定フレームを描き、
//! [`Animation::hit`] で押した位置にある部品の `cl` を返す。区間の名前（マーカー）は [`Animation::marker`]。
//! 対応する機能と、対応しないものの扱いは AGENT.md を参照。

mod model;
mod render;
mod scene;
mod value;

use std::collections::HashMap;
use std::fmt;

use kurbo::{Affine, Point};
pub use tiny_skia::Pixmap;

use model::Layer;

/// 読み込みの失敗。
#[derive(Debug)]
pub enum Error {
    Json(serde_json::Error),
    /// JSON の形が Lottie として読めない。
    Format(&'static str),
    /// このクレートが描けない機能を使っている。
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json(e) => write!(f, "JSON を読めない: {e}"),
            Error::Format(m) => write!(f, "Lottie の形でない: {m}"),
            Error::Unsupported(m) => write!(f, "対応していない機能: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// 名前の付いた区間（Lottie のマーカー）。
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub name: String,
    /// 始まりのフレーム。
    pub start: f64,
    /// 長さ（フレーム）。
    pub duration: f64,
}

impl Marker {
    pub fn end(&self) -> f64 {
        self.start + self.duration
    }
}

/// 読み込んだアニメーション。描画はどのスレッドからでも呼べる（[`Send`] + [`Sync`]）。
#[derive(Debug)]
pub struct Animation {
    /// 合成の大きさ。
    pub width: f64,
    pub height: f64,
    /// 1 秒あたりのフレーム数。
    pub frame_rate: f64,
    /// 最初と最後（含まない）のフレーム。
    pub in_point: f64,
    pub out_point: f64,
    layers: Vec<Layer>,
    assets: HashMap<String, Vec<Layer>>,
    markers: Vec<Marker>,
}

impl Animation {
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let root: serde_json::Value = serde_json::from_str(json).map_err(Error::Json)?;
        let num = |name: &str| {
            root.get(name)
                .and_then(serde_json::Value::as_f64)
                .ok_or(Error::Format("合成の値がない"))
        };
        let markers = root
            .get("markers")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Marker {
                    name: m.get("cm")?.as_str()?.to_owned(),
                    start: m.get("tm")?.as_f64()?,
                    duration: m.get("dr")?.as_f64()?,
                })
            })
            .collect();
        Ok(Animation {
            width: num("w")?,
            height: num("h")?,
            frame_rate: num("fr")?,
            in_point: num("ip")?,
            out_point: num("op")?,
            layers: model::layers(
                root.get("layers")
                    .ok_or(Error::Format("合成に layers がない"))?,
            )?,
            assets: model::assets(&root)?,
            markers,
        })
    }

    /// 名前 `name` の区間。
    pub fn marker(&self, name: &str) -> Option<&Marker> {
        self.markers.iter().find(|m| m.name == name)
    }

    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// `frame` を `width` × `height` ピクセルに描く（合成の全体をその大きさに合わせる）。
    /// 画素は乗算済みの RGBA。大きさが 0 なら `None`。
    pub fn render(&self, frame: f64, width: u32, height: u32) -> Option<Pixmap> {
        let mut pixmap = Pixmap::new(width, height)?;
        let base =
            Affine::scale_non_uniform(width as f64 / self.width, height as f64 / self.height);
        let ctx = render::Ctx {
            assets: &self.assets,
            width,
            height,
        };
        render::layers(&ctx, &self.layers, frame, base, &mut pixmap, None);
        Some(pixmap)
    }

    /// `frame` で、合成の座標 `(x, y)` にある、`cl` の付いた塗りか線の `cl` を返す（手前のものを優先）。
    /// 型やマスクによる切り抜きは考えない。
    pub fn hit(&self, frame: f64, x: f64, y: f64) -> Option<String> {
        render::hit(
            &self.assets,
            &self.layers,
            frame,
            Affine::IDENTITY,
            Point::new(x, y),
        )
    }
}
