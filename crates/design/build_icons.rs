//! Phosphor アイコンのコード生成。ワークスペースのソースで実際に使われているアイコンとウェイトだけを埋め込む。
//! - アイコン: `icons::HEART` / `icons::{HEART, BELL}` と書かれた定数名（UPPER_SNAKE）。Phosphor に無い名前はビルドを落とす。
//! - ウェイト: `Weight::Bold` など。`Weight::Regular` は常に入る。`Weight::ALL` と書けば全部入る。
//!
//! データは icons/<weight>.tsv（vendor.mjs が @phosphor-icons/core から作る）。
//! path の d は svgtypes で絶対座標の M / L / Q / C / Z にそろえる（相対・省略形・円弧は展開）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::{fs, path::Path};

use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};

const WEIGHTS: [&str; 6] = ["thin", "light", "regular", "bold", "fill", "duotone"];

pub fn generate(out_dir: &Path) {
    println!("cargo:rerun-if-changed=icons");
    let root = Path::new("../..");
    let mut files = Vec::new();
    for dir in ["crates", "src"] {
        println!("cargo:rerun-if-changed=../../{dir}");
        collect(&root.join(dir), &mut files);
    }
    let (mut names, mut weights) = (BTreeSet::new(), BTreeSet::from(["regular".to_string()]));
    for f in files {
        let text = fs::read_to_string(&f).unwrap_or_default();
        scan_names(&text, &mut names);
        scan_weights(&text, &mut weights);
    }

    // weight -> name -> d のリスト
    let mut data: BTreeMap<&str, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for w in weights
        .iter()
        .map(|w| *WEIGHTS.iter().find(|x| *x == w).unwrap())
    {
        let tsv = fs::read_to_string(format!("icons/{w}.tsv")).unwrap();
        let map = tsv
            .lines()
            .map(|l| {
                let mut it = l.split('\t');
                (
                    it.next().unwrap().to_string(),
                    it.map(String::from).collect(),
                )
            })
            .collect();
        data.insert(w, map);
    }

    let mut out = String::from("pub mod icons {\n    use crate::icon::{Icon, Layer, Seg::*};\n");
    let mut all = Vec::new();
    for name in &names {
        let kebab = name.to_lowercase().replace('_', "-");
        writeln!(
            out,
            "    pub static {name}: Icon = Icon {{ name: {kebab:?},"
        )
        .unwrap();
        for w in WEIGHTS {
            write!(out, "        {w}: &[").unwrap();
            if let Some(map) = data.get(w) {
                let ds = map.get(&kebab).unwrap_or_else(|| {
                    panic!("Phosphor に `{kebab}`（icons::{name}）が無い。綴りを確認する")
                });
                for d in ds {
                    out += &layer(d);
                }
            }
            out += "],\n";
        }
        out += "    };\n";
        all.push(name.as_str());
    }
    let list: Vec<String> = all.iter().map(|n| format!("&{n}")).collect();
    writeln!(out, "    /// 実際に使われていて、ビルドに含まれるアイコン。\n    pub static ALL: &[&Icon] = &[{}];", list.join(", ")).unwrap();
    let used: Vec<String> = weights.iter().map(|w| format!("{w:?}")).collect();
    writeln!(
        out,
        "    /// ビルドに含まれるウェイト（小文字）。\n    pub const WEIGHTS: &[&str] = &[{}];",
        used.join(", ")
    )
    .unwrap();
    out += "}\n";
    fs::write(out_dir.join("icons.rs"), out).unwrap();
}

fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if name != "target" && name != "node_modules" && !name.starts_with('.') {
                collect(&p, files);
            }
        } else if name.ends_with(".rs")
            // 生成側と、ウェイトの定義そのもの（全ウェイトの名前が出てくる）は数えない
            && !matches!(&*name, "build.rs" | "build_icons.rs" | "icon.rs")
        {
            files.push(p);
        }
    }
}

fn is_const(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

fn scan_names(text: &str, names: &mut BTreeSet<String>) {
    for (i, _) in text.match_indices("icons::") {
        // `xicons::` のような別の識別子の末尾は除く
        if text[..i]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let rest = &text[i + 7..];
        let span = match rest.strip_prefix('{') {
            Some(b) => &b[..b.find('}').unwrap_or(0)],
            None => rest,
        };
        let mut cur = String::new();
        for c in span.chars().chain([' ']) {
            if is_const(c) {
                cur.push(c);
            } else {
                // 先頭が大文字で、`ALL` / `WEIGHTS` 以外が定数名
                if cur.starts_with(|c: char| c.is_ascii_uppercase())
                    && cur != "ALL"
                    && cur != "WEIGHTS"
                {
                    names.insert(std::mem::take(&mut cur));
                }
                cur.clear();
                if !rest.starts_with('{') {
                    break;
                }
            }
        }
    }
}

fn scan_weights(text: &str, weights: &mut BTreeSet<String>) {
    for (i, _) in text.match_indices("Weight::") {
        let word: String = text[i + 8..]
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if word == "ALL" {
            weights.extend(WEIGHTS.map(String::from));
        } else if WEIGHTS.contains(&word.to_lowercase().as_str())
            && word.starts_with(|c: char| c.is_ascii_uppercase())
        {
            weights.insert(word.to_lowercase());
        }
    }
}

/// `~` 始まりは duotone の薄い面。
fn layer(d: &str) -> String {
    let (opacity, d) = match d.strip_prefix('~') {
        Some(d) => ("0.2", d),
        None => ("1.0", d),
    };
    let n = |v: f64| format!("{:?}", ((v * 1000.0).round() / 1000.0) as f32);
    let segs: Vec<String> = SimplifyingPathParser::from(d)
        .map(|s| match s.unwrap_or_else(|e| panic!("{d}: {e}")) {
            S::MoveTo { x, y } => format!("M({},{})", n(x), n(y)),
            S::LineTo { x, y } => format!("L({},{})", n(x), n(y)),
            S::Quadratic { x1, y1, x, y } => format!("Q({},{},{},{})", n(x1), n(y1), n(x), n(y)),
            S::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                format!(
                    "C({},{},{},{},{},{})",
                    n(x1),
                    n(y1),
                    n(x2),
                    n(y2),
                    n(x),
                    n(y)
                )
            }
            S::ClosePath => "Z".into(),
        })
        .collect();
    format!(
        "Layer {{ opacity: {opacity}, segs: &[{}] }},",
        segs.join(",")
    )
}
