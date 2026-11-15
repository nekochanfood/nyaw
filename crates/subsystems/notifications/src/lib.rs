//! OS の通知（トースト）を読み、押されたら OS と同じ経路でアプリへ届ける。Windows は `UserNotificationListener`
//! （通知センターの一覧）と、通知の元の XML（ボタンと押したときの引数。OS の通知の DB にある）。
//! 問い合わせ・アイコンの読み込み・押したときの呼び出しはワーカーが行い、メインスレッドは最新の一覧を受け取るだけ
//! （[`Notifications::now`]）。押す（[`Notifications::activate`]）もワーカーへ送るだけで待たない。

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

#[cfg(windows)]
mod windows;

/// OS に問い合わせ直す間隔。
// ponytail: 見張り（NotificationChanged）はパッケージの ID が無いアプリでは使えないので読み直す。遅れは最長この間隔。
const POLL: Duration = Duration::from_secs(1);
/// アプリのアイコンの一辺（px）。
const ICON_PX: u32 = 64;

/// アイコンの画像（RGBA、ストレートアルファ。ミラーリングのアプリのアイコンと同じ型）。
pub use desktop::Icon as Image;

/// 通知を出したアプリ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// OS が付けた ID（Windows は AppUserModelId）。
    pub id: String,
    /// 表示名。
    pub name: String,
    pub icon: Option<Image>,
}

/// 通知のボタン。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub label: String,
    /// 押すと開く URL（画面がユーザーに確かめてから開く）。`None` なら [`Notifications::activate`] でアプリへ届ける。
    pub url: Option<String>,
}

/// 起動してから届いた通知（OS の通知センターに残っている間だけ）。
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// OS の通知の ID（起動している間は一意）。
    pub id: u32,
    pub app: App,
    pub title: String,
    /// 本文（2 行目から。行は `\n` でつなぐ）。
    pub body: String,
    pub arrived: SystemTime,
    /// 本文を押すと開く URL。`None` なら [`Notifications::activate`] でアプリへ届ける（開く）。
    pub url: Option<String>,
    /// 押せるボタン（届けられないものは含めない）。
    pub actions: Vec<Action>,
}

/// OS に出す通知（デバッグのテスト用。[`Notifications::send`]）。押すと開くものは、どれもウェブの URL
/// （確かめてから開く。アプリは開かない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub title: String,
    pub body: String,
    /// 本文を押すと開く URL。
    pub url: String,
    /// ボタン（文字と、押すと開く URL）。
    pub buttons: Vec<(String, String)>,
}

/// ワーカーへの頼みごと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// 通知の本文（`None`）かボタンを押した（[`Notifications::activate`]）。
    Activate(u32, Option<usize>),
    /// OS に通知を出す（[`Notifications::send`]）。
    Send(Outgoing),
}

/// 押したときにすること（ワーカーの中だけで持つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    /// URL を開く（画面が確かめてから開く）。
    Url(String),
    /// アプリの通知の受け口に引数を渡す（OS が通知を押したときと同じ）。
    Invoke(String),
    /// アプリを開く。
    Launch,
}

/// OS の 1 件分の問い合わせ結果。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
struct Raw {
    id: u32,
    app_id: String,
    app_name: String,
    /// 通知の文字（1 つ目が見出し）。
    texts: Vec<String>,
    arrived: SystemTime,
}

/// OS ごとの問い合わせ先。ワーカーのスレッドだけが持つ。
#[cfg_attr(not(windows), allow(dead_code))]
trait Backend {
    /// 通知センターにある通知。読めなければ `None`（前の一覧のまま）。
    fn read(&mut self) -> Option<Vec<Raw>>;
    /// 通知 `id` の元の XML。
    fn payload(&mut self, id: u32) -> Option<String>;
    /// 通知 `id` のアプリのアイコンの画像のファイルの中身。
    fn icon(&mut self, id: u32) -> Option<Vec<u8>>;
    /// アプリ `app` のスタートメニューのアイコン（通知がロゴを持たないとき）。
    fn shell_icon(&mut self, app: &str) -> Option<Image> {
        desktop::app_icon(app)
    }
    /// アプリ `app` が通知の受け口（押したときの引数を受け取る口）を持っているか。
    fn invokable(&mut self, app: &str) -> bool;
    /// 押したことをアプリへ届ける（`args` が `None` ならアプリを開く）。届けられたら `true`。
    fn activate(&mut self, app: &str, args: Option<&str>) -> bool;
    /// 通知センターから消す。
    fn remove(&mut self, id: u32);
    /// OS に通知を出す。
    fn send(&mut self, toast: &Outgoing);
}

