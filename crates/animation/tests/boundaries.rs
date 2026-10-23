//! 動きの責務の境界。破るとここで落ちる。
//! - `animation` は GPUI にも design にも依存しない（下位層の overlay からも使えるように）。
//! - イージングの式は `animation` にだけ書く。部品は `Motion::ease` / `Transition` / `Animation` を使う。

use std::{fs, path::Path};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn animation_depends_on_neither_gpui_nor_design() {
    let manifest = fs::read_to_string(Path::new(ROOT).join("Cargo.toml")).unwrap();
    let manifest: String = manifest
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    for banned in ["gpui", "design"] {
        assert!(
            !manifest.contains(banned),
            "animation は {banned} に依存しない"
        );
    }
}

#[test]
fn easing_math_lives_only_in_animation() {
    let crates = Path::new(ROOT).join("..");
    let mut found = Vec::new();
    for entry in fs::read_dir(&crates).unwrap().flatten() {
        if entry.file_name() != "animation" {
            visit(&entry.path().join("src"), &mut found);
        }
    }
    assert!(
        found.is_empty(),
        "手書きのイージング（`powi(3)`）は animation::Motion::ease に寄せる: {found:?}"
    );
}

fn visit(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            visit(&path, found);
        } else if path.extension().is_some_and(|x| x == "rs")
            && fs::read_to_string(&path).is_ok_and(|s| s.contains("powi(3)"))
        {
            found.push(path.display().to_string());
        }
    }
}

/// design の部品は動きの時刻を `design::drive::now()` からだけ読む（テストは `drive::advance` で進める）。
#[test]
fn design_reads_the_clock_only_through_drive() {
    let src = Path::new(ROOT).join("../design/src");
    let mut found = Vec::new();
    scan(&src, &mut found);
    assert!(
        found.is_empty(),
        "Instant::now() ではなく design::drive::now() を使う: {found:?}"
    );

    fn scan(dir: &Path, found: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                scan(&path, found);
            } else if path.extension().is_some_and(|x| x == "rs")
                && path.file_name() != Some("drive.rs".as_ref())
            {
                let s = fs::read_to_string(&path).unwrap();
                // テストのモジュールより前（本体）だけ見る。
                let body = s.split("#[cfg(test)]").next().unwrap();
                if body.contains("Instant::now()") {
                    found.push(path.display().to_string());
                }
            }
        }
    }
}
