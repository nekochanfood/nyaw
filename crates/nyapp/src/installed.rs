//! 入っているニャプリ。パッケージを置いたフォルダそのものがインストールの状態で、
//! `data_dir()/nyapps/<id>.nyapp` を置けば入り、消せば消える。保存領域は `data_dir()/appdata/<id>/`。
//!
//! どれもファイルを読み書きするので、フレームループ（メインスレッド）からは呼ばず、別スレッドで呼ぶ。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::RenderImage;

use kernel::subsystems::os;

use crate::{Error, Manifest, package, system};

/// 入っているニャプリ 1 つ。
#[derive(Clone)]
pub struct Installed {
    pub manifest: Manifest,
    pub path: PathBuf,
    /// パッケージのアイコン（無い・読めなければ `None`。描くときは [`crate::icon::element`] が代わりを描く）。
    pub icon: Option<Arc<RenderImage>>,
}

impl Installed {
    /// 一辺 `size` px のアイコン（無ければ代わりのもの）。名前は `lang` のもの。
    pub fn icon(&self, lang: &str, size: f32, theme: &design::Theme) -> gpui::AnyElement {
        crate::icon::element(self.icon.clone(), self.manifest.name_in(lang), size, theme)
    }
}

/// パッケージを置くフォルダ。
pub fn dir() -> Option<PathBuf> {
    os::data_dir().map(|d| d.join("nyapps"))
}

/// ニャプリ `id` の保存領域。
pub fn appdata(id: &str) -> Option<PathBuf> {
    os::data_dir().map(|d| d.join("appdata").join(id))
}

/// 入っているものを名前の順に。読めないもの・ファイル名と ID が違うもの・システムの ID を名乗るものは、
/// ログに残して外す。
pub fn scan() -> Vec<Installed> {
    dir().map(|d| scan_dir(&d)).unwrap_or_default()
}

fn scan_dir(dir: &Path) -> Vec<Installed> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == package::EXTENSION))
        .filter_map(|path| match check(&path) {
            Ok((manifest, icon)) => Some(Installed {
                manifest,
                icon: icon.as_ref().and_then(crate::icon::decode),
                path,
            }),
            Err(e) => {
                logger::warn!("skipping nyapp package", path = path.display(), error = e);
                None
            }
        })
        .collect();
    found.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    found
}

/// 入っているパッケージとして正しいか（マニフェストが読め、ファイル名が `<id>.nyapp` で、システムの ID でない）。
/// 正しければマニフェストとアイコンを返す。
fn check(path: &Path) -> Result<(Manifest, Option<package::Icon>), Error> {
    let (manifest, icon) = package::read_listing(path)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if stem != manifest.id {
        return Err(Error::new(format!(
            "file name must be {}.{}",
            manifest.id,
            package::EXTENSION
        )));
    }
    if system::is_system(&manifest.id) {
        return Err(Error::new(format!("{} is a system nyapp", manifest.id)));
    }
    Ok((manifest, icon))
}

/// パッケージ `src` を入れる（同じ ID があれば置き換える = 更新）。システムの ID は入れない。
pub fn install(src: &Path) -> Result<Installed, Error> {
    let dir = dir().ok_or_else(|| Error::new("no data directory"))?;
    install_into(&dir, src)
}

pub(crate) fn install_into(dir: &Path, src: &Path) -> Result<Installed, Error> {
    let (manifest, icon) = package::read_listing(src)?;
    if system::is_system(&manifest.id) {
        return Err(Error::new(format!("{} is a system nyapp", manifest.id)));
    }
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.{}", manifest.id, package::EXTENSION));
    // 書きかけのパッケージを読まないよう、隣に写してから入れ替える。
    let partial = path.with_extension("partial");
    fs::copy(src, &partial)?;
    fs::rename(&partial, &path)?;
    Ok(Installed {
        manifest,
        path,
        icon: icon.as_ref().and_then(crate::icon::decode),
    })
}

/// ニャプリ `id` を消す（パッケージと保存領域）。システムのニャプリは消せない。
pub fn uninstall(id: &str) -> Result<(), Error> {
    if system::is_system(id) || !crate::manifest::valid_id(id) {
        return Err(Error::new(format!("{id} cannot be uninstalled")));
    }
    if let Some(dir) = dir() {
        let path = dir.join(format!("{id}.{}", package::EXTENSION));
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    if let Some(data) = appdata(id).filter(|d| d.exists()) {
        fs::remove_dir_all(data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(dir: &Path, file: &str, id: &str) -> PathBuf {
        let path = dir.join(file);
        let manifest = format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\napi = 1\n");
        package::write(&path, &manifest, b"", None).unwrap();
        path
    }

    /// 入れると `<id>.nyapp` になって一覧に出る。ファイル名と ID が違うもの・システムの ID は一覧にも入れない。
    #[test]
    fn installs_and_lists_only_well_named_user_packages() {
        let root = std::env::temp_dir().join(format!("nyapp-installed-{}", std::process::id()));
        let (src, dir) = (root.join("src"), root.join("nyapps"));
        fs::create_dir_all(&src).unwrap();
        let downloaded = package(&src, "download (1).nyapp", "net.bettaworx.clock");
        let installed = install_into(&dir, &downloaded).unwrap();
        assert_eq!(installed.path, dir.join("net.bettaworx.clock.nyapp"));
        package(&dir, "wrong-name.nyapp", "net.bettaworx.other");
        package(&dir, "net.bettaworx.wrist.nyapp", "net.bettaworx.wrist");
        let system = package(&src, "x.nyapp", "net.bettaworx.dashboard");
        assert!(
            install_into(&dir, &system).is_err(),
            "システムのニャプリは上書きできない"
        );
        let ids: Vec<_> = scan_dir(&dir).into_iter().map(|i| i.manifest.id).collect();
        assert_eq!(ids, ["net.bettaworx.clock"]);
        fs::remove_dir_all(root).unwrap();
    }
}