/// 通知と、押したときにすること。
struct Entry {
    note: Arc<Notification>,
    launch: Plan,
    actions: Vec<Plan>,
}

/// ワーカーが持つ一覧。
#[derive(Default)]
struct State {
    /// 前に読んだ通知の ID。起動時にあったものは知らせず、新しく来たものだけを足す。`None` はまだ一度も読んでいない。
    seen: Option<HashSet<u32>>,
    live: Vec<Entry>,
    /// アプリごとのアイコン（読めなかったものも覚える）。
    icons: HashMap<String, Option<Image>>,
}

impl State {
    /// 読んだ一覧を反映する（消えたものを落とし、新しく来たものを足す）。変わったら `true`。
    fn update(&mut self, raws: Vec<Raw>, backend: &mut dyn Backend) -> bool {
        let ids: HashSet<u32> = raws.iter().map(|r| r.id).collect();
        let before = self.live.len();
        self.live.retain(|e| ids.contains(&e.note.id));
        let mut changed = self.live.len() != before;
        let fresh: Vec<Raw> = match &self.seen {
            Some(seen) => raws.into_iter().filter(|r| !seen.contains(&r.id)).collect(),
            None => Vec::new(),
        };
        self.seen = Some(ids);
        for raw in fresh {
            // 中身は書かない（人のやりとりが入る）。
            logger::debug!("notification arrived", id = raw.id, app = raw.app_id);
            let entry = self.entry(raw, backend);
            self.live.push(entry);
            changed = true;
        }
        changed
    }

    fn entry(&mut self, raw: Raw, backend: &mut dyn Backend) -> Entry {
        let invoke = backend.invokable(&raw.app_id);
        // ponytail: DB は リスナーに見えた時点で書かれている（実測）ので読み直さない。読めなければ本文で開くだけ。
        let (launch, actions) = backend
            .payload(raw.id)
            .map_or((Plan::Launch, Vec::new()), |xml| parse(&xml, invoke));
        let icon = self
            .icons
            .entry(raw.app_id.clone())
            // ストアのアプリは通知がロゴを持つ。デスクトップのアプリ（Chrome・Discord など）はスタートメニューから引く。
            .or_insert_with(|| {
                (backend.icon(raw.id).and_then(|b| decode(&b)))
                    .or_else(|| backend.shell_icon(&raw.app_id))
            })
            .clone();
        let mut texts = raw.texts.into_iter().filter(|t| !t.trim().is_empty());
        let title = texts.next().unwrap_or_default();
        let body = texts.collect::<Vec<_>>().join("\n");
        let url = |p: &Plan| match p {
            Plan::Url(u) => Some(u.clone()),
            _ => None,
        };
        let note = Notification {
            id: raw.id,
            app: App {
                id: raw.app_id,
                name: raw.app_name,
                icon,
            },
            title,
            body,
            arrived: raw.arrived,
            url: url(&launch),
            actions: actions
                .iter()
                .map(|(label, plan)| Action {
                    label: label.clone(),
                    url: url(plan),
                })
                .collect(),
        };
        Entry {
            note: Arc::new(note),
            launch,
            actions: actions.into_iter().map(|(_, p)| p).collect(),
        }
    }

    /// 通知 `id` の本文（`action` が `None`）かボタンを押す。アプリへ届けたら通知センターから消し、一覧からも落とす（`true`）。
    fn activate(&mut self, id: u32, action: Option<usize>, backend: &mut dyn Backend) -> bool {
        let Some(i) = self.live.iter().position(|e| e.note.id == id) else {
            return false;
        };
        let e = &self.live[i];
        let plan = match action {
            None => Some(&e.launch),
            Some(a) => e.actions.get(a),
        };
        let done = match plan {
            Some(Plan::Invoke(args)) => backend.activate(&e.note.app.id, Some(args)),
            Some(Plan::Launch) => backend.activate(&e.note.app.id, None),
            // URL は画面が確かめてから開く（ここには来ない）。
            Some(Plan::Url(_)) | None => false,
        };
        if done {
            backend.remove(id);
            self.live.remove(i);
        }
        done
    }

