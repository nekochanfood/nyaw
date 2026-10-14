//! web/ を Vite でビルドし、出力を `OUT_DIR/web` に置いて、全ファイルを埋め込む `assets.rs` を生成する。
//! node_modules が無ければ先に `npm ci` する。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // canonicalize はしない（Windows では \\?\ 付きのパスになり、cmd.exe が作業ディレクトリにできない）。
    let web = manifest.join("..").join("..").join("web");
    assert!(web.join("package.json").exists(), "web/ がある");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dist = out.join("web");

    // ソースが変わったときだけビルドし直す（ディレクトリは中身の更新時刻で見られる）。無いパスを書くと毎回走るので書かない。
    // ページを足したら、その入口のディレクトリもここに足す。
    for p in [
        "src",
        "testing",
        "testing-transparent",
        "package.json",
        "package-lock.json",
        "vite.config.ts",
        "tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={}", web.join(p).display());
    }
    println!("cargo:rerun-if-env-changed=NYAW_WEB_SKIP");

    let files = if env::var_os("NYAW_WEB_SKIP").is_some() {
        println!("cargo:warning=NYAW_WEB_SKIP: Web のページをビルドしない（ページは 404 になる）");
        Vec::new()
    } else {
        if !web.join("node_modules").exists() {
            npm(&web, &["ci"]);
        }
        npm_build(&web, &dist);
        let mut files = Vec::new();
        walk(&dist, &dist, &mut files);
        files.sort();
        files
    };

    let mut src = String::from(
        "/// ビルドしたページの全ファイル（`<ページ名>/index.html` と共有の `assets/`）。\n",
    );
    src.push_str("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in files {
        src.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    src.push_str("];\n");
    fs::write(out.join("assets.rs"), src).unwrap();
}

/// Windows の npm はバッチファイル。
fn npm_command() -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "npm"]);
        c
    } else {
        Command::new("npm")
    }
}

fn npm(web: &Path, args: &[&str]) {
    let status = npm_command().args(args).current_dir(web).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("npm {args:?} が失敗した（{s}）"),
        Err(e) => panic!(
            "npm を起動できない（{e}）。Node.js を入れるか、NYAW_WEB_SKIP=1 でページのビルドを飛ばす"
        ),
    }
}

fn npm_build(web: &Path, dist: &Path) {
    let status = npm_command()
        .args(["run", "build", "--silent"])
        .env("NYAW_WEB_OUT", dist)
        .current_dir(web)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("web/ のビルドが失敗した（{s}）"),
        Err(e) => panic!(
            "npm を起動できない（{e}）。Node.js を入れるか、NYAW_WEB_SKIP=1 でページのビルドを飛ばす"
        ),
    }
}

/// `dir` 以下のファイルを (`root` からの相対パス（/ 区切り）, 絶対パス) で集める。
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).unwrap();
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path.to_string_lossy().into_owned()));
        }
    }
}
