//! tokens.json（唯一の情報源）から Constants と Theme のコードを作る。
//! 構成は Radix Colors のエイリアス(https://www.radix-ui.com/colors/docs/overview/aliasing)に倣う:
//! - scales: 色相ごとの 12 段（段ごとに用途が決まっている）。ライトとダークで別の値。これが Constants。
//! - aliases: `accent` → `blue` のようにスケールへ役割の名前を付け、各段に用途名（`accent-solid` = 9 段）を付ける。
//! - mutable: `panel` のようにテーマで指す先が変わる名前。
//!
//! 役割の組が WCAG のコントラスト比を満たさなければ、ここでビルドを落とす。

use std::collections::BTreeMap;
use std::fmt::Write;
use std::{env, fs, path::Path};

use palette::color_difference::Wcag21RelativeContrast;
use palette::{Clamp, FromColor, Oklch, Srgb};
use serde_json::Value;

#[path = "build_icons.rs"]
mod icons;

type Map = serde_json::Map<String, Value>;

const THEMES: [&str; 2] = ["light", "dark"];

fn main() {
    twemoji();
    icons::generate(Path::new(&env::var("OUT_DIR").unwrap()));
    println!("cargo:rerun-if-changed=tokens.json");
    let src = fs::read_to_string("tokens.json").expect("tokens.json を読めない");
    let root: Value = serde_json::from_str(&src).expect("tokens.json が JSON ではない");
    let scales = obj(&root, "scales");
    let statics = obj(&root, "static");
    let aliases = obj(&root, "aliases");
    let steps: Vec<&str> = root["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(steps.len(), 12, "段は 12 個");

    let mut out = String::from("pub mod scale {\n");
    for (name, v) in statics {
        let c = rgba(oklch_to_srgb(v.as_str().unwrap()));
        writeln!(out, "    pub const {}: u32 = 0x{c:08x};", konst(name)).unwrap();
    }
    for theme in THEMES {
        writeln!(out, "    pub mod {theme} {{").unwrap();
        for (name, s) in scales {
            let list = s[theme]
                .as_array()
                .unwrap_or_else(|| panic!("{name}.{theme} が無い"));
            assert_eq!(list.len(), 12, "{name}.{theme}: 12 段");
            for (i, c) in list.iter().enumerate() {
                let c = rgba(oklch_to_srgb(c.as_str().unwrap()));
                writeln!(
                    out,
                    "        pub const {}_{}: u32 = 0x{c:08x};",
                    konst(name),
                    i + 1
                )
                .unwrap();
            }
        }
        out += "    }\n";
    }
    out += "}\n";
    for (module, ty) in [
        ("radius", "f32"),
        ("spacing", "f32"),
        ("font_size", "f32"),
        ("letter_spacing", "f32"),
        ("scrollbar", "f32"),
        ("duration_ms", "u64"),
    ] {
        writeln!(out, "pub mod {module} {{").unwrap();
        for (name, v) in obj(&root, &module.replace('_', "-")) {
            let n = if ty == "f32" {
                format!("{:?}", v.as_f64().unwrap() as f32)
            } else {
                v.as_u64().unwrap().to_string()
            };
            writeln!(out, "    pub const {}: {ty} = {n};", konst(name)).unwrap();
        }
        out += "}\n";
    }

    // テーマごとに「役割名 → (定数の式, 色)」を作る。
    // 用途エイリアス（accent-solid ...）は名前が全テーマ共通で、mutable は指す先がテーマで変わる。
    let mutable = obj(&root, "mutable");
    let mut fields: Vec<String> = Vec::new();
    for alias in aliases.keys() {
        for s in &steps {
            fields.push(format!("{alias}-{s}"));
        }
    }
    fields.extend(mutable["light"].as_object().unwrap().keys().cloned());

    let mut resolved: BTreeMap<&str, BTreeMap<String, (String, Srgb)>> = BTreeMap::new();
    for theme in THEMES {
        let reference = |r: &str| -> (String, Srgb) {
            if let Some(v) = statics.get(r) {
                return (
                    format!("scale::{}", konst(r)),
                    oklch_to_srgb(v.as_str().unwrap()),
                );
            }
            let (name, n) = r
                .rsplit_once('-')
                .unwrap_or_else(|| panic!("{r}: 名前-段 で書く"));
            let n: usize = n.parse().unwrap_or_else(|_| panic!("{r}: 段が数字でない"));
            assert!((1..=12).contains(&n), "{r}: 段は 1..=12");
            let scale = aliases.get(name).map_or(name, |a| a.as_str().unwrap());
            let list = scales
                .get(scale)
                .unwrap_or_else(|| panic!("{r}: スケール {scale} が無い"))[theme]
                .as_array()
                .unwrap();
            (
                format!("scale::{theme}::{}_{n}", konst(scale)),
                oklch_to_srgb(list[n - 1].as_str().unwrap()),
            )
        };
        let m = mutable[theme]
            .as_object()
            .unwrap_or_else(|| panic!("mutable.{theme} が無い"));
        let mut map = BTreeMap::new();
        for alias in aliases.keys() {
            for (i, s) in steps.iter().enumerate() {
                map.insert(
                    format!("{alias}-{s}"),
                    reference(&format!("{alias}-{}", i + 1)),
                );
            }
        }
        for (k, v) in m {
            map.insert(k.clone(), reference(v.as_str().unwrap()));
        }
        assert!(
            fields.iter().all(|f| map.contains_key(f)),
            "{theme}: 役割が足りない"
        );
        resolved.insert(theme, map);
    }

    out += "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct Theme {\n";
    for f in &fields {
        writeln!(out, "    /// 0xRRGGBBAA\n    pub {}: u32,", field(f)).unwrap();
    }
    out += "}\nimpl Theme {\n";
    let mut failures = Vec::new();
    for theme in THEMES {
        writeln!(out, "    pub const {}: Theme = Theme {{", konst(theme)).unwrap();
        for f in &fields {
            writeln!(out, "        {}: {},", field(f), resolved[theme][f].0).unwrap();
        }
        out += "    };\n";
        for rule in root["contrast"]
            .as_array()
            .unwrap()
            .iter()
            .chain(root["distinct"].as_array().unwrap())
        {
            let (fg, bg) = (rule["fg"].as_str().unwrap(), rule["bg"].as_str().unwrap());
            let min = rule["min"].as_f64().unwrap();
            let c = |r: &str| {
                resolved[theme]
                    .get(r)
                    .unwrap_or_else(|| panic!("コントラスト: {r} が無い"))
                    .1
            };
            let got = f64::from(c(fg).relative_contrast(c(bg)));
            if got < min {
                failures.push(format!(
                    "テーマ {theme}: {fg} と {bg} のコントラスト比 {got:.2} が {min} に足りない"
                ));
            }
        }
    }
    out += "}\n";
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));

    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("tokens.rs"),
        out,
    )
    .unwrap();
}

