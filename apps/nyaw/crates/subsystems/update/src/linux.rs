//! Linux の配布形式ごとの更新の適用と、適用後の再起動（`AGENT.md` の「Linux での適用」）。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Installer, Package, err};

const INSTALL_PACKAGE: &str = include_str!("linux/install-package.sh");
/// 引数の PID のプロセスが終了するまで待ってから、残りの引数を実行する。
const RELAUNCH: &str = r#"while kill -0 "$1" 2> /dev/null; do sleep 0.2; done; shift; exec "$@""#;

/// ダウンロード済みの `path` を適用し、更新後に起動するファイルを返す。呼び出し元のプロセスは終了しない。
/// パッケージのインストールは認証とパッケージマネージャーの完了を待つため、別スレッドで呼び出す。
pub(crate) fn apply(
    package: Package,
    path: &Path,
    installer: &Installer,
) -> Result<PathBuf, String> {
    // 置き換え後の `current_exe` は削除済みのファイルを指すため、適用の前に取得する。
    let exe = std::env::current_exe().map_err(err)?;
    match package {
        Package::AppImage => {
            let target = os::running_appimage().ok_or("not running from an AppImage")?;
            replace_file(path, &target)?;
            Ok(target)
        }
        Package::Tar => {
            let dir = exe.parent().ok_or("the executable has no directory")?;
            unpack_over(path, dir)?;
            Ok(exe)
        }
        Package::Deb => install_package(path, installer, "deb").map(|()| exe),
        Package::Rpm => install_package(path, installer, "rpm").map(|()| exe),
        Package::Pacman => install_package(path, installer, "pacman").map(|()| exe),
        Package::WindowsSetup => Err("the Windows installer cannot run on Linux".into()),
    }
}

/// 呼び出し元の終了後に、`args` を指定して `exe` を起動する。
// ponytail: `--appimage-extract-and-run` で起動した AppImage は、再起動ではマウントして実行する。FUSE のない環境にも
// 対応する場合は、展開して実行していることを検出して `APPIMAGE_EXTRACT_AND_RUN` を設定する。
pub(crate) fn relaunch(exe: &Path, args: impl IntoIterator<Item = String>) -> Result<(), String> {
    relaunch_after(std::process::id(), exe, args)
}

fn relaunch_after(
    pid: u32,
    exe: &Path,
    args: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    Command::new("/bin/sh")
        .args(["-c", RELAUNCH, "nyaw-relaunch"])
        .arg(pid.to_string())
        .arg(exe)
        .args(args)
        .stdin(Stdio::null())
        // 呼び出し元のプロセスグループに送られるシグナルで終了しないよう、別のグループにする。
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot restart {}: {e}", exe.display()))
}

/// `target` を `new` の内容に置き換える。実行中のファイルは書き換えられないため、同じディレクトリに複製して名前を変える。
fn replace_file(new: &Path, target: &Path) -> Result<(), String> {
    let name = target
        .file_name()
        .ok_or_else(|| format!("{} is not a file", target.display()))?;
    let staged = target.with_file_name(format!(".{}.update", name.to_string_lossy()));
    let result = fs::copy(new, &staged)
        .and_then(|_| fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)))
        .and_then(|()| fs::rename(&staged, target));
    result.map_err(|e| {
        let _ = fs::remove_file(&staged);
        format!("cannot replace {}: {e}", target.display())
    })
}

/// tar.gz（最上位のディレクトリは 1 つ）を展開し、含まれるファイルを `dir` の同名のファイルと置き換える。
/// `dir` のその他のファイルは変更しない。
fn unpack_over(archive: &Path, dir: &Path) -> Result<(), String> {
    // 名前の変更で置き換えるため、`dir` と同じファイルシステムに展開する。
    let staging = dir.join(".nyaw-update");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir(&staging).map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let result = (|| {
        run(Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .arg("--strip-components=1"))?;
        // 展開の結果が不完全な場合に、実行ファイルのないディレクトリにしない。
        if !staging.join("nyaw").is_file() {
            return Err("the archive does not contain nyaw".into());
        }
        for entry in fs::read_dir(&staging).map_err(err)? {
            let entry = entry.map_err(err)?;
            let to = dir.join(entry.file_name());
            fs::rename(entry.path(), &to)
                .map_err(|e| format!("cannot replace {}: {e}", to.display()))?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

fn install_package(path: &Path, installer: &Installer, format: &str) -> Result<(), String> {
    run(Command::new("pkexec")
        .args(["/bin/sh", "-c", INSTALL_PACKAGE, "nyaw-update"])
        .arg(path)
        .args([&installer.name, &installer.sha256])
        .arg(format))
    .map_err(|e| format!("{e}. The package is at {}", path.display()))
}

/// `command` を実行し、失敗した場合は標準エラー出力の末尾を含むエラーを返す。
fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    // pkexec は、認証の取り消しで 126、認可できない場合（認証エージェントがない場合を含む）に 127 を返す。
    let reason = match (program.as_str(), out.status.code()) {
        ("pkexec", Some(126)) => "authentication was cancelled".to_owned(),
        ("pkexec", Some(127)) => {
            "not authorized (a polkit authentication agent is required)".into()
        }
        _ => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lines: Vec<&str> = stderr.trim().lines().collect();
            lines[lines.len().saturating_sub(3)..].join("\n")
        }
    };
    Err(format!("{program} failed ({}): {reason}", out.status))
}

#[cfg(test)]
mod tests;
