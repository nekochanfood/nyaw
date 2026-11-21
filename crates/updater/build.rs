//! バージョン文字列とビルド番号を埋め込む（`$OUT_DIR/version.rs`）。
//! ビルド番号 = HEAD までのコミット数。main はリニア履歴（rebase マージ）のため、新しいビルドほど大きい。
//! バージョン文字列は `NYAW_VERSION`（リリースワークフローがタグから渡す。`2026.0.1-beta.1` など）、未設定であれば Cargo.toml の値。

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    // コミットごとに再カウントする（HEAD と、それが指すブランチの先頭）。
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
    // git のない環境（ソースの tarball など）では 0。すべてのリリースが新しいと判定される。
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
