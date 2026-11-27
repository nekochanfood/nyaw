//! Linux のアプリ情報の解決。`.desktop` ファイル（Desktop Entry Specification）と、アイコンテーマの PNG を読む。

use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use crate::{Icon, InstalledApp};

/// アイコンの一辺の上限（px）。
const ICON_PX: u32 = 64;
/// アイコンテーマで探す大きさ。[`ICON_PX`] 以上を小さい順に、続けて未満を大きい順に調べる。
const SIZES: [u32; 11] = [64, 96, 128, 192, 256, 512, 48, 32, 24, 22, 16];
/// 読み込むファイルの大きさの上限（バイト）。
const MAX_BYTES: u64 = 4 << 20;

pub(crate) fn installed_app(id: &str) -> Option<InstalledApp> {
    find(&os::data_search_dirs(), os::ui_language(), id)
}

/// `.desktop` ファイルの `[Desktop Entry]` のうち、使用する項目。
#[derive(Debug, Default, PartialEq, Eq)]
struct Entry {
    name: String,
    /// 表示言語の名前（`Name[ja]` など）。
    localized: Option<String>,
    icon: Option<String>,
    wm_class: Option<String>,
    no_display: bool,
    hidden: bool,
}

fn parse(text: &str, language: Option<&str>) -> Entry {
    let mut entry = Entry::default();
    let mut inside = false;
    let localized_key = language.map(|l| format!("Name[{l}]"));
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| inside) else {
            continue;
        };
        let (key, value) = (key.trim_end(), value.trim_start().to_owned());
        match key {
            "Name" => entry.name = value,
            "Icon" => entry.icon = Some(value).filter(|v| !v.is_empty()),
            "StartupWMClass" => entry.wm_class = Some(value),
            "NoDisplay" => entry.no_display = value == "true",
            "Hidden" => entry.hidden = value == "true",
            key if Some(key) == localized_key.as_deref() => entry.localized = Some(value),
            _ => {}
        }
    }
    entry
}

fn find(dirs: &[PathBuf], language: Option<&str>, id: &str) -> Option<InstalledApp> {
    // ID は他のプロセス（メディアプレイヤー）が申告する値であるため、パスとして解釈される文字を受け付けない。
    if id.is_empty() || id.contains('/') {
        return None;
    }
    let (stem, entry) = by_file_name(dirs, language, id).or_else(|| search(dirs, language, id))?;
    let icon = entry.icon.as_deref().and_then(|i| icon(dirs, i));
    Some(InstalledApp {
        name: entry.localized.unwrap_or(entry.name),
        id: stem,
        // 起動中のウィンドウとの照合に使う値。Linux では個別のウィンドウを扱わないため、設定しない。
        exe: None,
        icon,
    })
}

/// `<id>.desktop`。優先度の高いディレクトリのファイルが、後のディレクトリの同名のファイルを隠す。
fn by_file_name(dirs: &[PathBuf], language: Option<&str>, id: &str) -> Option<(String, Entry)> {
    let text = dirs
        .iter()
        .find_map(|d| read(&d.join("applications").join(format!("{id}.desktop"))))?;
    let entry = parse(&String::from_utf8_lossy(&text), language);
    (!entry.hidden && !entry.name.is_empty()).then(|| (id.to_owned(), entry))
}

