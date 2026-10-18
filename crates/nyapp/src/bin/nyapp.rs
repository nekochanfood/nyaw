//! ニャプリの開発用の道具（`cargo nyapps`。`.cargo/config.toml` の別名）。CLI なので結果は標準出力に出す。
//!
//! - `cargo nyapps build`: `nyapps/` の試作を wasm にビルドし、`target/nyapps/<id>.nyapp` にまとめる
//! - `cargo nyapps install`: build して、nyaw のパッケージのフォルダ（`data_dir()/nyapps`）に入れる
//! - `cargo nyapps pack <ニャプリのフォルダ> <wasm> [出力先]`: 1 つをまとめる（フォルダに `manifest.toml`、
//!   あればアイコンの `icon-background.*` / `icon-foreground.*`（svg か png））

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nyapp::{installed, package};

const TARGET: &str = "wasm32-unknown-unknown";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("build") => build().map(|_| ()),
        Some("install") => build().and_then(|packages| {
            for p in packages {
                let i = installed::install(&p).map_err(|e| e.to_string())?;
                println!(
                    "installed {} {} → {}",
                    i.manifest.id,
                    i.manifest.version,
                    i.path.display()
                );
            }
            Ok(())
        }),
        Some("pack") if args.len() >= 3 => {
            let out = args
                .get(3)
                .map_or_else(|| PathBuf::from("."), PathBuf::from);
            pack(Path::new(&args[1]), Path::new(&args[2]), &out).map(|_| ())
        }
        _ => Err("usage: cargo nyapps build | install | pack <dir> <wasm> [out]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// リポジトリの根（このクレートの 2 つ上）。
fn root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .ancestors()
        .nth(2)
        .unwrap_or(crate_dir)
        .to_path_buf()
}

/// `nyapps/` をビルドしてまとめ、できたパッケージを返す。
fn build() -> Result<Vec<PathBuf>, String> {
    let workspace = root().join("nyapps");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", TARGET])
        .current_dir(&workspace)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!(
            "building nyapps failed (is the target installed? rustup target add {TARGET})"
        ));
    }
    let out = root().join("target").join("nyapps");
    let mut packages = Vec::new();
    for entry in std::fs::read_dir(&workspace)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let dir = entry.path();
        if !dir.join(package::MANIFEST).exists() {
            continue;
        }
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let wasm = workspace
            .join("target")
            .join(TARGET)
            .join("release")
            .join(format!("{name}.wasm"));
        packages.push(pack(&dir, &wasm, &out)?);
    }
    Ok(packages)
}

/// ニャプリのフォルダ `dir` と `wasm` を `out/<id>.nyapp` にまとめる。
fn pack(dir: &Path, wasm: &Path, out: &Path) -> Result<PathBuf, String> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let manifest = String::from_utf8(read(&dir.join(package::MANIFEST))?)
        .map_err(|_| "manifest.toml is not UTF-8".to_string())?;
    // アイコンの背景と前景（それぞれ SVG があればそれ、無ければ PNG）。
    let layer = |[svg, png]: [&str; 2]| -> Result<Option<package::Image>, String> {
        Ok(if dir.join(svg).exists() {
            Some(package::Image::Svg(read(&dir.join(svg))?))
        } else if dir.join(png).exists() {
            Some(package::Image::Png(read(&dir.join(png))?))
        } else {
            None
        })
    };
    let icon = package::Icon {
        background: layer(package::ICON_BACKGROUND)?,
        foreground: layer(package::ICON_FOREGROUND)?,
    };
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // ID が決まるまで仮の名前で書き、最後に `<id>.nyapp` にする。
    let partial = out.join("pack.partial");
    let m = package::write(&partial, &manifest, &read(wasm)?, &icon)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = out.join(format!("{}.{}", m.id, package::EXTENSION));
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    println!("packed {} {} → {}", m.id, m.version, path.display());
    Ok(path)
}
