//! テスト専用の D-Bus（`dbus-daemon`）に模擬プレイヤーを立て、実際の利用者のプレイヤーに影響を与えずに検証する。

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command as Process, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

use zbus::blocking::connection::Builder;

use super::*;
use crate::{Media, NowPlaying};

/// テスト専用のセッションバス。ドロップすると停止する。
struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start() -> Self {
        let mut daemon = Process::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("the tests need dbus-daemon on PATH");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_owned();
        assert!(!address.is_empty(), "dbus-daemon printed no address");
        Self { daemon, address }
    }

    fn builder(&self) -> Builder<'_> {
        Builder::address(self.address.as_str()).unwrap()
    }

    fn session(&self) -> Session {
        Session::connect(self.builder()).unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// 模擬プレイヤーが返す値と、受け取った操作。
#[derive(Clone)]
struct Model {
    identity: String,
    desktop_entry: String,
    can_raise: bool,
    can_control: bool,
    can_seek: bool,
    status: &'static str,
    position: i64,
    metadata: Vec<(&'static str, Value<'static>)>,
    calls: Vec<String>,
}

impl Model {
    fn new(identity: &str, status: &'static str) -> Self {
        Self {
            identity: identity.into(),
            desktop_entry: String::new(),
            can_raise: true,
            can_control: true,
            can_seek: true,
            status,
            position: 30_000_000,
            metadata: vec![
                ("xesam:title", format!("{identity} song").into()),
                ("xesam:artist", vec!["A", "B"].into()),
                ("xesam:album", "Album".into()),
                (
                    "mpris:trackid",
                    ObjectPath::from_static_str_unchecked("/track/1").into(),
                ),
                ("mpris:length", 200_000_000i64.into()),
            ],
            calls: Vec::new(),
        }
    }

    fn set(&mut self, key: &'static str, value: impl Into<Value<'static>>) {
        self.metadata.retain(|(k, _)| *k != key);
        self.metadata.push((key, value.into()));
    }

    fn properties(&self) -> Properties {
        let metadata: HashMap<&str, Value<'_>> = self.metadata.iter().cloned().collect();
        let can = |v: bool| Value::from(v);
        [
            ("PlaybackStatus", Value::from(self.status)),
            ("Metadata", metadata.into()),
            ("Position", self.position.into()),
            ("CanControl", can(self.can_control)),
            ("CanPause", can(true)),
            ("CanGoNext", can(true)),
            ("CanGoPrevious", can(false)),
            ("CanSeek", can(self.can_seek)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.try_into().unwrap()))
        .collect()
    }
}

type Shared = Arc<Mutex<Model>>;

struct Root(Shared);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.0.lock().unwrap().identity.clone()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        self.0.lock().unwrap().desktop_entry.clone()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        self.0.lock().unwrap().can_raise
    }

    fn raise(&self) {
        self.0.lock().unwrap().calls.push("Raise".into());
    }
}

struct Player(Shared);

impl Player {
    fn property(&self, key: &str) -> OwnedValue {
        self.0.lock().unwrap().properties().remove(key).unwrap()
    }

    fn record(&self, call: impl Into<String>) {
        self.0.lock().unwrap().calls.push(call.into());
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.lock().unwrap().status.into()
    }

    #[zbus(property)]
    fn metadata(&self) -> OwnedValue {
        self.property("Metadata")
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.0.lock().unwrap().position
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        self.0.lock().unwrap().can_control
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.0.lock().unwrap().can_seek
    }

    fn play_pause(&self) {
        let mut model = self.0.lock().unwrap();
        model.status = if model.status == "Playing" {
            "Paused"
        } else {
            "Playing"
        };
        model.calls.push("PlayPause".into());
    }

    fn next(&self) {
        self.record("Next");
    }

    fn previous(&self) {
        self.record("Previous");
    }

    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        self.record(format!("SetPosition {track} {position}"));
    }
}

/// バスに登録した模擬プレイヤー。ドロップするとバスから消える。
struct Fake {
    model: Shared,
    _connection: Connection,
}

