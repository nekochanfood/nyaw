//! 版の文字とビルド番号を埋め込む（`$OUT_DIR/version.rs`）。
//! ビルド番号 = HEAD までのコミット数。main は直線の履歴（rebase でマージ）なので、新しいビルドほど大きい。
//! 版の文字は `NYAW_VERSION`（リリースのワークフローがタグから渡す。`2026.0.1p1` など）、無ければ Cargo.toml のもの。

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    // コミットするたびに数え直す（HEAD と、それが指すブランチの先）。
    let head = git(&["rev-parse", "--symbolic-full-name", "HEAD"]);
    let watched = [
        Some("HEAD".to_owned()),
        head.filter(|r| r.starts_with("refs/")),
    ];
    for r in watched.into_iter().flatten() {
        if let Some(path) = git(&["rev-parse", "--git-path", &r]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!("cargo:rerun-if-env-changed=NYAW_VERSION");
    // git の無い所（ソースの tarball など）では 0。どのリリースも新しく見える。
    let build: u32 = git(&["rev-list", "--count", "HEAD"])
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let version = std::env::var("NYAW_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("version.rs");
    std::fs::write(
        out,
        format!("pub const VERSION: &str = {version:?};\npub const BUILD: u32 = {build};\n"),
    )
    .unwrap();
}
