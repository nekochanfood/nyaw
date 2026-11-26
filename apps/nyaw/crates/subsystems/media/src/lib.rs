//! 再生中のメディア（音楽・動画）の情報取得と操作。バックエンドは、Windows では GSMTC
//! （`GlobalSystemMediaTransportControlsSessionManager`）、Linux では MPRIS（D-Bus）を使用する。
//! OS への問い合わせ・画像の読み込みとぼかし処理はワーカースレッドが実行し、メインスレッドは最新の結果を
//! 参照するのみ（[`Media::now`]）。操作（[`Command`]）もワーカーへ送信するだけで待機しない。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// OS への再問い合わせ間隔。
const POLL: Duration = Duration::from_secs(1);
/// ジャケット画像の一辺の上限（px）。
const ART_PX: u32 = 512;
/// 背景用ぼかし前の縮小サイズ（px）とぼかし強度。小さく縮小してからぼかすため処理が軽量。
const BACKDROP_PX: u32 = 64;
const BACKDROP_BLUR: f32 = 6.0;
/// ぼかし背景の出力サイズ（px）。表示時の引き伸ばしで縁の暗い帯が 1px 未満になるサイズ。
const BACKDROP_OUT_PX: u32 = 512;
/// アプリアイコンの一辺（px）。
const ICON_PX: u32 = 64;
/// 背景の平均輝度（相対輝度 0〜1）がこの値を超える場合、前景テキストを暗色にする。白色テキストと黒色テキストのコントラストが均衡する閾値。
const LIGHT_LUMINANCE: f32 = 0.18;

/// 画像データ（RGBA、ストレートアルファ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// メディア再生中のアプリケーション情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// OS が割り当てた ID（バックエンド固有）。
    pub id: String,
    /// 表示名（"Spotify" など）。
    pub name: String,
    pub icon: Option<Image>,
    /// 実行ファイル名（"chrome.exe" 等）。ID が実行ファイル名と異なるアプリ（ブラウザ等）のウィンドウ検索に使用する。
    /// ID から解決できた場合のみ設定される（Windows のみ）。
    pub exe: Option<String>,
}

/// 利用可能な操作。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Controls {
    pub play_pause: bool,
    pub next: bool,
    pub previous: bool,
    pub seek: bool,
    /// プレイヤーウィンドウの前面化が可能。
    pub raise: bool,
}

/// 現在再生中（一時停止中を含む）のメディア情報。
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// [`Self::at`] 時点での再生位置。
    pub position: Duration,
    pub duration: Option<Duration>,
    pub at: Instant,
    pub app: App,
    /// ジャケット画像。
    pub art: Option<Image>,
    /// ジャケットをぼかした背景画像。
    pub backdrop: Option<Image>,
    /// 背景（[`Self::backdrop`]）が明るいかどうか。前景のテキスト・部品の明暗を決定する際に使用する。背景なしの場合は `None`。
    pub light: Option<bool>,
    pub controls: Controls,
    /// 再生中・一時停止中のメディアセッション数。2 以上の場合は [`Command::Switch`] で切り替え可能。
    pub sessions: usize,
}

impl NowPlaying {
    /// 現在の再生位置（再生中は取得時刻からの経過を加算する。duration を超えない）。
    pub fn position_now(&self) -> Duration {
        let position = if self.playing {
            self.position + self.at.elapsed()
        } else {
            self.position
        };
        self.duration.map_or(position, |d| position.min(d))
    }
}

/// 再生中のアプリへのコマンド。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    /// 曲の先頭からの指定位置へシークする。
    Seek(Duration),
    /// アプリのウィンドウを前面に出す（[`Controls::raise`] が有効な場合）。
    Raise,
    /// 表示・操作対象のメディアを次（`true`）または前（`false`）のセッションに切り替える。ワーカーが処理する。
    Switch(bool),
}

/// 切り替えで選択されたセッション: アプリの ID と、同一 ID のセッション中でのインデックス（ブラウザはタブごとに生成される）。
type Pick = (String, usize);

/// OS への 1 回の問い合わせ結果（画像は読み込み前の状態）。
#[derive(Debug, Clone, PartialEq)]
struct Raw {
    title: String,
    artist: String,
    album: String,
    playing: bool,
    position: Duration,
    duration: Option<Duration>,
    app_id: String,
    controls: Controls,
    /// ジャケット変更を判別するキー（同一であれば再読み込みしない）。
    art_key: String,
    /// 再生中・一時停止中セッションのアプリ ID（OS の並び順）と、取得対象のインデックス。
    sessions: Vec<String>,
    index: usize,
}