/// ファイル名が一致しないアプリを、大文字と小文字を区別せずに、ファイル名、`StartupWMClass`、名前の順で照合する。
/// 同じ順位では、メニューに表示するエントリー、優先度の高いディレクトリ、ファイル名の順で先のものを選ぶ。
// ponytail: applications/ のサブディレクトリは調べない。必要になれば再帰的に走査する。
fn search(dirs: &[PathBuf], language: Option<&str>, id: &str) -> Option<(String, Entry)> {
    let wanted = id.to_lowercase();
    let same = |s: &str| s.to_lowercase() == wanted;
    let mut seen = HashSet::new();
    let mut best: Option<((u8, bool), String, Entry)> = None;
    for dir in dirs {
        let Ok(list) = fs::read_dir(dir.join("applications")) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = list.filter_map(|e| Some(e.ok()?.path())).collect();
        paths.sort();
        for path in paths {
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !seen.insert(stem.to_owned()) {
                continue;
            }
            let Some(text) = read(&path) else {
                continue;
            };
            let entry = parse(&String::from_utf8_lossy(&text), language);
            if entry.hidden || entry.name.is_empty() {
                continue;
            }
            let rank = if same(stem) {
                0
            } else if entry.wm_class.as_deref().is_some_and(same) {
                1
            } else if same(&entry.name) {
                2
            } else {
                continue;
            };
            let score = (rank, entry.no_display);
            if best.as_ref().is_none_or(|(b, _, _)| score < *b) {
                best = Some((score, stem.to_owned(), entry));
            }
        }
    }
    best.map(|(_, stem, entry)| (stem, entry))
}

/// `Icon` の値（絶対パス、またはアイコン名）が指す画像。
// ponytail: 調べるのは hicolor テーマと pixmaps の PNG だけで、利用者が選んだテーマと SVG は読まない。必要になれば
// index.theme の解釈と SVG の描画を追加する。
fn icon(dirs: &[PathBuf], value: &str) -> Option<Icon> {
    if value.starts_with('/') {
        return decode(Path::new(value));
    }
    if value.contains('/') {
        return None;
    }
    let file = format!("{value}.png");
    let themed = SIZES.iter().flat_map(|size| {
        let file = &file;
        dirs.iter().map(move |d| {
            d.join(format!("icons/hicolor/{size}x{size}/apps"))
                .join(file)
        })
    });
    themed
        .chain(dirs.iter().map(|d| d.join("pixmaps").join(&file)))
        .find_map(|path| decode(&path))
}

fn decode(path: &Path) -> Option<Icon> {
    let mut image = image::load_from_memory(&read(path)?).ok()?;
    if image.width() > ICON_PX || image.height() > ICON_PX {
        image = image.thumbnail(ICON_PX, ICON_PX);
    }
    let rgba = image.into_rgba8();
    Some(Icon {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw().into(),
    })
}

