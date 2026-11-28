//! 時間で変わる値（Lottie のプロパティ）。静止値か、キーフレームの列。

use serde_json::Value;

use crate::Error;

/// 数値の組で表す値（位置なら [x, y]、角度なら [deg]）。
#[derive(Clone, Debug)]
pub(crate) enum Prop {
    Static(Vec<f64>),
    Keyed(Vec<Key>),
}

#[derive(Clone, Debug)]
pub(crate) struct Key {
    t: f64,
    s: Vec<f64>,
    hold: bool,
    /// 次のキーフレームまでのイージング（3 次ベジェの制御点 (x1, y1, x2, y2)）。
    ease: [f64; 4],
}

impl Prop {
    /// `{"a": 0|1, "k": ...}` を読む。
    pub(crate) fn parse(v: &Value) -> Result<Self, Error> {
        let k = v.get("k").ok_or(Error::Format("プロパティに k がない"))?;
        if v.get("a").and_then(Value::as_i64) == Some(1) {
            let keys = k
                .as_array()
                .ok_or(Error::Format("キーフレームが配列でない"))?;
            let keys = keys
                .iter()
                .map(|key| {
                    let t = key
                        .get("t")
                        .and_then(Value::as_f64)
                        .ok_or(Error::Format("キーフレームに t がない"))?;
                    let s = numbers(
                        key.get("s")
                            .ok_or(Error::Format("キーフレームに s がない"))?,
                    )?;
                    let first = |name: &str, axis: &str, default: f64| {
                        key.get(name)
                            .and_then(|e| e.get(axis))
                            .and_then(|a| {
                                a.as_array()
                                    .and_then(|a| a.first())
                                    .and_then(Value::as_f64)
                                    .or_else(|| a.as_f64())
                            })
                            .unwrap_or(default)
                    };
                    Ok(Key {
                        t,
                        s,
                        hold: key.get("h").and_then(Value::as_i64) == Some(1),
                        ease: [
                            first("o", "x", 0.),
                            first("o", "y", 0.),
                            first("i", "x", 1.),
                            first("i", "y", 1.),
                        ],
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            if keys.is_empty() {
                return Err(Error::Format("キーフレームが空"));
            }
            Ok(Prop::Keyed(keys))
        } else {
            Ok(Prop::Static(numbers(k)?))
        }
    }

    /// `frame` での値。
    pub(crate) fn at(&self, frame: f64) -> Vec<f64> {
        match self {
            Prop::Static(v) => v.clone(),
            Prop::Keyed(keys) => {
                let next = keys.partition_point(|k| k.t <= frame);
                if next == 0 {
                    return keys[0].s.clone();
                }
                let key = &keys[next - 1];
                let Some(to) = keys.get(next) else {
                    return key.s.clone();
                };
                if key.hold {
                    return key.s.clone();
                }
                let u = cubic_bezier(key.ease, (frame - key.t) / (to.t - key.t));
                key.s
                    .iter()
                    .zip(&to.s)
                    .map(|(a, b)| a + (b - a) * u)
                    .collect()
            }
        }
    }

    /// 1 つ目の成分の、`frame` での値。
    pub(crate) fn scalar(&self, frame: f64) -> f64 {
        self.at(frame).first().copied().unwrap_or(0.)
    }
}

/// 数か数の配列を読む。
fn numbers(v: &Value) -> Result<Vec<f64>, Error> {
    match v {
        Value::Number(n) => Ok(vec![n.as_f64().unwrap_or(0.)]),
        Value::Array(a) => a
            .iter()
            .map(|n| n.as_f64().ok_or(Error::Format("数でない値")))
            .collect(),
        _ => Err(Error::Format("数でない値")),
    }
}

/// CSS の cubic-bezier と同じイージング。進み具合 `x`（0〜1）に対する値を返す。
/// 式は animato のものを使う（イージングの式を手で書かない。animation クレートの境界のテストを参照）。
pub(crate) fn cubic_bezier([x1, y1, x2, y2]: [f64; 4], x: f64) -> f64 {
    animato::easing::cubic_bezier(x as f32, x1 as f32, y1 as f32, x2 as f32, y2 as f32) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn linear_keys_interpolate() {
        let p = Prop::parse(&json!({"a": 1, "k": [
            {"t": 0, "s": [0, 10], "o": {"x": [0], "y": [0]}, "i": {"x": [1], "y": [1]}},
            {"t": 10, "s": [10, 20]}
        ]}))
        .unwrap();
        assert_eq!(p.at(-5.), vec![0., 10.]);
        let mid = p.at(5.);
        assert!((mid[0] - 5.).abs() < 1e-6 && (mid[1] - 15.).abs() < 1e-6);
        assert_eq!(p.at(99.), vec![10., 20.]);
    }

    #[test]
    fn hold_keys_jump() {
        let p =
            Prop::parse(&json!({"a": 1, "k": [{"t": 0, "s": [0], "h": 1}, {"t": 10, "s": [1]}]}))
                .unwrap();
        assert_eq!(p.scalar(9.9), 0.);
        assert_eq!(p.scalar(10.), 1.);
    }

    #[test]
    fn easing_hits_ends_and_is_monotonic() {
        let ease = [0.33, 0., 0.67, 1.];
        assert!(cubic_bezier(ease, 0.).abs() < 1e-6);
        assert!((cubic_bezier(ease, 1.) - 1.).abs() < 1e-6);
        let mut last = 0.;
        for i in 1..=20 {
            let v = cubic_bezier(ease, i as f64 / 20.);
            assert!(v >= last);
            last = v;
        }
    }
}
