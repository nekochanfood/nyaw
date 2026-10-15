//! tokens.json（唯一の情報源）から Constants と Theme のコードを作る。
//! 役割の色はパレットの名前で書かれていて、WCAG のコントラスト比を満たさない組はここでビルドを落とす。

use std::collections::BTreeMap;
use std::fmt::Write;
use std::{env, fs, path::Path};

use palette::color_difference::Wcag21RelativeContrast;
use palette::{Clamp, FromColor, Oklch, Srgb};
use serde_json::Value;

type Map = serde_json::Map<String, Value>;

fn main() {
    println!("cargo:rerun-if-changed=tokens.json");
    let src = fs::read_to_string("tokens.json").expect("tokens.json を読めない");
    let root: Value = serde_json::from_str(&src).expect("tokens.json が JSON ではない");

    let palette: BTreeMap<String, Srgb> = obj(&root, "palette")
        .iter()
        .map(|(k, v)| (k.clone(), oklch_to_srgb(v.as_str().unwrap())))
        .collect();
    let mut out = String::new();

    out += "pub mod palette {\n";
    for (name, v) in obj(&root, "palette") {
        let c: Srgb<u8> = oklch_to_srgb(v.as_str().unwrap()).into_format();
        let rgba =
            u32::from(c.red) << 24 | u32::from(c.green) << 16 | u32::from(c.blue) << 8 | 0xff;
        writeln!(out, "    pub const {}: u32 = 0x{rgba:08x};", konst(name)).unwrap();
    }
    out += "}\n";
    for (module, ty) in [
        ("radius", "f32"),
        ("spacing", "f32"),
        ("font_size", "f32"),
        ("letter_spacing", "f32"),
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

    // テーマ: 役割 → パレットの名前。どのテーマも同じ役割を全部持つ。
    let themes = obj(&root, "themes");
    let roles: Vec<&String> = themes
        .values()
        .next()
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .collect();
    out += "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct Theme {\n";
    for r in &roles {
        writeln!(out, "    /// 0xRRGGBBAA\n    pub {}: u32,", field(r)).unwrap();
    }
    out += "}\nimpl Theme {\n";
    for (tname, t) in themes {
        let t = t.as_object().unwrap();
        assert!(
            t.keys().eq(roles.iter().copied()),
            "テーマ {tname} の役割が他と違う"
        );
        writeln!(out, "    pub const {}: Theme = Theme {{", konst(tname)).unwrap();
        for r in &roles {
            let color = t[*r].as_str().unwrap();
            assert!(
                palette.contains_key(color),
                "{tname}.{r}: パレットに {color} が無い"
            );
            writeln!(out, "        {}: palette::{},", field(r), konst(color)).unwrap();
        }
        out += "    };\n";
        check_contrast(tname, t, &palette, obj_arr(&root, "contrast"));
    }
    out += "}\n";

    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("tokens.rs"),
        out,
    )
    .unwrap();
}

fn obj<'a>(v: &'a Value, k: &str) -> &'a Map {
    v[k].as_object().unwrap_or_else(|| panic!("{k} が無い"))
}

fn obj_arr<'a>(v: &'a Value, k: &str) -> &'a Vec<Value> {
    v[k].as_array().unwrap_or_else(|| panic!("{k} が無い"))
}

fn konst(s: &str) -> String {
    s.replace('-', "_").to_uppercase()
}

fn field(s: &str) -> String {
    s.replace('-', "_")
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

fn check_contrast(theme: &str, t: &Map, palette: &BTreeMap<String, Srgb>, rules: &[Value]) {
    for rule in rules {
        let (fg, bg) = (rule["fg"].as_str().unwrap(), rule["bg"].as_str().unwrap());
        let min = rule["min"].as_f64().unwrap();
        let c = |role: &str| palette[t[role].as_str().unwrap()];
        let got = f64::from(c(fg).relative_contrast(c(bg)));
        assert!(
            got >= min,
            "テーマ {theme}: {fg} と {bg} のコントラスト比 {got:.2} が {min} に足りない"
        );
    }
}