    fn list(&self) -> Arc<[Arc<Notification>]> {
        self.live.iter().map(|e| e.note.clone()).collect()
    }
}

/// 通知の元の XML（トーストの XML）から、本文を押したときとボタンを押したときにすることを読む。`invoke` はアプリが通知の
/// 受け口を持っているか（無ければ引数を渡せないので、本文はアプリを開くだけ・引数で動くボタンは出さない）。
/// 右クリックのメニュー（`placement`）・文字を打つ欄の横のボタン（`hint-inputId`）・OS のボタン（`system`）・
/// ウェブ以外の URL は出さない（URL は確かめてから開く決まりで、`http` / `https` しか開けない）。
fn parse(xml: &str, invoke: bool) -> (Plan, Vec<(String, Plan)>) {
    let Ok(doc) = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')) else {
        return (Plan::Launch, Vec::new());
    };
    let toast = doc.root_element();
    let plan = |kind: Option<&str>, args: &str| match kind.unwrap_or("foreground") {
        "protocol" => web(args).then(|| Plan::Url(args.to_owned())),
        "foreground" | "background" if invoke => Some(Plan::Invoke(args.to_owned())),
        _ => None,
    };
    let launch = plan(
        toast.attribute("activationType"),
        toast.attribute("launch").unwrap_or(""),
    )
    .unwrap_or(Plan::Launch);
    let actions = toast
        .descendants()
        .filter(|n| n.has_tag_name("action"))
        .filter(|a| a.attribute("placement").is_none() && a.attribute("hint-inputId").is_none())
        .filter_map(|a| {
            let label = a.attribute("content")?.trim();
            let plan = plan(
                a.attribute("activationType"),
                a.attribute("arguments").unwrap_or(""),
            )?;
            (!label.is_empty()).then(|| (label.to_owned(), plan))
        })
        .collect();
    (launch, actions)
}

/// ブラウザで開く URL か。
fn web(url: &str) -> bool {
    let scheme = url.split_once(':').map_or("", |(s, _)| s);
    scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
}

/// 画像のファイルの中身を読み、一辺 [`ICON_PX`] に収める。
fn decode(bytes: &[u8]) -> Option<Image> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width() > ICON_PX || image.height() > ICON_PX {
        image.thumbnail(ICON_PX, ICON_PX)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw().into(),
    })
}

/// 通知を見張るワーカー。落とすと止まる。
pub struct Notifications {
    latest: Arc<Mutex<Arc<[Arc<Notification>]>>>,
    requests: Sender<Request>,
    stop: Arc<AtomicBool>,
}

