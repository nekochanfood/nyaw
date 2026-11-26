//! Steam Audio（Windows は phonon.dll、Linux は libphonon.so）を実行ファイルの配置先（`target/<profile>`）にコピーする。
//! `cargo build` のみで HRTF で再生可能になり、インストーラ（`installer/nyaw.iss`）もここから同梱する。SDK は版ごとに一度だけ
//! ダウンロードして `%LOCALAPPDATA%\nyaw\steamaudio-<版>`（Linux は `$XDG_DATA_HOME/nyaw/steamaudio-<版>`）に保持する
//! （target・worktree ごとにはダウンロードしない）。
//! ダウンロード不可でもビルドは停止しない（実行時はパンニングにフォールバックする）。`NYAW_SKIP_PHONON` があれば何も行わない（CI の clippy・テスト）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// `src/phonon.rs` の `VERSION` と同じメジャーで、それ以上の版。
const VERSION: &str = "4.8.1";
/// SDK（zip の `steamaudio/`）から使うものと、実行ファイルの隣での名前（ビルド対象の OS ごと）。
/// THIRDPARTY.md は Steam Audio のライブラリに含まれるライブラリの使用許諾で、配布時に添付する。
fn files(os: &str) -> Option<[(&'static str, &'static str); 2]> {
    let lib = match os {
        "windows" => ("lib/windows-x64/phonon.dll", "phonon.dll"),
        "linux" => ("lib/linux-x64/libphonon.so", "libphonon.so"),
        _ => return None,
    };
    Some([lib, ("THIRDPARTY.md", "steamaudio-THIRDPARTY.md")])
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=NYAW_SKIP_PHONON");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let Some(files) = files(&os) else {
        return;
    };
    if env::var_os("NYAW_SKIP_PHONON").is_some() {
        return;
    }
    // OUT_DIR は target/<profile>/build/audio-<hash>/out（Rust 1.100 からは build/audio/<hash>/out）。build の親が実行ファイルの置き場
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bin = out
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "build"))
        .and_then(Path::parent)
        .expect("OUT_DIR is under target/<profile>/build");
    if let Err(e) = place(bin, &os, files) {
        println!(
            "cargo:warning=Steam Audio was not placed next to the executable ({e}); \
             spatial audio falls back to panning. Retry with `cargo clean -p audio`"
        );
    }
}

fn place(bin: &Path, os: &str, files: [(&str, &str); 2]) -> Result<(), String> {
    let sdk = data_dir(os)?
        .join("nyaw")
        .join(format!("steamaudio-{VERSION}"));
    if !sdk.exists() {
        fetch(&sdk, files)?;
    }
    for (from, to) in files {
        let (from, to) = (sdk.join(from), bin.join(to));
        let len = fs::metadata(&from)
            .map_err(|e| format!("{}: {e}", from.display()))?
            .len();
        // 実行中の nyaw が読み込んでいる phonon.dll は上書き不可。同一であればコピーしない
        if fs::metadata(&to).map(|m| m.len()).ok() != Some(len) {
            fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    // 削除されたら再コピーする（コピーしたライブラリの更新日時は SDK のままのため、コピーしただけでは再実行されない）
    println!("cargo:rerun-if-changed={}", bin.join(files[0].1).display());
    Ok(())
}

/// SDK を保持するディレクトリの親（`os::data_dir` の親と同じ場所）。ビルドスクリプトは os クレートを使わないため、ここで求める。
fn data_dir(os: &str) -> Result<PathBuf, String> {
    let var = |key: &str| {
        env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if os == "windows" {
        return var("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA is not set".into());
    }
    var("XDG_DATA_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| Some(var("HOME")?.join(".local").join("share")))
        .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".into())
}

/// SDK の zip をダウンロードし、必要なもののみを `sdk` に展開する。途中で失敗しても不完全なものを残留させないよう、別の場所に展開してから移動する。
fn fetch(sdk: &Path, files: [(&str, &str); 2]) -> Result<(), String> {
    let parent = sdk.parent().unwrap();
    // 別の target のビルドが同時にダウンロードしても競合しないよう、プロセスごとの名前にする
    let id = std::process::id();
    let zip = parent.join(format!("steamaudio-{VERSION}.{id}.zip"));
    let staging = parent.join(format!("steamaudio-{VERSION}.{id}.partial"));
    let result = download(&zip, &staging, sdk, files);
    let _ = fs::remove_file(&zip);
    let _ = fs::remove_dir_all(&staging);
    // 同時にダウンロードした別のビルドが先に配置済みであれば、それを使用する
    result.or_else(|e| if sdk.exists() { Ok(()) } else { Err(e) })
}

fn download(
    zip: &Path,
    staging: &Path,
    sdk: &Path,
    files: [(&str, &str); 2],
) -> Result<(), String> {
    fs::create_dir_all(staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let url = format!(
        "https://github.com/ValveSoftware/steam-audio/releases/download/v{VERSION}/steamaudio_{VERSION}.zip"
    );
    // Windows 10 以降に搭載されている curl.exe と tar.exe（bsdtar。zip も読み取り可能）。Command は PATH より System32 を先に参照するため、
    // Git Bash の GNU tar（zip を読み取れない）は参照しない（testkit の runtimes と同じ）
    run(Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--connect-timeout", "30", "-o"])
        .arg(zip)
        .arg(url))?;
    let entries = files.map(|(f, _)| format!("steamaudio/{f}"));
    if cfg!(windows) {
        run(Command::new("tar")
            .arg("-xf")
            .arg(zip)
            .arg("-C")
            .arg(staging)
            .args(entries))?;
    } else {
        // GNU tar は zip を読み取れないため、unzip で展開する。
        run(Command::new("unzip")
            .arg("-q")
            .arg(zip)
            .args(entries)
            .arg("-d")
            .arg(staging))?;
    }
    fs::rename(staging.join("steamaudio"), sdk).map_err(|e| format!("{}: {e}", sdk.display()))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    out.status.success().then_some(()).ok_or_else(|| {
        format!(
            "{cmd:?}: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )
    })
}
