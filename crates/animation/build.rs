//! 動きの長さは design の tokens.json（唯一の情報源）の `duration-ms` から生成する。

use std::{env, fmt::Write, fs, path::Path};

fn main() {
    // env! ではビルドスクリプトのコンパイル時のパスが焼き込まれ、target を共有する別の worktree の残存パスを参照してしまう
    let tokens = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../design/tokens.json");
    println!("cargo:rerun-if-changed={}", tokens.display());
    let root: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tokens).expect("tokens.json")).unwrap();
    let mut out = String::from("pub mod duration_ms {\n");
    for (name, v) in root["duration-ms"]
        .as_object()
        .expect("duration-ms が存在しない")
    {
        let name = name.replace('-', "_").to_uppercase();
        writeln!(out, "    pub const {name}: u64 = {};", v.as_u64().unwrap()).unwrap();
    }
    out += "}\n";
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("duration_ms.rs"),
        out,
    )
    .unwrap();
}
