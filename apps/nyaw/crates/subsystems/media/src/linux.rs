//! Linux: MPRIS（D-Bus のセッションバスに現れる `org.mpris.MediaPlayer2.*`）。ユーザーが切り替えで選択したプレイヤー、
//! または再生中のプレイヤーを返す（選び方は AGENT.md を参照）。

use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::Read,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

use zbus::{
    blocking::{Connection, connection::Builder},
    export::serde::Serialize,
    zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

use super::{Backend, Command, Controls, Image, Pick, Raw, fallback_name, picked};

/// プレイヤーのバス名の接頭辞。
const PREFIX: &str = "org.mpris.MediaPlayer2.";
/// 他のプレイヤーを中継するデーモン。中継元と重複するため除外する。
const PROXY: &str = "org.mpris.MediaPlayer2.playerctld";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// プレイヤーの応答を待つ上限。応答しないプレイヤーがあっても、ワーカーを止め続けない。
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
/// ジャケットの取得を待つ上限。
const ART_TIMEOUT: Duration = Duration::from_secs(5);
/// ジャケットの大きさの上限（バイト）。
const ART_MAX_BYTES: u64 = 10 << 20;

type Properties = HashMap<String, OwnedValue>;

/// 直前の [`Backend::read`] で選んだプレイヤー。操作の送信先と、ジャケットおよびアプリ情報の取得に使う。
struct Current {
    bus_name: String,
    track_id: Option<OwnedObjectPath>,
    art_url: String,
    identity: String,
    desktop_entry: String,
}

/// https の URL からジャケットを取得する処理。
type Fetch = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// https のジャケットの取得。取得は数秒かかることがあるため、別のスレッドで行い、曲の情報の更新と操作を待たせない。
struct Download {
    url: String,
    receiver: Receiver<Option<Vec<u8>>>,
    /// 取得の結果。取得中は `None`、失敗は `Some(None)`。
    result: Option<Option<Vec<u8>>>,
}

impl Download {
    fn start(url: String, fetch: Fetch) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let target = url.clone();
        let spawned = thread::Builder::new()
            .name("media-art".into())
            .spawn(move || {
                let _ = sender.send(fetch(&target));
            });
        if let Err(e) = spawned {
            logger::warn!("cannot start the artwork download", error = e);
        }
        Self {
            url,
            receiver,
            result: None,
        }
    }

    /// 取得が終わっていれば `true`。
    fn finished(&mut self) -> bool {
        if self.result.is_none() {
            match self.receiver.try_recv() {
                Ok(bytes) => self.result = Some(bytes),
                Err(mpsc::TryRecvError::Disconnected) => self.result = Some(None),
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.result.is_some()
    }
}

pub(crate) struct Session {
    bus: Connection,
    current: Option<Current>,
    fetch: Fetch,
    /// 最後に取得を始めた https のジャケット。
    download: Option<Download>,
}

/// 再生中または一時停止中のプレイヤーの状態。
#[derive(Debug, Clone, PartialEq)]
struct State {
    playing: bool,
    position: Duration,
    duration: Option<Duration>,
    title: String,
    artist: String,
    album: String,
    art_url: String,
    /// シーク（`SetPosition`）に渡す曲の ID。
    track_id: Option<OwnedObjectPath>,
    controls: Controls,
}

impl Session {
    pub(crate) fn new() -> Option<Self> {
        Builder::session()
            .and_then(Self::connect)
            .map_err(|e| logger::info!("cannot connect to the session bus", error = e))
            .ok()
    }

    fn connect(builder: Builder<'_>) -> zbus::Result<Self> {
        // 接続を再利用するため、エージェントは 1 つだけ作る。
        let agent = agent();
        Ok(Self {
            bus: builder.method_timeout(CALL_TIMEOUT).build()?,
            current: None,
            fetch: Arc::new(move |url| {
                fetch(&agent, url)
                    .map_err(|e| logger::debug!("cannot download the artwork", error = e))
                    .ok()
            }),
            download: None,
        })
    }

    /// `url` が https なら取得を始め、取得中かどうかを返す。
    fn downloading(&mut self, url: &str) -> bool {
        if !url.starts_with("https://") {
            return false;
        }
        if self.download.as_ref().is_none_or(|d| d.url != url) {
            self.download = Some(Download::start(url.to_owned(), self.fetch.clone()));
        }
        self.download.as_mut().is_some_and(|d| !d.finished())
    }

    /// プレイヤーのバス名。`ListNames` の順序は定まらないため、名前順に並べる。
    fn players(&self) -> zbus::Result<Vec<String>> {
        let mut names: Vec<String> = self
            .bus
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "ListNames",
                &(),
            )?
            .body()
            .deserialize()?;
        names.retain(|n| n.starts_with(PREFIX) && n != PROXY);
        names.sort();
        Ok(names)
    }

    fn properties(&self, bus_name: &str, interface: &str) -> zbus::Result<Properties> {
        self.bus
            .call_method(
                Some(bus_name),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(interface,),
            )?
            .body()
            .deserialize()
    }

    fn call<B: Serialize + DynamicType>(
        &self,
        bus_name: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<()> {
        self.bus
            .call_method(Some(bus_name), PATH, Some(interface), method, body)
            .map(drop)
    }
}

/// 一覧に含めるプレイヤー（バス名、状態、`org.mpris.MediaPlayer2` のプロパティ）。
struct Candidate {
    bus_name: String,
    state: State,
    root: Properties,
}

impl Candidate {
    fn identity(&self) -> String {
        string(&self.root, "Identity")
    }

    fn desktop_entry(&self) -> String {
        string(&self.root, "DesktopEntry")
    }

    /// アプリの ID。`DesktopEntry`、`Identity`、バス名の順に、最初に得られるものを使う。
    fn app_id(&self) -> String {
        [self.desktop_entry(), self.identity()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let name = self.bus_name.strip_prefix(PREFIX).unwrap_or(&self.bus_name);
                // 同じアプリの複数の起動は ".instance<番号>" で区別される。
                name.split(".instance").next().unwrap_or(name).to_owned()
            })
    }
}

/// 表示するプレイヤーを選ぶ。MPRIS には OS が現在のプレイヤーを定める仕組みが無いため、再生中のものを優先し、
/// すべて一時停止していれば直前のもの（`last`）を保つ。`playing` は各候補のバス名と再生中かどうか。
fn choose(playing: &[(&str, bool)], last: Option<&str>) -> Option<usize> {
    let last = playing.iter().position(|(name, _)| Some(*name) == last);
    last.filter(|&i| playing[i].1)
        .or_else(|| playing.iter().position(|(_, playing)| *playing))
        .or(last)
        .or((!playing.is_empty()).then_some(0))
}

impl Backend for Session {
    fn read(&mut self, pick: Option<&Pick>) -> Option<Raw> {
        // 取得に失敗したプレイヤー（終了した直後、応答なしなど）は一覧から除く。
        let mut candidates: Vec<Candidate> = self
            .players()
            .ok()?
            .into_iter()
            .filter_map(|bus_name| {
                let state = state(&self.properties(&bus_name, PLAYER).ok()?)?;
                let root = self.properties(&bus_name, ROOT).ok()?;
                Some(Candidate {
                    bus_name,
                    state,
                    root,
                })
            })
            .collect();
        let sessions: Vec<String> = candidates.iter().map(Candidate::app_id).collect();
        let index = picked(&sessions, pick).or_else(|| {
            let playing: Vec<(&str, bool)> = candidates
                .iter()
                .map(|c| (c.bus_name.as_str(), c.state.playing))
                .collect();
            choose(&playing, self.current.as_ref().map(|c| c.bus_name.as_str()))
        })?;
        let mut chosen = candidates.swap_remove(index);
        let app_id = sessions[index].clone();
        chosen.state.controls.raise = boolean(&chosen.root, "CanRaise");
        let (identity, desktop_entry) = (chosen.identity(), chosen.desktop_entry());
        let state = chosen.state;
        let loading = self.downloading(&state.art_url);
        self.current = Some(Current {
            bus_name: chosen.bus_name,
            track_id: state.track_id,
            art_url: state.art_url.clone(),
            identity,
            desktop_entry,
        });
        Some(Raw {
            // ジャケットの URL は曲の情報より後に変わることがあるため、キーに含める。https の取得の完了もキーを変え、
            // 画像を読み直させる。
            art_key: format!(
                "{app_id}\n{}\n{}\n{}\n{}\n{loading}",
                state.title, state.artist, state.album, state.art_url
            ),
            title: state.title,
            artist: state.artist,
            album: state.album,
            playing: state.playing,
            position: state.position,
            duration: state.duration,
            app_id,
            controls: state.controls,
            sessions,
            index,
        })
    }

    fn art(&mut self) -> Option<Vec<u8>> {
        let url = &self.current.as_ref()?.art_url;
        match &self.download {
            Some(download) if download.url == *url => download.result.clone().flatten(),
            _ => read_art_file(url),
        }
    }

    fn app(&mut self, id: &str) -> (String, Option<Image>, Option<String>) {
        let (identity, desktop_entry) = self.current.as_ref().map_or(("", ""), |c| {
            (c.identity.as_str(), c.desktop_entry.as_str())
        });
        // DesktopEntry を持たないプレイヤー（Chromium 系など）は、名前で照合する。
        let installed =
            desktop::installed_app(desktop_entry).or_else(|| desktop::installed_app(identity));
        // 表示名は、プレイヤーが申告する名前を優先する（.desktop の名前は "Spotify (Launcher)" などの場合がある）。
        let name = Some(identity.to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| installed.as_ref().map(|a| a.name.clone()))
            .unwrap_or_else(|| fallback_name(id));
        let icon = installed.and_then(|a| a.icon).map(|i| Image {
            width: i.width,
            height: i.height,
            rgba: i.rgba,
        });
        (name, icon, None)
    }

    fn command(&mut self, command: Command) {
        let Some(current) = &self.current else {
            return;
        };
        let to = current.bus_name.as_str();
        let done = match command {
            Command::PlayPause => self.call(to, PLAYER, "PlayPause", &()),
            Command::Next => self.call(to, PLAYER, "Next", &()),
            Command::Previous => self.call(to, PLAYER, "Previous", &()),
            Command::Seek(at) => {
                let Some(track) = &current.track_id else {
                    return;
                };
                let micros = i64::try_from(at.as_micros()).unwrap_or(i64::MAX);
                self.call(to, PLAYER, "SetPosition", &(track.as_ref(), micros))
            }
            Command::Raise => self.call(to, ROOT, "Raise", &()),
            Command::Switch(_) => return,
        };
        if let Err(e) = done {
            logger::warn!("media command failed", command = ?command, error = e);
        }
    }
}

/// `org.mpris.MediaPlayer2.Player` のプロパティから状態を作る。停止中は `None`。
fn state(player: &Properties) -> Option<State> {
    let playing = match string(player, "PlaybackStatus").as_str() {
        "Playing" => true,
        "Paused" => false,
        _ => return None,
    };
    let metadata = metadata(player);
    let field = |key: &str| metadata.get(key).copied();
    let text = |key: &str| field(key).and_then(text).unwrap_or_default();
    let micros = |value: i64| Duration::from_micros(value.max(0) as u64);
    let duration = field("mpris:length")
        .and_then(integer)
        .filter(|l| *l > 0)
        .map(micros);
    // 仕様の型はオブジェクトパスだが、文字列で返すプレイヤー（Spotify）がある。
    let track_id = field("mpris:trackid")
        .and_then(self::text)
        .and_then(|id| ObjectPath::try_from(id).ok())
        .map(OwnedObjectPath::from);
    let control = boolean(player, "CanControl");
    let controls = Controls {
        play_pause: control && boolean(player, "CanPause"),
        next: control && boolean(player, "CanGoNext"),
        previous: control && boolean(player, "CanGoPrevious"),
        seek: control && boolean(player, "CanSeek") && track_id.is_some() && duration.is_some(),
        // `org.mpris.MediaPlayer2` のプロパティで決まる。
        raise: false,
    };
    Some(State {
        playing,
        position: player
            .get("Position")
            .and_then(|v| integer(v))
            .map_or(Duration::ZERO, micros),
        duration,
        title: text("xesam:title"),
        artist: field("xesam:artist").map(names).unwrap_or_default(),
        album: text("xesam:album"),
        art_url: text("mpris:artUrl"),
        track_id,
        controls,
    })
}

/// 値が variant に包まれていれば、中身を返す。
fn inner<'a>(mut value: &'a Value<'a>) -> &'a Value<'a> {
    while let Value::Value(boxed) = value {
        value = boxed;
    }
    value
}