impl Fake {
    fn start(bus: &Bus, suffix: &str, model: Model) -> Self {
        let model = Arc::new(Mutex::new(model));
        let connection = bus
            .builder()
            .serve_at(PATH, Root(model.clone()))
            .unwrap()
            .serve_at(PATH, Player(model.clone()))
            .unwrap()
            .name(format!("{PREFIX}{suffix}"))
            .unwrap()
            .build()
            .unwrap();
        Self {
            model,
            _connection: connection,
        }
    }

    fn edit(&self, edit: impl FnOnce(&mut Model)) {
        edit(&mut self.model.lock().unwrap());
    }

    fn calls(&self) -> Vec<String> {
        std::mem::take(&mut self.model.lock().unwrap().calls)
    }
}

#[test]
fn a_player_is_read_over_the_bus() {
    let bus = Bus::start();
    let mut session = bus.session();
    assert_eq!(session.read(None), None, "プレイヤーが無い");

    let mut model = Model::new("Fake Player", "Playing");
    model.desktop_entry = "fake-desktop-entry".into();
    let _player = Fake::start(&bus, "fake", model);
    let raw = session.read(None).unwrap();
    assert_eq!(
        (raw.title.as_str(), raw.artist.as_str(), raw.album.as_str()),
        ("Fake Player song", "A, B", "Album")
    );
    assert!(raw.playing);
    assert_eq!(raw.position, Duration::from_secs(30));
    assert_eq!(raw.duration, Some(Duration::from_secs(200)));
    assert_eq!(raw.app_id, "fake-desktop-entry");
    assert_eq!((raw.sessions.len(), raw.index), (1, 0));
    assert_eq!(
        raw.controls,
        Controls {
            play_pause: true,
            next: true,
            previous: false,
            seek: true,
            raise: true,
        }
    );
    // .desktop が無いアプリは、プレイヤーが申告する名前を表示する。
    assert_eq!(session.app(&raw.app_id).0, "Fake Player");
}

/// Spotify は、曲の ID を文字列で、曲の長さを符号なし整数で返す。
#[test]
fn values_with_the_types_spotify_uses_are_accepted() {
    let bus = Bus::start();
    let mut model = Model::new("Spotify", "Paused");
    model.set("mpris:trackid", "/com/spotify/track/abc");
    model.set("mpris:length", 203_108_000u64);
    let player = Fake::start(&bus, "spotify", model);
    let mut session = bus.session();
    let raw = session.read(None).unwrap();
    assert!(!raw.playing);
    assert_eq!(raw.duration, Some(Duration::from_micros(203_108_000)));
    assert!(raw.controls.seek);
    session.command(Command::Seek(Duration::from_millis(1500)));
    assert_eq!(
        player.calls(),
        ["SetPosition /com/spotify/track/abc 1500000"]
    );
}

#[test]
fn seeking_needs_a_track_id_and_a_length() {
    let bus = Bus::start();
    let mut model = Model::new("Player", "Playing");
    model.set("mpris:trackid", "not an object path");
    let player = Fake::start(&bus, "player", model);
    let mut session = bus.session();
    assert!(!session.read(None).unwrap().controls.seek);
    session.command(Command::Seek(Duration::from_secs(1)));
    assert!(player.calls().is_empty());

    player.edit(|m| {
        m.set(
            "mpris:trackid",
            ObjectPath::from_static_str_unchecked("/track/2"),
        );
        m.metadata.retain(|(k, _)| *k != "mpris:length");
    });
    let raw = session.read(None).unwrap();
    assert_eq!((raw.duration, raw.controls.seek), (None, false));
}

#[test]
fn controls_follow_what_the_player_allows() {
    let bus = Bus::start();
    let mut model = Model::new("Player", "Playing");
    model.can_control = false;
    model.can_raise = false;
    let _player = Fake::start(&bus, "player", model);
    assert_eq!(
        bus.session().read(None).unwrap().controls,
        Controls::default()
    );
}

#[test]
fn stopped_players_and_the_proxy_daemon_are_left_out() {
    let bus = Bus::start();
    let _stopped = Fake::start(&bus, "stopped", Model::new("Stopped", "Stopped"));
    let _proxy = Fake::start(&bus, "playerctld", Model::new("Proxy", "Playing"));
    let mut session = bus.session();
    assert_eq!(session.read(None), None);
    let _paused = Fake::start(&bus, "paused", Model::new("Paused", "Paused"));
    let raw = session.read(None).unwrap();
    assert_eq!((raw.app_id.as_str(), raw.sessions.len()), ("Paused", 1));
}