fn twemoji() {
    println!("cargo:rerun-if-changed=twemoji/index.tsv");
    println!("cargo:rerun-if-changed=twemoji/svg.bin");
    let index = fs::read_to_string("twemoji/index.tsv").expect("Twemoji index");
    let mut out = String::from("static INDEX: &[(&str, usize, usize)] = &[\n");
    let mut lines: Vec<_> = index.lines().collect();
    lines.sort_unstable();
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let offset: usize = fields[1].parse().unwrap();
        let len: usize = fields[2].parse().unwrap();
        writeln!(out, "({:?}, {offset}, {len}),", fields[0]).unwrap();
    }
    out += "];\n";
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("twemoji.rs"),
        out,
    )
    .unwrap();
}

fn obj<'a>(v: &'a Value, k: &str) -> &'a Map {
    v[k].as_object().unwrap_or_else(|| panic!("{k} が無い"))
}

fn konst(s: &str) -> String {
    s.replace('-', "_").to_uppercase()
}

fn field(s: &str) -> String {
    s.replace('-', "_")
}

fn rgba(c: Srgb) -> u32 {
    let c: Srgb<u8> = c.into_format();
    u32::from(c.red) << 24 | u32::from(c.green) << 16 | u32::from(c.blue) << 8 | 0xff
}

/// "oklch(L C H)"（L は 0..1、H は度）を sRGB にする。
/// ponytail: 色域外は各チャンネルを切るだけ。彩度を落として収める方式は色が合わなくなったら入れる。
fn oklch_to_srgb(s: &str) -> Srgb {
    let inner = s
        .trim()
        .strip_prefix("oklch(")
        .and_then(|x| x.strip_suffix(')'));
    let v: Vec<f32> = inner
        .unwrap_or_else(|| panic!("{s}: oklch(L C H) で書く"))
        .split_whitespace()
        .map(|x| x.parse().unwrap_or_else(|_| panic!("{s}: 数値でない")))
        .collect();
    assert_eq!(v.len(), 3, "{s}: oklch(L C H) で書く");
    Srgb::from_color(Oklch::new(v[0], v[1], v[2])).clamp()
}
