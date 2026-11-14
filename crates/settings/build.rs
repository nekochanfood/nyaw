//! 設定の「ライセンス」に出す依存の一覧を作る。`cargo metadata` で nyaw（バイナリ）から通常・ビルドの依存だけを
//! たどり（開発用の依存とこのワークスペースのクレートは除く）、`(名前, バージョン, ライセンス, URL)` の表を書き出す。

use std::{
    collections::{BTreeSet, HashMap},
    env, fs,
    path::Path,
    process::Command,
};

use serde_json::Value;

fn main() {
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    let out = Command::new(env::var("CARGO").unwrap())
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .args(["--filter-platform", &env::var("TARGET").unwrap()])
        .current_dir("../..")
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages: HashMap<&str, &Value> = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p))
        .collect();
    let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["id"].as_str().unwrap(), n))
        .collect();

    // nyaw から、開発用でない依存をたどる。
    let root = meta["resolve"]["root"].as_str().expect("root package");
    let (mut seen, mut todo) = (BTreeSet::from([root]), vec![root]);
    while let Some(id) = todo.pop() {
        for dep in nodes[id]["deps"].as_array().unwrap() {
            let used = dep["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k["kind"].as_str() != Some("dev"));
            let pkg = dep["pkg"].as_str().unwrap();
            if used && seen.insert(pkg) {
                todo.push(pkg);
            }
        }
    }

    let rows: BTreeSet<(String, String, String, String)> = seen
        .into_iter()
        .filter(|id| !members.contains(id))
        .map(|id| {
            let p = packages[id];
            let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            let (name, version) = (s("name"), s("version"));
            let url = [s("repository"), s("homepage")]
                .into_iter()
                .find(|u| !u.is_empty())
                .unwrap_or_else(|| format!("https://crates.io/crates/{name}/{version}"));
            (name, version, s("license"), url)
        })
        .collect();

    let mut code = String::from("pub const LICENSES: &[(&str, &str, &str, &str)] = &[\n");
    for row in rows {
        code += &format!("    {row:?},\n");
    }
    code += "];\n";
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("licenses.rs");
    fs::write(path, code).unwrap();
}