#[test]
fn the_playing_player_is_shown_and_kept_after_it_pauses() {
    let bus = Bus::start();
    let a = Fake::start(&bus, "a", Model::new("A", "Paused"));
    let b = Fake::start(&bus, "b", Model::new("B", "Playing"));
    let mut session = bus.session();
    assert_eq!(session.read(None).unwrap().app_id, "B");
    b.edit(|m| m.status = "Paused");
    assert_eq!(
        session.read(None).unwrap().app_id,
        "B",
        "すべて一時停止していれば、直前のプレイヤーを保つ"
    );
    a.edit(|m| m.status = "Playing");
    assert_eq!(session.read(None).unwrap().app_id, "A");
    b.edit(|m| m.status = "Playing");
    assert_eq!(
        session.read(None).unwrap().app_id,
        "A",
        "両方が再生中なら、直前のプレイヤーを保つ"
    );
    drop(a);
    assert_eq!(session.read(None).unwrap().app_id, "B");
}

#[test]
fn a_picked_player_receives_the_commands() {
    let bus = Bus::start();
    let a = Fake::start(&bus, "a.instance1", Model::new("Same", "Playing"));
    let b = Fake::start(&bus, "a.instance2", Model::new("Same", "Paused"));
    let mut session = bus.session();
    let raw = session.read(None).unwrap();
    assert_eq!(raw.sessions, ["Same", "Same"]);
    let pick = raw.switched(true).unwrap();
    let raw = session.read(Some(&pick)).unwrap();
    assert_eq!((raw.index, raw.playing), (1, false));
    for command in [
        Command::PlayPause,
        Command::Next,
        Command::Previous,
        Command::Raise,
    ] {
        session.command(command);
    }
    assert_eq!(b.calls(), ["PlayPause", "Next", "Previous", "Raise"]);
    assert!(a.calls().is_empty());
}

#[test]
fn a_player_without_names_is_identified_by_its_bus_name() {
    let bus = Bus::start();
    let _player = Fake::start(&bus, "vlc.instance42", Model::new("", "Playing"));
    let mut session = bus.session();
    let raw = session.read(None).unwrap();
    assert_eq!(raw.app_id, "vlc");
}

/// 応答しないプレイヤーがあっても、他のプレイヤーは読める。
#[test]
fn a_player_that_does_not_answer_is_skipped() {
    let bus = Bus::start();
    // 名前を取得するだけで、呼び出しを処理しない接続。
    let _silent = bus
        .builder()
        .name(format!("{PREFIX}aaa"))
        .unwrap()
        .build()
        .unwrap();
    let _player = Fake::start(&bus, "player", Model::new("Player", "Playing"));
    let mut session = Session::connect(bus.builder()).unwrap();
    let started = Instant::now();
    assert_eq!(session.read(None).unwrap().app_id, "Player");
    let waited = started.elapsed();
    assert!(
        (CALL_TIMEOUT..CALL_TIMEOUT * 3).contains(&waited),
        "応答しないプレイヤーを上限まで待つ: {waited:?}"
    );
}