impl Raw {
    /// 次（`forward`）または前のセッションを返す。セッションが 1 つのみの場合は `None`。
    fn switched(&self, forward: bool) -> Option<Pick> {
        let n = self.sessions.len();
        if n < 2 {
            return None;
        }
        let i = (self.index + if forward { 1 } else { n - 1 }) % n;
        let id = &self.sessions[i];
        Some((
            id.clone(),
            self.sessions[..i].iter().filter(|s| *s == id).count(),
        ))
    }
}

/// `sessions` 内で `pick` に該当するセッションのインデックスを返す。該当なし（終了済み）の場合は `None`。
fn picked(sessions: &[String], pick: Option<&Pick>) -> Option<usize> {
    let (id, nth) = pick?;
    sessions
        .iter()
        .enumerate()
        .filter(|(_, s)| *s == id)
        .nth(*nth)
        .map(|(i, _)| i)
}

/// OS 固有の問い合わせバックエンド。ワーカースレッドのみが保持する。
trait Backend {
    /// 現在再生中（一時停止中を含む）のメディアがあればその情報を返す。`pick` のセッションがあればそれを、なければ OS が
    /// 最も関連性が高いと判断したものを取得し、以後の操作（[`Backend::command`]）もそのセッションに送信する。
    fn read(&mut self, pick: Option<&Pick>) -> Option<Raw>;
    /// 直前の [`Backend::read`] で取得したジャケット画像のファイル内容。
    fn art(&mut self) -> Option<Vec<u8>>;
    /// アプリ `id` の表示名、アイコン、および実行ファイル名（[`App::exe`]）を返す。
    fn app(&mut self, id: &str) -> (String, Option<Image>, Option<String>);
    fn command(&mut self, command: Command);
}

/// 再生中のメディアを監視するワーカー。ドロップすると停止する。
pub struct Media {
    latest: Arc<Mutex<Option<Arc<NowPlaying>>>>,
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
}

impl Media {
    /// ワーカーを起こす。OS が対応していなければ、何も再生していない扱いのまま。
    pub fn start() -> Self {
        Self::spawn(backend)
    }