/// `Metadata`（`a{sv}`）の項目。
fn metadata(player: &Properties) -> HashMap<&str, &Value<'_>> {
    let Some(Value::Dict(dict)) = player.get("Metadata").map(|v| inner(v)) else {
        return HashMap::new();
    };
    dict.iter()
        .filter_map(|(key, value)| match inner(key) {
            Value::Str(key) => Some((key.as_str(), inner(value))),
            _ => None,
        })
        .collect()
}

fn text(value: &Value<'_>) -> Option<String> {
    match inner(value) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

/// 整数。仕様の型（`x`）と異なる型を返すプレイヤーがある（Spotify の `mpris:length` は `t`）。
fn integer(value: &Value<'_>) -> Option<i64> {
    match inner(value) {
        Value::I64(v) => Some(*v),
        Value::U64(v) => Some(i64::try_from(*v).unwrap_or(i64::MAX)),
        Value::I32(v) => Some((*v).into()),
        Value::U32(v) => Some((*v).into()),
        _ => None,
    }
}

/// 名前の一覧（`as`）を連結する。単独の文字列も受け付ける。
fn names(value: &Value<'_>) -> String {
    match inner(value) {
        Value::Array(list) => list
            .iter()
            .filter_map(text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => text(other).unwrap_or_default(),
    }
}

fn string(properties: &Properties, key: &str) -> String {
    properties
        .get(key)
        .and_then(|v| text(v))
        .unwrap_or_default()
}

fn boolean(properties: &Properties, key: &str) -> bool {
    matches!(
        properties.get(key).map(|v| inner(v)),
        Some(Value::Bool(true))
    )
}

/// `file` の URL のジャケットのファイル内容。URL はプレイヤーが申告する値であるため、ジャケットとして読むのは
/// `file` と `https`（[`fetch`]）だけで、どちらも大きさに上限を設ける。
fn read_art_file(url: &str) -> Option<Vec<u8>> {
    let path = file_path(url)?;
    // FIFO やデバイスは、開くだけで待機し得るため、通常のファイルだけを読む。
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > ART_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(ART_MAX_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(ART_TIMEOUT))
        // リダイレクト先にも https を要求する。
        .https_only(true)
        .user_agent(concat!("nyaw/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, ureq::Error> {
    agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(ART_MAX_BYTES)
        .read_to_vec()
}

/// `file://` の URL が指すパス（ホストが空か `localhost` のものに限る）。パーセントエンコードを復号する。
fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::with_capacity(path.len());
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let digit = |d: Option<u8>| (d? as char).to_digit(16);
        let (high, low) = (digit(input.next())?, digit(input.next())?);
        bytes.push((high * 16 + low) as u8);
    }
    Some(OsString::from_vec(bytes).into())
}

#[cfg(test)]
mod tests;