/// 通常のファイルで、大きさが [`MAX_BYTES`] 以下のときだけ読む（FIFO やデバイスは、開くだけで待機し得る）。
fn read(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 利用者とシステムの 2 つの検索先を持つ一時ディレクトリ。ドロップすると削除する。
    struct Data {
        root: PathBuf,
        dirs: Vec<PathBuf>,
    }

    impl Data {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("nyaw-apps-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let dirs = vec![root.join("user"), root.join("system")];
            for dir in &dirs {
                fs::create_dir_all(dir.join("applications")).unwrap();
            }
            Self { root, dirs }
        }

        fn entry(&self, dir: usize, stem: &str, body: &str) {
            let path = self.dirs[dir].join(format!("applications/{stem}.desktop"));
            fs::write(path, format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap();
        }

        fn png(&self, path: &Path, side: u32) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            image::RgbaImage::from_pixel(side, side, image::Rgba([1, 2, 3, 255]))
                .save(path)
                .unwrap();
        }

        fn find(&self, id: &str) -> Option<InstalledApp> {
            find(&self.dirs, Some("ja"), id)
        }
    }

    impl Drop for Data {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn only_the_main_group_is_read() {
        let entry = parse(
            "# comment\n[Desktop Entry]\nName = Player\nName[ja]=プレイヤー\nName[de]=Spieler\n\
             Icon=player\nStartupWMClass=PlayerWindow\nNoDisplay=true\n\
             [Desktop Action New]\nName=New Window\nIcon=other\n",
            Some("ja"),
        );
        assert_eq!(
            entry,
            Entry {
                name: "Player".into(),
                localized: Some("プレイヤー".into()),
                icon: Some("player".into()),
                wm_class: Some("PlayerWindow".into()),
                no_display: true,
                hidden: false,
            }
        );
    }

    #[test]
    fn the_users_file_hides_the_system_file_of_the_same_name() {
        let data = Data::new("shadow");
        data.entry(1, "player", "Name=System");
        assert_eq!(data.find("player").unwrap().name, "System");
        data.entry(0, "player", "Name=User\nName[ja]=利用者");
        assert_eq!(data.find("player").unwrap().name, "利用者");
        // Hidden は削除されたアプリを表し、システムのファイルへは戻らない。
        data.entry(0, "player", "Name=User\nHidden=true");
        assert_eq!(data.find("player"), None);
    }

    /// Spotify のように、プレイヤーが申告する名前と `.desktop` のファイル名が異なるアプリがある。
    #[test]
    fn apps_are_matched_by_window_class_and_name_when_the_file_name_differs() {
        let data = Data::new("match");
        data.entry(1, "a-by-name", "Name=Spotify");
        data.entry(
            1,
            "spotify-launcher",
            "Name=Spotify (Launcher)\nStartupWMClass=Spotify",
        );
        let app = data.find("spotify").unwrap();
        assert_eq!(
            app.id, "spotify-launcher",
            "StartupWMClass は名前より優先する"
        );
        assert_eq!(app.name, "Spotify (Launcher)");

        data.entry(1, "chromium-based", "Name=Helium");
        assert_eq!(data.find("helium").unwrap().id, "chromium-based");
        data.entry(1, "Helium", "Name=Something else");
        assert_eq!(
            data.find("helium").unwrap().id,
            "Helium",
            "ファイル名は大文字と小文字を区別せず、最も優先する"
        );
        assert_eq!(data.find("unknown"), None);
    }

    #[test]
    fn entries_shown_in_the_menu_come_first() {
        let data = Data::new("nodisplay");
        data.entry(0, "a-handler", "Name=Player\nNoDisplay=true");
        data.entry(1, "z-app", "Name=Player");
        assert_eq!(data.find("player").unwrap().id, "z-app");
    }

    #[test]
    fn ids_that_look_like_paths_are_rejected() {
        let data = Data::new("traversal");
        fs::write(
            data.dirs[0].join("secret.desktop"),
            "[Desktop Entry]\nName=S\n",
        )
        .unwrap();
        assert_eq!(data.find("../secret"), None);
        assert_eq!(data.find(""), None);
    }

    #[test]
    fn icons_come_from_an_absolute_path_or_the_closest_theme_size() {
        let data = Data::new("icons");
        let absolute = data.root.join("icon-without-extension");
        // 拡張子の無いファイルも、内容から形式を判定する。
        data.png(&data.root.join("source.png"), 256);
        fs::rename(data.root.join("source.png"), &absolute).unwrap();
        data.entry(0, "abs", &format!("Name=A\nIcon={}", absolute.display()));
        let icon = data.find("abs").unwrap().icon.unwrap();
        assert_eq!((icon.width, icon.height), (64, 64), "64px に縮小する");

        data.entry(0, "themed", "Name=T\nIcon=themed");
        let theme =
            |size: u32| data.dirs[1].join(format!("icons/hicolor/{size}x{size}/apps/themed.png"));
        data.png(&data.dirs[1].join("pixmaps/themed.png"), 10);
        assert_eq!(data.find("themed").unwrap().icon.unwrap().width, 10);
        data.png(&theme(32), 32);
        assert_eq!(data.find("themed").unwrap().icon.unwrap().width, 32);
        data.png(&theme(48), 48);
        assert_eq!(data.find("themed").unwrap().icon.unwrap().width, 48);
        data.png(&theme(128), 128);
        assert_eq!(data.find("themed").unwrap().icon.unwrap().width, 64);

        data.entry(0, "svg", "Name=S\nIcon=missing");
        assert_eq!(data.find("svg").unwrap().icon, None);
    }
}
