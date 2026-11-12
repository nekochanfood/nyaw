//! Steam Audio（phonon.dll）を実行ファイルの置き場（`target/<profile>`）に写す。`cargo build` だけで HRTF で鳴り、
//! インストーラ（`installer/nyaw.iss`）もここから同梱する。SDK は版ごとに一度だけ落として
//! `%LOCALAPPDATA%\nyaw\steamaudio-<版>` に取っておく（target・worktree ごとには落とさない）。
//! 落とせなくてもビルドは止めない（実行時はパンニングに落ちる）。`NYAW_SKIP_PHONON` があれば何もしない（CI の clippy・テスト）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// `src/phonon.rs` の `VERSION` と同じメジャーで、それ以上の版。
const VERSION: &str = "4.8.1";
/// SDK（zip の `steamaudio/`）から使うものと、実行ファイルの隣での名前。
/// THIRDPARTY.md は phonon.dll に入っているライブラリの使用許諾で、配るときに添える。
const FILES: [(&str, &str); 2] = [
    ("lib/windows-x64/phonon.dll", "phonon.dll"),
    ("THIRDPARTY.md", "steamaudio-THIRDPARTY.md"),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=NYAW_SKIP_PHONON");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var_os("NYAW_SKIP_PHONON").is_some()
    {
        return;
    }
    // OUT_DIR は target/<profile>/build/audio-<hash>/out。3 つ上が実行ファイルの置き場
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bin = out.ancestors().nth(3).unwrap();
    if let Err(e) = place(bin) {
        println!(
            "cargo:warning=Steam Audio was not placed next to the executable ({e}); \
             spatial audio falls back to panning. Retry with `cargo clean -p audio`"
        );
    }
}

fn place(bin: &Path) -> Result<(), String> {
    let sdk = PathBuf::from(env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?)
        .join("nyaw")
        .join(format!("steamaudio-{VERSION}"));
    if !sdk.exists() {
        fetch(&sdk)?;
    }
    for (from, to) in FILES {
        let (from, to) = (sdk.join(from), bin.join(to));
        let len = fs::metadata(&from)
            .map_err(|e| format!("{}: {e}", from.display()))?
            .len();
        // 動いている nyaw が読み込んでいる phonon.dll は上書きできない。同じものなら写さない
        if fs::metadata(&to).map(|m| m.len()).ok() != Some(len) {
            fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    // 消されたら写し直す（写した dll の更新日時は SDK のままなので、写しただけでは走り直さない）
    println!(
        "cargo:rerun-if-changed={}",
        bin.join("phonon.dll").display()
    );
    Ok(())
}

/// SDK の zip を落とし、使うものだけを `sdk` に展開する。途中で失敗しても半端なものを残さないよう、別の場所に展開してから移す。
fn fetch(sdk: &Path) -> Result<(), String> {
    let parent = sdk.parent().unwrap();
    // 別の target のビルドが同時に落としてもぶつからないよう、プロセスごとの名前にする
    let id = std::process::id();
    let zip = parent.join(format!("steamaudio-{VERSION}.{id}.zip"));
    let staging = parent.join(format!("steamaudio-{VERSION}.{id}.partial"));
    let result = download(&zip, &staging, sdk);
    let _ = fs::remove_file(&zip);
    let _ = fs::remove_dir_all(&staging);
    // 同時に落とした別のビルドが先に置いたなら、それを使う
    result.or_else(|e| if sdk.exists() { Ok(()) } else { Err(e) })
}

fn download(zip: &Path, staging: &Path, sdk: &Path) -> Result<(), String> {
    fs::create_dir_all(staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let url = format!(
        "https://github.com/ValveSoftware/steam-audio/releases/download/v{VERSION}/steamaudio_{VERSION}.zip"
    );
    // Windows 10 以降に入っている curl.exe と tar.exe（bsdtar。zip も読める）。Command は PATH より System32 を先に見るので、
    // Git Bash の GNU tar（zip を読めない）は拾わない（testkit の runtimes と同じ）
    run(Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--connect-timeout", "30", "-o"])
        .arg(zip)
        .arg(url))?;
    run(Command::new("tar")
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(staging)
        .args(FILES.map(|(f, _)| format!("steamaudio/{f}"))))?;
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
