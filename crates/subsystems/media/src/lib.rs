//! 再生中のメディア（音楽・動画）の情報と操作。Windows は GSMTC（`GlobalSystemMediaTransportControlsSessionManager`）、
//! Linux は MPRIS（D-Bus）。OS への問い合わせ・画像の読み込みとぼかしはワーカーが行い、メインスレッドは最新の結果を
//! 受け取るだけ（[`Media::now`]）。操作（[`Command`]）もワーカーへ送るだけで待たない。

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

/// OS に問い合わせ直す間隔。
const POLL: Duration = Duration::from_secs(1);
/// ジャケットの一辺の上限（px）。
const ART_PX: u32 = 512;
/// 背景用にぼかす前に縮める一辺（px）と、ぼかしの強さ。小さく縮めてからぼかすので軽い。
const BACKDROP_PX: u32 = 64;
const BACKDROP_BLUR: f32 = 6.0;
/// ぼかした背景を渡す一辺（px）。表示で引き伸ばしたときの縁の暗い帯が 1px に満たない大きさ。
const BACKDROP_OUT_PX: u32 = 512;
/// アプリのアイコンの一辺（px）。
const ICON_PX: u32 = 64;
/// 背景の平均の明るさ（相対輝度 0〜1）がこれより明るければ、載せる文字を暗くする。白と黒の文字のコントラストが釣り合う所。
const LIGHT_LUMINANCE: f32 = 0.18;

/// 画像（RGBA、ストレートアルファ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// 再生しているアプリ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// OS が付けた名前（Windows は AppUserModelId（"Spotify.exe" など）、Linux は D-Bus の名前の末尾）。
    pub id: String,
    /// 表示名（"Spotify" など）。
    pub name: String,
    pub icon: Option<Image>,
}

/// できる操作。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Controls {
    pub play_pause: bool,
    pub next: bool,
    pub previous: bool,
    pub seek: bool,
}

/// 今再生している（一時停止中も含む）メディア。
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// [`Self::at`] の時点の再生位置。
    pub position: Duration,
    pub duration: Option<Duration>,
    pub at: Instant,
    pub app: App,
    /// ジャケット。
    pub art: Option<Image>,
    /// ジャケットをぼかした背景。
    pub backdrop: Option<Image>,
    /// 背景（[`Self::backdrop`]）が明るいか。載せる文字・部品を暗くするか明るくするかの判断に使う。背景が無ければ `None`。
    pub light: Option<bool>,
    pub controls: Controls,
}

impl NowPlaying {
    /// 今の再生位置（再生中は読んだ時刻からの経過を足す。長さを超えない）。
    pub fn position_now(&self) -> Duration {
        let position = if self.playing {
            self.position + self.at.elapsed()
        } else {
            self.position
        };
        self.duration.map_or(position, |d| position.min(d))
    }
}

/// 再生中のアプリへの操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    /// 曲の頭からの位置へ移る。
    Seek(Duration),
    /// アプリの画面を前に出す（Linux の MPRIS だけ。Windows は何もしない）。
    Raise,
}

/// OS の 1 回分の問い合わせ結果（画像は読む前のもの）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
struct Raw {
    title: String,
    artist: String,
    album: String,
    playing: bool,
    position: Duration,
    duration: Option<Duration>,
    app_id: String,
    controls: Controls,
    /// ジャケットが変わったか見分ける鍵（同じなら読み直さない）。
    art_key: String,
}

/// OS ごとの問い合わせ先。ワーカーのスレッドだけが持つ。
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
trait Backend {
    /// 今再生している（一時停止中も含む）ものがあれば、その情報。
    fn read(&mut self) -> Option<Raw>;
    /// 最後の [`Backend::read`] のジャケットの画像のファイルの中身。
    fn art(&mut self) -> Option<Vec<u8>>;
    /// アプリ `id` の表示名と、アイコンの画像のファイルの中身。
    fn app(&mut self, id: &str) -> (String, Option<Vec<u8>>);
    fn command(&mut self, command: Command);
}

/// 再生中のメディアを見張るワーカー。落とすと止まる。
pub struct Media {
    latest: Arc<Mutex<Option<Arc<NowPlaying>>>>,
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
}

impl Media {
    /// ワーカーを起こす。OS が対応していなければ、何も再生していない扱いのまま。
    pub fn start() -> Self {
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
            while !stopping.load(Ordering::Acquire) {
                for command in rx.try_iter() {
                    backend.command(command);
                }
                let now = backend
                    .read()
                    .map(|raw| Arc::new(cache.resolve(raw, backend.as_mut())));
                *out.lock().unwrap() = now;
                // 操作が来たらすぐ読み直して、押した結果をすぐ見せる。
                match rx.recv_timeout(POLL) {
                    Ok(command) => backend.command(command),
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
    return linux::Mpris::new().map(|b| Box::new(b) as Box<dyn Backend>);
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
        if self.art.as_ref().is_none_or(|(key, _, _)| *key != raw.art_key) {
            let decoded = backend
                .art()
                .and_then(|bytes| image::load_from_memory(&bytes).ok());
            let art = decoded
                .as_ref()
                .map(|i| to_image(fit(i, ART_PX)));
            let backdrop = decoded.as_ref().map(|i| {
                let small = i.resize_exact(BACKDROP_PX, BACKDROP_PX, image::imageops::FilterType::Triangle);
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
            let (name, icon) = backend.app(&raw.app_id);
            let icon = icon
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|i| to_image(fit(&i, ICON_PX)));
            self.app = Some(App {
                id: raw.app_id.clone(),
                name,
                icon,
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
        }
    }
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
    let pixels = image.rgba.chunks_exact(4);
    let n = pixels.len().max(1) as f32;
    pixels
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

/// アプリの ID から表示名を作る（"Spotify.exe" → "Spotify"、"org.mpris.MediaPlayer2.vlc" → "vlc"）。表示名が取れないときに使う。
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn fallback_name(id: &str) -> String {
    let id = id.rsplit(['!', '\\', '/']).next().unwrap_or(id);
    let id = id.strip_suffix(".exe").or(id.strip_suffix(".EXE")).unwrap_or(id);
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
        fn read(&mut self) -> Option<Raw> {
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
        fn app(&mut self, id: &str) -> (String, Option<Vec<u8>>) {
            self.app_reads += 1;
            (fallback_name(id), None)
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
        let first = cache.resolve(fake.read().unwrap(), &mut fake);
        assert_eq!(first.app.name, "Spotify");
        let art = first.art.as_ref().unwrap();
        assert_eq!((art.width, art.height), (8, 8));
        let backdrop = first.backdrop.as_ref().unwrap();
        assert_eq!((backdrop.width, backdrop.height), (BACKDROP_OUT_PX, BACKDROP_OUT_PX));
        cache.resolve(fake.read().unwrap(), &mut fake);
        assert_eq!((fake.art_reads, fake.app_reads), (1, 1));
        fake.raw = raw("B");
        cache.resolve(fake.read().unwrap(), &mut fake);
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
        assert_eq!(fallback_name("org.mpris.MediaPlayer2.vlc"), "vlc");
        assert_eq!(
            fallback_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
    }
}