/// ワーカー（[`Media`]）を模擬プレイヤーに接続し、公開 API でジャケットの読み込みと操作を検証する。
#[test]
fn the_worker_shows_the_player_and_forwards_commands() {
    let bus = Bus::start();
    let art = std::env::temp_dir().join(format!("nyaw art {}.png", std::process::id()));
    image::RgbaImage::from_pixel(8, 6, image::Rgba([200, 100, 50, 255]))
        .save(&art)
        .unwrap();
    let mut model = Model::new("Player", "Playing");
    model.set(
        "mpris:artUrl",
        format!("file://{}", art.display().to_string().replace(' ', "%20")),
    );
    let player = Fake::start(&bus, "player", model);

    let address = bus.address.clone();
    let media = Media::spawn(move || {
        let session = Session::connect(Builder::address(address.as_str()).ok()?).ok()?;
        Some(Box::new(session) as Box<dyn Backend>)
    });
    let wait = |ready: &dyn Fn(&NowPlaying) -> bool| {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(now) = media.now().filter(|now| ready(now)) {
                return now;
            }
            assert!(Instant::now() < end, "timed out: {:?}", media.now());
            thread::sleep(Duration::from_millis(10));
        }
    };
    let now = wait(&|_| true);
    assert_eq!(now.title, "Player song");
    assert_eq!(now.app.name, "Player");
    assert_eq!(now.art.as_ref().map(|a| (a.width, a.height)), Some((8, 6)));
    assert!(now.backdrop.is_some());

    media.send(Command::PlayPause);
    wait(&|now| !now.playing);
    assert_eq!(player.calls(), ["PlayPause"]);

    // ジャケットは、曲の情報が同じでも URL が変われば読み直す。
    player.edit(|m| m.set("mpris:artUrl", "http://example.invalid/art.png"));
    wait(&|now| now.art.is_none());
    drop(player);
    let end = Instant::now() + Duration::from_secs(10);
    while media.now().is_some() {
        assert!(Instant::now() < end, "the closed player stayed");
        thread::sleep(Duration::from_millis(10));
    }
    std::fs::remove_file(art).unwrap();
}

/// https のジャケットは別のスレッドで取得し、取得中も曲の情報と操作を扱える。
#[test]
fn a_slow_download_does_not_delay_the_player() {
    let bus = Bus::start();
    let mut model = Model::new("Player", "Playing");
    model.set("mpris:artUrl", "https://example.invalid/a.png");
    let player = Fake::start(&bus, "player", model);
    let mut session = bus.session();
    let (release, released) = mpsc::channel::<Vec<u8>>();
    let released = Mutex::new(released);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    session.fetch = Arc::new(move |url| {
        seen.lock().unwrap().push(url.to_owned());
        released.lock().unwrap().recv().ok()
    });

    let loading = session.read(None).unwrap();
    assert_eq!(session.art(), None);
    session.command(Command::PlayPause);
    assert_eq!(player.calls(), ["PlayPause"]);
    assert_eq!(session.read(None).unwrap().art_key, loading.art_key);

    release.send(vec![1, 2, 3]).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let loaded = loop {
        let raw = session.read(None).unwrap();
        if raw.art_key != loading.art_key {
            break raw;
        }
        assert!(Instant::now() < end, "the download never finished");
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(session.art(), Some(vec![1, 2, 3]));
    assert_eq!(session.read(None).unwrap().art_key, loaded.art_key);
    assert_eq!(
        *requests.lock().unwrap(),
        ["https://example.invalid/a.png"],
        "同じ URL は取得し直さない"
    );
}

#[test]
fn art_is_read_only_from_files_and_https() {
    assert_eq!(
        file_path("file:///tmp/a%20b/%E3%81%AB.png"),
        Some(PathBuf::from("/tmp/a b/に.png"))
    );
    assert_eq!(
        file_path("file://localhost/tmp/a"),
        Some(PathBuf::from("/tmp/a"))
    );
    assert_eq!(file_path("file://host/tmp/a"), None);
    assert_eq!(file_path("file:///tmp/%zz"), None);
    assert_eq!(file_path("https://example.com/a"), None);
    assert_eq!(
        read_art_file("file:///dev/zero"),
        None,
        "通常のファイルだけを読む"
    );
    assert_eq!(read_art_file("file:///nonexistent-nyaw"), None);
    assert_eq!(read_art_file("http://127.0.0.1:9/a.png"), None);
    assert_eq!(read_art_file(""), None);
    assert!(
        fetch(&agent(), "http://127.0.0.1:9/a.png")
            .is_err_and(|e| matches!(e, ureq::Error::RequireHttpsOnly(_))),
        "リダイレクト先にも使う設定は、http を接続の前に拒否する"
    );
}

#[test]
fn the_choice_prefers_playing_players() {
    assert_eq!(choose(&[], None), None);
    assert_eq!(choose(&[("a", false), ("b", false)], None), Some(0));
    assert_eq!(choose(&[("a", false), ("b", false)], Some("b")), Some(1));
    assert_eq!(choose(&[("a", false), ("b", true)], Some("a")), Some(1));
    assert_eq!(choose(&[("a", true), ("b", true)], Some("b")), Some(1));
    assert_eq!(choose(&[("a", false)], Some("closed")), Some(0));
}