impl Notifications {
    /// ワーカーを起こす。OS が対応していない・通知へのアクセスが許されていなければ、何も届かないまま。
    pub fn start() -> Self {
        let latest: Arc<Mutex<Arc<[Arc<Notification>]>>> = Arc::new(Mutex::new(Arc::new([])));
        let stop = Arc::new(AtomicBool::new(false));
        let (requests, rx) = mpsc::channel::<Request>();
        let (out, stopping) = (latest.clone(), stop.clone());
        let spawned = thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                let Some(mut backend) = backend() else {
                    logger::info!("notifications are unavailable on this system");
                    return;
                };
                let mut state = State::default();
                let mut pending = Vec::new();
                while !stopping.load(Ordering::Acquire) {
                    let mut changed = false;
                    pending.extend(rx.try_iter());
                    for request in pending.drain(..) {
                        match request {
                            Request::Activate(id, action) => {
                                changed |= state.activate(id, action, backend.as_mut());
                            }
                            Request::Send(toast) => backend.send(&toast),
                        }
                    }
                    if let Some(raws) = backend.read() {
                        changed |= state.update(raws, backend.as_mut());
                    }
                    if changed {
                        *out.lock().unwrap() = state.list();
                    }
                    match rx.recv_timeout(POLL) {
                        Ok(a) => pending.push(a),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        if let Err(e) = spawned {
            logger::warn!("cannot start the notification worker", error = e);
        }
        Self {
            latest,
            requests,
            stop,
        }
    }

    /// OS を見ずに、決まった一覧 `list` を届いたものとして返す（E2E・見本用）。頼みごと（押した・出した）は受け口に届く。
    pub fn fixed(list: Vec<Notification>) -> (Self, mpsc::Receiver<Request>) {
        let (requests, received) = mpsc::channel();
        let list: Arc<[Arc<Notification>]> = list.into_iter().map(Arc::new).collect();
        let notifications = Self {
            latest: Arc::new(Mutex::new(list)),
            requests,
            stop: Arc::new(AtomicBool::new(false)),
        };
        (notifications, received)
    }

    /// 起動してから届いた通知（通知センターに残っているもの。届いた順）。
    pub fn now(&self) -> Arc<[Arc<Notification>]> {
        // 入れ替えるのは参照だけなので、待っても一瞬。読めないフレームに空を返すと、消えて届き直したように見える。
        self.latest.lock().unwrap().clone()
    }

    /// 通知 `id` の本文（`action` が `None`）か `action` 番目のボタンを押したことをアプリへ届ける（待たない）。
    /// 届けた通知は OS の通知センターからも消える。URL を開くもの（[`Notification::url`]）は画面が開く。
    pub fn activate(&self, id: u32, action: Option<usize>) {
        let _ = self.requests.send(Request::Activate(id, action));
    }

    /// OS に通知を出す（デバッグのテスト用。待たない）。Windows は PowerShell の名前で出す（nyaw は通知を出すための
    /// アプリの ID を登録していない）。届いた通知は、ほかのアプリのものと同じく一覧に入る。
    pub fn send(&self, toast: Outgoing) {
        let _ = self.requests.send(Request::Send(toast));
    }
}

impl Drop for Notifications {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn backend() -> Option<Box<dyn Backend>> {
    #[cfg(windows)]
    return windows::Listener::new().map(|b| Box::new(b) as Box<dyn Backend>);
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 決まった一覧を返す問い合わせ先。届けた・消した・アイコンを読んだ回数を覚える。
    #[derive(Default)]
    struct Fake {
        raws: Vec<Raw>,
        payloads: HashMap<u32, String>,
        invokable: bool,
        activated: Vec<(String, Option<String>)>,
        removed: Vec<u32>,
        icon_reads: usize,
    }

    impl Backend for Fake {
        fn read(&mut self) -> Option<Vec<Raw>> {
            Some(self.raws.clone())
        }
        fn payload(&mut self, id: u32) -> Option<String> {
            self.payloads.get(&id).cloned()
        }
        fn icon(&mut self, _: u32) -> Option<Vec<u8>> {
            self.icon_reads += 1;
            let mut png = Vec::new();
            image::RgbaImage::from_pixel(128, 128, image::Rgba([1, 2, 3, 255]))
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            Some(png)
        }
        fn shell_icon(&mut self, _: &str) -> Option<Image> {
            None
        }
        fn invokable(&mut self, _: &str) -> bool {
            self.invokable
        }
        fn activate(&mut self, app: &str, args: Option<&str>) -> bool {
            self.activated.push((app.into(), args.map(str::to_owned)));
            true
        }
        fn remove(&mut self, id: u32) {
            self.removed.push(id);
        }
        fn send(&mut self, _: &Outgoing) {}
    }

    fn raw(id: u32) -> Raw {
        Raw {
            id,
            app_id: "Chat".into(),
            app_name: "Chat".into(),
            texts: vec!["Hi".into(), "line 1".into(), " ".into(), "line 2".into()],
            arrived: SystemTime::UNIX_EPOCH,
        }
    }

    fn step(state: &mut State, fake: &mut Fake) -> bool {
        let raws = fake.read().unwrap();
        state.update(raws, fake)
    }

    /// 起動時に通知センターにあったものは知らせず、後から来たものだけを足し、通知センターから消えたら落とす。
    #[test]
    fn only_notifications_arriving_after_start_are_reported() {
        let mut fake = Fake {
            raws: vec![raw(1)],
            ..Default::default()
        };
        let mut state = State::default();
        assert!(!step(&mut state, &mut fake), "起動時にあったもの");
        assert!(state.list().is_empty());
        fake.raws.push(raw(2));
        assert!(step(&mut state, &mut fake));
        let list = state.list();
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), [2]);
        assert_eq!(
            (list[0].title.as_str(), list[0].body.as_str()),
            ("Hi", "line 1\nline 2")
        );
        assert!(!step(&mut state, &mut fake), "変わらなければ知らせない");
        fake.raws.retain(|r| r.id != 2);
        assert!(step(&mut state, &mut fake));
        assert!(state.list().is_empty());
    }

    /// アイコンはアプリごとに一度だけ読み、一辺 64px に収める。
    #[test]
    fn icons_are_read_once_per_app() {
        let mut fake = Fake::default();
        let mut state = State::default();
        step(&mut state, &mut fake);
        fake.raws = vec![raw(1), raw(2)];
        step(&mut state, &mut fake);
        assert_eq!(fake.icon_reads, 1);
        let icon = state.list()[1].app.icon.clone().unwrap();
        assert_eq!((icon.width, icon.height), (ICON_PX, ICON_PX));
    }

    const TOAST: &str = r#"<toast launch="open-chat" activationType="foreground">
        <visual><binding template="ToastGeneric"><text>Hi</text></binding></visual>
        <actions>
          <input id="reply" type="text"/>
          <action content="Send" hint-inputId="reply" arguments="send"/>
          <action content="Mute" arguments="mute" activationType="background"/>
          <action content="Open site" arguments="https://example.com/a" activationType="protocol"/>
          <action content="Settings" arguments="ms-settings:notifications" activationType="protocol"/>
          <action content="Snooze" arguments="snooze" activationType="system"/>
          <action content="Notification settings" placement="contextMenu" arguments="x"/>
        </actions></toast>"#;

    /// 受け口のあるアプリは、本文もボタンも引数を渡す。ウェブの URL のボタンは URL として出し、
    /// 打つ欄・OS のボタン・右クリックのメニュー・ウェブ以外の URL は出さない。
    #[test]
    fn actions_are_read_from_the_toast_xml() {
        let (launch, actions) = parse(TOAST, true);
        assert_eq!(launch, Plan::Invoke("open-chat".into()));
        assert_eq!(
            actions,
            [
                ("Mute".to_owned(), Plan::Invoke("mute".into())),
                (
                    "Open site".to_owned(),
                    Plan::Url("https://example.com/a".into())
                ),
            ]
        );
        // 受け口が無ければ、本文はアプリを開くだけで、引数で動くボタンは出さない。
        let (launch, actions) = parse(TOAST, false);
        assert_eq!(launch, Plan::Launch);
        assert_eq!(actions.len(), 1);
        // 本文が URL を開くトースト。
        let url = r#"<toast launch="HTTPS://example.com" activationType="protocol"/>"#;
        assert_eq!(parse(url, false).0, Plan::Url("HTTPS://example.com".into()));
        assert_eq!(parse("not xml", true), (Plan::Launch, Vec::new()));
    }

    /// 押すとアプリへ届け、通知センターから消して一覧からも落とす。URL のボタンはワーカーでは何もしない。
    #[test]
    fn activating_delivers_and_removes() {
        let mut fake = Fake {
            invokable: true,
            payloads: HashMap::from([(2, TOAST.to_owned())]),
            ..Default::default()
        };
        let mut state = State::default();
        step(&mut state, &mut fake);
        fake.raws = vec![raw(2), raw(3)];
        step(&mut state, &mut fake);
        let note = state.list()[0].clone();
        assert_eq!(
            note.actions[1].url.as_deref(),
            Some("https://example.com/a")
        );
        assert!(!state.activate(2, Some(1), &mut fake), "URL は画面が開く");
        assert!(!state.activate(2, Some(9), &mut fake));
        assert!(state.activate(2, Some(0), &mut fake));
        assert_eq!(
            fake.activated,
            [("Chat".to_owned(), Some("mute".to_owned()))]
        );
        assert_eq!(fake.removed, [2]);
        // XML が読めなかった通知は、本文でアプリを開く。
        assert!(state.activate(3, None, &mut fake));
        assert_eq!(fake.activated[1], ("Chat".to_owned(), None));
        assert!(state.list().is_empty());
    }
}