    /// `backend` が返す問い合わせ先を監視するワーカーを起こす（問い合わせ先はワーカーのスレッドで作る）。
    fn spawn(backend: impl FnOnce() -> Option<Box<dyn Backend>> + Send + 'static) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (commands, rx) = mpsc::channel::<Command>();
        let (out, stopping) = (latest.clone(), stop.clone());
        let spawned = thread::Builder::new().name("media".into()).spawn(move || {
            let Some(mut backend) = backend() else {
                logger::info!("media sessions are unavailable on this system");
                return;
            };
            let mut cache = Cache::default();
            // 切り替えて選んだセッション（閉じたら OS の選ぶものに戻る）と、最後に読んだもの。
            let mut pick: Option<Pick> = None;
            let mut last: Option<Raw> = None;
            let mut pending: Vec<Command> = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                pending.extend(rx.try_iter());
                for command in pending.drain(..) {
                    match command {
                        Command::Switch(forward) => {
                            if let Some(p) = last.as_ref().and_then(|r| r.switched(forward)) {
                                pick = Some(p);
                            }
                        }
                        command => backend.command(command),
                    }
                }
                last = backend.read(pick.as_ref());
                // 選んだものが閉じたら選び直さない（また開いても、OS の選ぶものから戻らない）。
                if last
                    .as_ref()
                    .is_none_or(|r| picked(&r.sessions, pick.as_ref()) != Some(r.index))
                {
                    pick = None;
                }
                let now = last
                    .clone()
                    .map(|raw| Arc::new(cache.resolve(raw, backend.as_mut())));
                *out.lock().unwrap() = now;
                // 操作が来たらすぐ読み直して、押した結果をすぐ見せる。
                match rx.recv_timeout(POLL) {
                    Ok(command) => pending.push(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        if let Err(e) = spawned {
            logger::warn!("cannot start the media worker", error = e);
        }
        Self {
            latest,
            commands,
            stop,
        }
    }

    /// 最後に読んだ再生中のメディア。何も再生していなければ `None`。
    pub fn now(&self) -> Option<Arc<NowPlaying>> {
        self.latest.try_lock().ok()?.clone()
    }

    /// 操作をワーカーへ送る（待たない）。
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn backend() -> Option<Box<dyn Backend>> {
    #[cfg(windows)]
    return windows::Session::new().map(|b| Box::new(b) as Box<dyn Backend>);
    #[cfg(target_os = "linux")]
    return linux::Session::new().map(|b| Box::new(b) as Box<dyn Backend>);
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// 画像は変わったときだけ読み直す。
#[derive(Default)]
struct Cache {
    art: Option<(String, Option<Image>, Option<Image>)>,
    app: Option<App>,
}

impl Cache {
    fn resolve(&mut self, raw: Raw, backend: &mut dyn Backend) -> NowPlaying {
        if self
            .art
            .as_ref()
            .is_none_or(|(key, _, _)| *key != raw.art_key)
        {
            let decoded = backend
                .art()
                .and_then(|bytes| image::load_from_memory(&bytes).ok());
            let art = decoded.as_ref().map(|i| to_image(fit(i, ART_PX)));
            let backdrop = decoded.as_ref().map(|i| {
                let small = i.resize_exact(
                    BACKDROP_PX,
                    BACKDROP_PX,
                    image::imageops::FilterType::Triangle,
                );
                // 小さいまま渡すと、表示で引き伸ばすときに縁の半画素が外（透明）と混ざって暗い帯になる。
                // ぼかした後なので、大きくしても見た目は変わらない。
                to_image(blur_mirrored(&small).resize_exact(
                    BACKDROP_OUT_PX,
                    BACKDROP_OUT_PX,
                    image::imageops::FilterType::Triangle,
                ))
            });
            self.art = Some((raw.art_key.clone(), art, backdrop));
        }
        if self.app.as_ref().is_none_or(|a| a.id != raw.app_id) {
            let (name, icon, exe) = backend.app(&raw.app_id);
            self.app = Some(App {
                id: raw.app_id.clone(),
                name,
                icon,
                exe,
            });
        }
        let (_, art, backdrop) = self.art.clone().unwrap_or_default();
        let light = backdrop.as_ref().map(|b| luminance(b) > LIGHT_LUMINANCE);
        NowPlaying {
            title: raw.title,
            artist: raw.artist,
            album: raw.album,
            playing: raw.playing,
            position: raw.position,
            duration: raw.duration,
            at: Instant::now(),
            app: self.app.clone().expect("resolved above"),
            art,
            backdrop,
            light,
            controls: raw.controls,
            sessions: raw.sessions.len(),
        }
    }
}

/// アイコン画像のファイル内容を読み込み、[`ICON_PX`] に収める。
#[cfg_attr(not(windows), allow(dead_code))]
fn icon_from_file(bytes: &[u8]) -> Option<Image> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(to_image(fit(&image, ICON_PX)))
}

/// 画像の平均の相対輝度（0〜1。sRGB をリニアに直して重み付け）。
fn luminance(image: &Image) -> f32 {
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let pixels = image.rgba.as_chunks::<4>().0;
    let n = pixels.len().max(1) as f32;
    pixels
        .iter()
        .map(|p| 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]))
        .sum::<f32>()
        / n
}

/// 一辺が `max` を超えるときだけ、縦横比を保って縮める（小さい画像は大きくしない）。
fn fit(image: &image::DynamicImage, max: u32) -> image::DynamicImage {
    if image.width() > max || image.height() > max {
        image.thumbnail(max, max)
    } else {
        image.clone()
    }
}

/// 周りを鏡写しに広げてからぼかし、元の大きさに切り戻す。そのままぼかすと端の画素の引き伸ばしで縁にすじが残る。
fn blur_mirrored(image: &image::DynamicImage) -> image::DynamicImage {
    let src = image.to_rgba8();
    let (w, h) = src.dimensions();
    let pad = (BACKDROP_BLUR * 3.0).ceil() as u32;
    let mirror = |i: u32, n: u32| match i.checked_sub(pad) {
        None => pad - 1 - i,
        Some(i) if i >= n => 2 * n - 1 - i,
        Some(i) => i,
    };
    let padded = image::RgbaImage::from_fn(w + 2 * pad, h + 2 * pad, |x, y| {
        *src.get_pixel(mirror(x, w), mirror(y, h))
    });
    image::DynamicImage::ImageRgba8(padded)
        .blur(BACKDROP_BLUR)
        .crop_imm(pad, pad, w, h)
}

fn to_image(image: image::DynamicImage) -> Image {
    let rgba = image.into_rgba8();
    Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw().into(),
    }
}

/// アプリの ID から表示名を作る（"Spotify.exe" → "Spotify"、"org.example.Player" → "Player"）。表示名が取れないときに使う。
fn fallback_name(id: &str) -> String {
    let id = id.rsplit(['!', '\\', '/']).next().unwrap_or(id);
    let id = id
        .strip_suffix(".exe")
        .or(id.strip_suffix(".EXE"))
        .unwrap_or(id);
    id.rsplit('.').next().unwrap_or(id).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 決まった結果を返す問い合わせ先。画像を読んだ回数を数える。
    struct Fake {
        raw: Raw,
        art_reads: usize,
        app_reads: usize,
    }

    impl Backend for Fake {
        fn read(&mut self, _: Option<&Pick>) -> Option<Raw> {
            Some(self.raw.clone())
        }
        fn art(&mut self) -> Option<Vec<u8>> {
            self.art_reads += 1;
            let mut png = Vec::new();
            image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 50, 255]))
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            Some(png)
        }
        fn app(&mut self, id: &str) -> (String, Option<Image>, Option<String>) {
            self.app_reads += 1;
            (fallback_name(id), None, None)
        }
        fn command(&mut self, _: Command) {}
    }

    fn raw(title: &str) -> Raw {
        Raw {
            title: title.into(),
            artist: "Artist".into(),
            album: "Album".into(),
            playing: true,
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(200)),
            app_id: "Spotify.exe".into(),
            controls: Controls::default(),
            art_key: title.into(),
            sessions: vec!["Spotify.exe".into()],
            index: 0,
        }
    }

    #[test]
    fn images_are_read_only_when_they_change() {
        let mut fake = Fake {
            raw: raw("A"),
            art_reads: 0,
            app_reads: 0,
        };
        let mut cache = Cache::default();
        let first = cache.resolve(fake.read(None).unwrap(), &mut fake);
        assert_eq!(first.app.name, "Spotify");
        let art = first.art.as_ref().unwrap();
        assert_eq!((art.width, art.height), (8, 8));
        let backdrop = first.backdrop.as_ref().unwrap();
        assert_eq!(
            (backdrop.width, backdrop.height),
            (BACKDROP_OUT_PX, BACKDROP_OUT_PX)
        );
        cache.resolve(fake.read(None).unwrap(), &mut fake);
        assert_eq!((fake.art_reads, fake.app_reads), (1, 1));
        fake.raw = raw("B");
        cache.resolve(fake.read(None).unwrap(), &mut fake);
        assert_eq!((fake.art_reads, fake.app_reads), (2, 1));
    }

    #[test]
    fn position_runs_while_playing_and_stops_at_the_end() {
        let mut now = Cache::default().resolve(
            raw("A"),
            &mut Fake {
                raw: raw("A"),
                art_reads: 0,
                app_reads: 0,
            },
        );
        now.at = Instant::now() - Duration::from_secs(10);
        assert!(now.position_now() >= Duration::from_secs(40));
        now.playing = false;
        assert_eq!(now.position_now(), Duration::from_secs(30));
        now.playing = true;
        now.at = Instant::now() - Duration::from_secs(1000);
        assert_eq!(now.position_now(), Duration::from_secs(200));
    }

    /// 切り替えは前後に一周し、同じアプリのセッション（ブラウザのタブ）も何番目かで見分ける。
    #[test]
    fn switching_cycles_through_sessions_of_the_same_app() {
        let mut r = raw("A");
        r.sessions = vec!["Chrome".into(), "Spotify.exe".into(), "Chrome".into()];
        let pick = r.switched(false).unwrap();
        assert_eq!(pick, ("Chrome".to_owned(), 1));
        assert_eq!(picked(&r.sessions, Some(&pick)), Some(2));
        r.index = 2;
        assert_eq!(r.switched(true), Some(("Chrome".to_owned(), 0)));
        assert_eq!(
            picked(&r.sessions, Some(&("Chrome".to_owned(), 2))),
            None,
            "閉じた"
        );
        r.sessions.truncate(1);
        r.index = 0;
        assert_eq!(r.switched(true), None, "1 つだけなら切り替えない");
    }

    #[test]
    fn light_and_dark_backdrops_are_told_apart() {
        let solid = |v: u8| Image {
            width: 2,
            height: 2,
            rgba: [v, v, v, 255].repeat(4).into(),
        };
        assert!(luminance(&solid(255)) > LIGHT_LUMINANCE);
        assert!(luminance(&solid(20)) < LIGHT_LUMINANCE);
        // 中間の灰色（sRGB 128）は相対輝度 0.22 で、黒い文字のほうが読みやすい。
        assert!(luminance(&solid(128)) > LIGHT_LUMINANCE);
    }

    #[test]
    fn names_fall_back_to_the_app_id() {
        assert_eq!(fallback_name("Spotify.exe"), "Spotify");
        assert_eq!(fallback_name("org.example.Player"), "Player");
        assert_eq!(
            fallback_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
    }
}
