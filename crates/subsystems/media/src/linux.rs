//! Linux: MPRIS（D-Bus の `org.mpris.MediaPlayer2.*`）。再生中のプレイヤーを優先し、無ければ一時停止中のもの。

use std::{collections::HashMap, path::PathBuf, time::Duration};

use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{Backend, Command, Controls, Raw, fallback_name};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

pub(crate) struct Mpris {
    conn: Connection,
    /// 最後に読んだプレイヤー（D-Bus の名前）と、その曲（シークに要る）・ジャケットの URL。
    player: Option<String>,
    track: Option<OwnedObjectPath>,
    art_url: Option<String>,
}

impl Mpris {
    pub(crate) fn new() -> Option<Self> {
        Some(Self {
            conn: Connection::session().ok()?,
            player: None,
            track: None,
            art_url: None,
        })
    }

    /// `name` の `interface` のプロパティ `property`（プロキシのキャッシュを使わず、毎回 Get で読む）。
    fn get(&self, name: &str, interface: &str, property: &str) -> Option<OwnedValue> {
        Proxy::new(&self.conn, name, PATH, "org.freedesktop.DBus.Properties")
            .ok()?
            .call("Get", &(interface, property))
            .ok()
    }

    fn players(&self) -> Vec<String> {
        Proxy::new(
            &self.conn,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .ok()
        .and_then(|p| p.call::<_, _, Vec<String>>("ListNames", &()).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n.starts_with(PREFIX))
        .collect()
    }

    fn call(&self, interface: &str, method: &str) {
        let Some(name) = &self.player else {
            return;
        };
        let called = Proxy::new(&self.conn, name.as_str(), PATH, interface)
            .and_then(|p| p.call_method(method, &()));
        if let Err(e) = called {
            logger::warn!("media command failed", method = method, error = e);
        }
    }
}

impl Backend for Mpris {
    fn read(&mut self) -> Option<Raw> {
        let status = |name: &String| {
            self.get(name, PLAYER, "PlaybackStatus")
                .and_then(|v| text(&v))
                .unwrap_or_default()
        };
        let players = self.players();
        let (name, playing) = players
            .iter()
            .find(|n| status(n) == "Playing")
            .map(|n| (n.clone(), true))
            .or_else(|| {
                players
                    .iter()
                    .find(|n| status(n) == "Paused")
                    .map(|n| (n.clone(), false))
            })?;
        let metadata: HashMap<String, OwnedValue> = self
            .get(&name, PLAYER, "Metadata")
            .and_then(|v| HashMap::try_from(v).ok())
            .unwrap_or_default();
        let field = |key: &str| metadata.get(key);
        let title = field("xesam:title").and_then(|v| text(v)).unwrap_or_default();
        let artist = field("xesam:artist").map(|v| texts(v).join(", ")).unwrap_or_default();
        let album = field("xesam:album").and_then(|v| text(v)).unwrap_or_default();
        let micros = |v: &OwnedValue| number(v).map(|n| Duration::from_micros(n.max(0) as u64));
        let duration = field("mpris:length").and_then(micros).filter(|d| !d.is_zero());
        let position = self
            .get(&name, PLAYER, "Position")
            .and_then(|v| micros(&v))
            .unwrap_or_default();
        let flag = |p: &str| {
            self.get(&name, PLAYER, p)
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        };
        let controls = Controls {
            play_pause: flag("CanPause") || flag("CanPlay"),
            next: flag("CanGoNext"),
            previous: flag("CanGoPrevious"),
            seek: flag("CanSeek"),
            raise: self
                .get(&name, ROOT, "CanRaise")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false),
        };
        self.track = field("mpris:trackid").and_then(|v| match &**v {
            Value::ObjectPath(p) => Some(OwnedObjectPath::from(p.to_owned())),
            _ => None,
        });
        self.art_url = field("mpris:artUrl").and_then(|v| text(v));
        let app_id = name.trim_start_matches(PREFIX).to_owned();
        self.player = Some(name);
        Some(Raw {
            art_key: self.art_url.clone().unwrap_or_else(|| format!("{title}\n{artist}")),
            title,
            artist,
            album,
            playing,
            position,
            duration,
            app_id,
            controls,
        })
    }

    fn art(&mut self) -> Option<Vec<u8>> {
        // ponytail: 手元のファイル（file://）だけ読む。https のジャケット（Linux の Spotify など）は HTTP の取得を足すときに。
        let path = file_path(self.art_url.as_deref()?)?;
        std::fs::read(path).ok()
    }

    fn app(&mut self, id: &str) -> (String, Option<Vec<u8>>) {
        let name = format!("{PREFIX}{id}");
        let identity = self
            .get(&name, ROOT, "Identity")
            .and_then(|v| text(&v))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback_name(id));
        let icon = self
            .get(&name, ROOT, "DesktopEntry")
            .and_then(|v| text(&v))
            .and_then(|entry| desktop_icon(&entry))
            .and_then(|path| std::fs::read(path).ok());
        (identity, icon)
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::PlayPause => self.call(PLAYER, "PlayPause"),
            Command::Next => self.call(PLAYER, "Next"),
            Command::Previous => self.call(PLAYER, "Previous"),
            Command::Raise => self.call(ROOT, "Raise"),
            Command::Seek(at) => {
                let (Some(name), Some(track)) = (&self.player, &self.track) else {
                    return;
                };
                let micros = at.as_micros().min(i64::MAX as u128) as i64;
                let called = Proxy::new(&self.conn, name.as_str(), PATH, PLAYER)
                    .and_then(|p| p.call_method("SetPosition", &(track, micros)));
                if let Err(e) = called {
                    logger::warn!("media seek failed", error = e);
                }
            }
        }
    }
}

fn text(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn texts(v: &OwnedValue) -> Vec<String> {
    match &**v {
        Value::Array(a) => a
            .iter()
            .filter_map(|v| match v {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        Value::Str(s) => vec![s.to_string()],
        _ => Vec::new(),
    }
}

fn number(v: &OwnedValue) -> Option<i64> {
    match &**v {
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::I32(n) => Some(*n as i64),
        Value::U32(n) => Some(*n as i64),
        _ => None,
    }
}

/// `file://` の URL を手元のパスにする（%XX を戻す）。ほかの URL は `None`。
fn file_path(url: &str) -> Option<PathBuf> {
    let path = url.strip_prefix("file://")?;
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = path
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// デスクトップエントリ（"spotify" など）のアイコンの PNG のパス。.desktop の `Icon=` を見て、
/// 絶対パスならそれ、名前なら hicolor のテーマ・pixmaps から探す。
// ponytail: アイコンテーマの継承と SVG は見ない。足りなければ freedesktop のアイコンの探し方を足す。
fn desktop_icon(entry: &str) -> Option<PathBuf> {
    let dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let home = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
        std::env::var("HOME")
            .ok()
            .map(|h| format!("{h}/.local/share"))
    });
    let roots: Vec<PathBuf> = home
        .into_iter()
        .chain(dirs.split(':').map(str::to_owned))
        .map(PathBuf::from)
        .collect();
    let icon = roots
        .iter()
        .find_map(|r| std::fs::read_to_string(r.join("applications").join(format!("{entry}.desktop"))).ok())
        .and_then(|desktop| {
            desktop
                .lines()
                .find_map(|l| l.strip_prefix("Icon=").map(|i| i.trim().to_owned()))
        })
        .unwrap_or_else(|| entry.to_owned());
    if icon.starts_with('/') {
        return Some(PathBuf::from(icon));
    }
    let file = format!("{icon}.png");
    let sizes = ["128x128", "256x256", "64x64", "48x48"];
    roots
        .iter()
        .flat_map(|r| {
            sizes
                .iter()
                .map(|s| r.join("icons/hicolor").join(s).join("apps").join(&file))
                .chain(std::iter::once(r.join("pixmaps").join(&file)))
                .collect::<Vec<_>>()
        })
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(
            file_path("file:///home/a/My%20Music/cover.png"),
            Some(PathBuf::from("/home/a/My Music/cover.png"))
        );
        assert_eq!(file_path("https://i.scdn.co/image/x"), None);
    }
}
