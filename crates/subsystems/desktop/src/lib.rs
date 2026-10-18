//! デスクトップの画面・ウィンドウ取得とポインタ入力。OS 資源と待ちはワーカーだけが所有する。

use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub const DEFAULT_FRAME_RATE: u32 = 60;
pub const MAX_FRAME_RATE: u32 = 240;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(test, feature = "test-support"))]
pub mod mock;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Screen,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub kind: SourceKind,
    /// ウィンドウのアプリのアイコン（取れなければ `None`。画面には無い）。
    pub icon: Option<Icon>,
    /// ウィンドウのアプリの実行ファイル名（"Spotify.exe" など。Windows だけ。再生中のメディアのアプリと突き合わせる）。
    pub app: Option<String>,
    target: isize,
}

impl Source {
    /// 特定の画面・ウィンドウを指す。Linux の一覧は選択ダイアログの入口で、開くとダイアログが出る（指さない）。
    pub fn concrete(&self) -> bool {
        self.target != 0
    }

    /// `other` と同じ画面・ウィンドウを指すか（名前はウィンドウのタイトルで変わるので見ない）。
    /// 特定のものを指さない一覧（[`Self::concrete`]）は、どれとも同じにならない。
    pub fn same_target(&self, other: &Source) -> bool {
        self.concrete() && self.kind == other.kind && self.target == other.target
    }
}

/// アイコンの画像（RGBA、ストレートアルファ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Starting,
    Streaming,
    Closed,
    Failed(Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// OS が画面を取り込んだ時刻。分からなければ受け取った時刻。遅延の計測に使う。
    pub captured: Instant,
    /// ワーカーで GPU から読み出して並べ替えるのに掛かった時間。
    pub cost: Duration,
}

impl Frame {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, Error> {
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0 || height == 0 || bytes != Some(rgba.len()) || rgba.len() > 256 * 1024 * 1024
        {
            return Err(Error("invalid desktop frame dimensions or buffer".into()));
        }
        Ok(Self {
            width,
            height,
            rgba,
            captured: Instant::now(),
            cost: Duration::ZERO,
        })
    }

    /// GPU の行パディングを取り除き、事前乗算済み BGRA を RGBA にする。
    // shortcut: 画像転送は CPU 経由。大きな画面の転送が律速になったら GPU の共有テクスチャへ置き換える。
    fn from_bgra(width: u32, height: u32, stride: usize, data: &[u8]) -> Result<Self, Error> {
        let row = (width as usize)
            .checked_mul(4)
            .ok_or_else(|| Error("frame overflow".into()))?;
        let size = row
            .checked_mul(height as usize)
            .filter(|n| *n <= 256 * 1024 * 1024)
            .ok_or_else(|| Error("desktop frame too large".into()))?;
        let required = stride
            .checked_mul(height.saturating_sub(1) as usize)
            .and_then(|n| n.checked_add(row));
        if width == 0 || height == 0 || stride < row || required.is_none_or(|n| data.len() < n) {
            return Err(Error("invalid desktop frame stride or buffer".into()));
        }
        // 1 画素を u32 として並べ替え、コピーと同じ 1 回の走査で済ませる（バイト単位の入れ替えはベクトル化されず 3 倍遅い）。
        let mut rgba = vec![0; size];
        for (y, dst) in rgba.chunks_exact_mut(row).enumerate() {
            let src = &data[y * stride..y * stride + row];
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                let x = u32::from_le_bytes(*s);
                *d = (x & 0xff00_ff00 | (x >> 16) & 0xff | (x & 0xff) << 16).to_le_bytes();
            }
        }
        Self::new(width, height, rgba)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// 左上原点、0〜1 の座標。パネル外へ出た接点は端に留める。ID は左右の手に対応する 0 / 1。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    pub id: u32,
    pub phase: TouchPhase,
    pub x: f32,
    pub y: f32,
}

impl Touch {
    fn validated(mut self) -> Result<Self, Error> {
        if self.id >= 2 || !self.x.is_finite() || !self.y.is_finite() {
            return Err(Error("invalid desktop touch contact".into()));
        }
        self.x = self.x.clamp(0.0, 1.0);
        self.y = self.y.clamp(0.0, 1.0);
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct Update {
    pub status: Status,
    pub frame: Option<Arc<Frame>>,
}

/// 一覧の取得もフレームループで OS を待たない。
pub struct Sources(Receiver<Result<Vec<Source>, Error>>);

impl Sources {
    pub fn refresh() -> Result<Self, Error> {
        let (tx, rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("desktop-sources".into())
            .spawn(move || {
                #[cfg(windows)]
                let result = windows::sources();
                #[cfg(target_os = "linux")]
                let result = linux::sources();
                #[cfg(not(any(windows, target_os = "linux")))]
                let result = Err(Error(
                    "desktop capture is not available on this platform yet".into(),
                ));
                let _ = tx.send(result);
            })
            .map_err(|e| Error(e.to_string()))?;
        Ok(Self(rx))
    }

    pub fn poll(&self) -> Option<Result<Vec<Source>, Error>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(Error("desktop source worker stopped".into())))
            }
        }
    }
}

enum Input {
    Touch(Touch),
    Button(Touch, Button),
    Scroll { x: f32, y: f32, dx: f32, dy: f32 },
    Frame,
}

/// OS からのフレーム到着でワーカーを起こす。通知は一つだけ溜め、タッチの入る余地を潰さない。
#[derive(Clone)]
struct Wake {
    tx: SyncSender<Input>,
    pending: Arc<AtomicBool>,
}

impl Wake {
    #[cfg_attr(not(windows), allow(dead_code))]
    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.try_send(Input::Frame);
        }
    }
}

trait Session {
    fn frame(&mut self) -> Result<Option<Frame>, Error>;
    fn touch(&mut self, touch: Touch) -> Result<(), Error>;
    fn button(&mut self, touch: Touch, _button: Button) -> Result<(), Error> {
        self.touch(touch)
    }
    // shortcut: Linux portal session requests touchscreen only; request pointer access when Linux wheel input is needed.
    fn scroll(&mut self, _x: f32, _y: f32, _dx: f32, _dy: f32) -> Result<(), Error> {
        Ok(())
    }
    fn heartbeat(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn closed(&self) -> bool {
        false
    }
}

/// 最新画像は常に一枚だけ保持し、遅れたフレームを蓄積しない。
/// Drop は停止を通知するだけで、OS 資源の解放はワーカーが行う。
pub struct Stream {
    update: Arc<Mutex<Option<Update>>>,
    input: SyncSender<Input>,
    stop: Arc<AtomicBool>,
}

/// 到着を通知しない OS（PipeWire のポーリング）と接点の維持のため、待ちはこれより長くしない。
const POLL: Duration = Duration::from_millis(8);

impl Stream {
    pub fn start(source: Source) -> Result<Self, Error> {
        Self::start_at(source, DEFAULT_FRAME_RATE)
    }

    pub fn start_at(source: Source, frame_rate: u32) -> Result<Self, Error> {
        #[cfg(windows)]
        {
            Self::spawn(frame_rate, move |_, wake| {
                windows::Capture::new(source, wake)
            })
        }
        #[cfg(target_os = "linux")]
        {
            Self::spawn(frame_rate, move |stop, _| linux::Capture::new(source, stop))
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (source, frame_rate);
            Err(Error(
                "desktop capture is not available on this platform yet".into(),
            ))
        }
    }

    fn spawn<S: Session + 'static>(
        frame_rate: u32,
        open: impl FnOnce(Arc<AtomicBool>, Wake) -> Result<S, Error> + Send + 'static,
    ) -> Result<Self, Error> {
        if !(1..=MAX_FRAME_RATE).contains(&frame_rate) {
            return Err(Error(format!(
                "desktop frame rate must be between 1 and {MAX_FRAME_RATE}"
            )));
        }
        let interval = Duration::from_secs_f64(1. / f64::from(frame_rate));
        let update = Arc::new(Mutex::new(Some(Update {
            status: Status::Starting,
            frame: None,
        })));
        let stop = Arc::new(AtomicBool::new(false));
        let (input, rx) = mpsc::sync_channel(64);
        let wake = Wake {
            tx: input.clone(),
            pending: Arc::new(AtomicBool::new(false)),
        };
        let output = update.clone();
        let stopping = stop.clone();
        thread::Builder::new()
            .name("desktop-stream".into())
            .spawn(move || {
                let result = (|| {
                    let mut session = open(stopping.clone(), wake.clone())?;
                    let mut next_frame = Instant::now();
                    let mut inputs: Vec<Input> = Vec::new();
                    while !stopping.load(Ordering::Acquire) {
                        if session.closed() {
                            break;
                        }
                        // 期限を過ぎていれば到着の通知を待ち、まだなら期限まで待つ。どちらもタッチで起きる。
                        let wait = match next_frame.saturating_duration_since(Instant::now()) {
                            Duration::ZERO => POLL,
                            wait => wait.min(POLL),
                        };
                        let first = match rx.recv_timeout(wait) {
                            Ok(input) => Some(input),
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                        };
                        // 溜まったタッチを一度に処理し、同じ接点の連続した移動は最新だけ送る。
                        // 一件ずつだと画像の取得中に移動が溜まり、ポインタが遅れて止まって見え、キューも溢れる。
                        for input in first.into_iter().chain(rx.try_iter()) {
                            match (inputs.last_mut(), input) {
                                (Some(Input::Touch(last)), Input::Touch(touch))
                                    if last.id == touch.id
                                        && last.phase == TouchPhase::Move
                                        && touch.phase == TouchPhase::Move =>
                                {
                                    *last = touch
                                }
                                (_, input) => inputs.push(input),
                            }
                        }
                        for input in inputs.drain(..) {
                            match input {
                                Input::Touch(touch) => session.touch(touch)?,
                                Input::Button(touch, button) => session.button(touch, button)?,
                                Input::Scroll { x, y, dx, dy } => session.scroll(x, y, dx, dy)?,
                                Input::Frame => {}
                            }
                        }
                        session.heartbeat()?;
                        let now = Instant::now();
                        if now < next_frame {
                            continue;
                        }
                        // 取得の前に下ろし、取得中に届いた画像の通知を落とさない。
                        wake.pending.store(false, Ordering::Release);
                        // 画像が無ければ期限を進めず、到着しだい取る（取り逃しで一周期遅れない）。
                        if let Some(mut frame) = session.frame()? {
                            frame.cost = now.elapsed();
                            // 期限から一定の間隔で進める。取った時刻から数えると、次の画像の到着待ちが毎回足されて
                            // 上限より遅くなる（60 で 48 になっていた）。止まっていた後は今から数え直す。
                            next_frame = (next_frame + interval).max(now);
                            *output.lock().unwrap() = Some(Update {
                                status: Status::Streaming,
                                frame: Some(Arc::new(frame)),
                            });
                        }
                    }
                    Ok::<_, Error>(())
                })();
                *output.lock().unwrap() = Some(Update {
                    status: result.err().map_or(Status::Closed, Status::Failed),
                    frame: None,
                });
            })
            .map_err(|e| Error(e.to_string()))?;
        Ok(Self {
            update,
            input,
            stop,
        })
    }

    pub fn poll(&self) -> Option<Update> {
        self.update.try_lock().ok()?.take()
    }

    pub fn touch(&self, touch: Touch) -> Result<(), Error> {
        self.send(Input::Touch(touch.validated()?))
    }

    pub fn button(&self, touch: Touch, button: Button) -> Result<(), Error> {
        self.send(Input::Button(touch.validated()?, button))
    }

    pub fn scroll(&self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        if ![x, y, dx, dy].iter().all(|v| v.is_finite()) {
            return Err(Error("invalid desktop scroll".into()));
        }
        self.send(Input::Scroll {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            dx,
            dy,
        })
    }

    fn send(&self, input: Input) -> Result<(), Error> {
        self.input.try_send(input).map_err(|e| {
            // 押下・解放を落として操作を続けると接点が残るため、入力詰まりはセッションごと停止する。
            self.stop.store(true, Ordering::Release);
            Error(format!("desktop input delivery failed: {e}"))
        })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn converts_padded_bgra_and_preserves_premultiplied_alpha() {
        let frame =
            Frame::from_bgra(1, 2, 8, &[10, 20, 30, 40, 0, 0, 0, 0, 50, 60, 70, 80]).unwrap();
        assert_eq!(frame.rgba, [30, 20, 10, 40, 70, 60, 50, 80]);
        for (w, h, stride, buf) in [
            (0, 1, 4, &[0; 4][..]),
            (1, 0, 4, &[][..]),
            (2, 1, 4, &[0; 8][..]),
            (1, 2, 8, &[0; 8][..]),
        ] {
            assert!(Frame::from_bgra(w, h, stride, buf).is_err());
        }
        assert!(Frame::new(u32::MAX, u32::MAX, vec![]).is_err());
    }

    #[test]
    fn touch_rejects_nonfinite_coordinates_and_unknown_contacts() {
        let touch = Touch {
            id: 0,
            phase: TouchPhase::Down,
            x: -1.0,
            y: 2.0,
        };
        assert_eq!(
            touch.validated().unwrap(),
            Touch {
                x: 0.0,
                y: 1.0,
                ..touch
            }
        );
        assert!(Touch { id: 2, ..touch }.validated().is_err());
        assert!(
            Touch {
                x: f32::NAN,
                ..touch
            }
            .validated()
            .is_err()
        );
        assert!(
            Touch {
                y: f32::INFINITY,
                ..touch
            }
            .validated()
            .is_err()
        );
    }

    struct Fake {
        dropped: Arc<AtomicBool>,
        input: SyncSender<Touch>,
        frame: u8,
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }
    impl Session for Fake {
        fn frame(&mut self) -> Result<Option<Frame>, Error> {
            self.frame = self.frame.wrapping_add(1);
            Ok(Some(Frame::new(1, 1, vec![self.frame; 4])?))
        }
        fn touch(&mut self, touch: Touch) -> Result<(), Error> {
            self.input.send(touch).unwrap();
            Ok(())
        }
    }

    fn until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(Instant::now() < deadline, "worker timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn worker_delivers_frames_and_touch_and_releases_session_on_drop() {
        let dropped = Arc::new(AtomicBool::new(false));
        let observer = dropped.clone();
        let (tx, rx) = mpsc::sync_channel(4);
        let stream = Stream::spawn(DEFAULT_FRAME_RATE, move |_, _| {
            Ok(Fake {
                dropped,
                input: tx,
                frame: 0,
            })
        })
        .unwrap();
        until(|| stream.poll().is_some_and(|u| u.frame.is_some()));
        let touch = Touch {
            id: 1,
            phase: TouchPhase::Down,
            x: 0.25,
            y: 0.75,
        };
        stream.touch(touch).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), touch);
        drop(stream);
        until(|| observer.load(Ordering::Acquire));
    }

    #[test]
    fn worker_delivers_button_and_scroll_in_order() {
        struct Recorder(SyncSender<&'static str>);
        impl Session for Recorder {
            fn frame(&mut self) -> Result<Option<Frame>, Error> {
                Ok(None)
            }
            fn touch(&mut self, _: Touch) -> Result<(), Error> {
                Ok(())
            }
            fn button(&mut self, _: Touch, _: Button) -> Result<(), Error> {
                self.0.send("button").unwrap();
                Ok(())
            }
            fn scroll(&mut self, _: f32, _: f32, _: f32, _: f32) -> Result<(), Error> {
                self.0.send("scroll").unwrap();
                Ok(())
            }
        }
        let (tx, rx) = mpsc::sync_channel(2);
        let stream = Stream::spawn(DEFAULT_FRAME_RATE, move |_, _| Ok(Recorder(tx))).unwrap();
        let touch = Touch {
            id: 1,
            phase: TouchPhase::Down,
            x: 0.5,
            y: 0.5,
        };
        stream.button(touch, Button::Right).unwrap();
        stream.scroll(0.5, 0.5, 0.0, 1.0).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), "button");
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), "scroll");
    }

    #[test]
    fn worker_reports_open_failure() {
        let stream = Stream::spawn::<Fake>(DEFAULT_FRAME_RATE, |_, _| {
            Err(Error("permission denied".into()))
        })
        .unwrap();
        until(|| {
            stream
                .poll()
                .is_some_and(|u| u.status == Status::Failed(Error("permission denied".into())))
        });
    }

    #[test]
    fn invalid_frame_rates_do_not_open_an_os_session() {
        for rate in [0, MAX_FRAME_RATE + 1, u32::MAX] {
            assert!(
                Stream::spawn::<Fake>(rate, |_, _| panic!("invalid rate must not open a session"))
                    .is_err()
            );
        }
    }

    #[test]
    fn low_frame_rate_limits_capture_without_delaying_touch() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(4);
        let stream = Stream::spawn(1, move |_, _| {
            Ok(Fake {
                dropped,
                input: tx,
                frame: 0,
            })
        })
        .unwrap();
        until(|| stream.poll().is_some_and(|u| u.frame.is_some()));
        let touch = Touch {
            id: 0,
            phase: TouchPhase::Down,
            x: 0.5,
            y: 0.5,
        };
        let sent = Instant::now();
        stream.touch(touch).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_millis(250)).unwrap(), touch);
        assert!(sent.elapsed() < Duration::from_millis(250));
        thread::sleep(Duration::from_millis(80));
        assert!(
            stream.poll().is_none(),
            "1 FPS must not capture a second frame immediately"
        );
        until(|| {
            stream
                .poll()
                .is_some_and(|u| u.frame.is_some_and(|f| f.rgba == [2; 4]))
        });
    }

    /// 画像の取得中は止まり、その間に届いたタッチが溜まる。
    struct Busy {
        entered: SyncSender<()>,
        gate: Receiver<()>,
        input: SyncSender<Touch>,
    }
    impl Session for Busy {
        fn frame(&mut self) -> Result<Option<Frame>, Error> {
            let _ = self.entered.try_send(());
            self.gate
                .recv_timeout(Duration::from_secs(3))
                .ok()
                .map(|()| Frame::new(1, 1, vec![0; 4]))
                .transpose()
        }
        fn touch(&mut self, touch: Touch) -> Result<(), Error> {
            self.input.send(touch).unwrap();
            Ok(())
        }
    }

    #[test]
    fn touches_queued_during_capture_are_drained_and_moves_coalesced() {
        let (entered, entering) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let (tx, rx) = mpsc::sync_channel(64);
        let stream = Stream::spawn(DEFAULT_FRAME_RATE, move |_, _| {
            Ok(Busy {
                entered,
                gate,
                input: tx,
            })
        })
        .unwrap();
        entering.recv_timeout(Duration::from_secs(3)).unwrap();
        let at = |phase, x| Touch {
            id: 0,
            phase,
            x,
            y: 0.5,
        };
        // 取得中に溜まった移動は、最新の一件にまとめて送る。
        stream.touch(at(TouchPhase::Down, 0.0)).unwrap();
        for i in 1..=60 {
            stream
                .touch(at(TouchPhase::Move, i as f32 / 100.0))
                .unwrap();
        }
        stream.touch(at(TouchPhase::Up, 0.6)).unwrap();
        release.send(()).unwrap();
        let got: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(Duration::from_secs(3)).unwrap())
            .collect();
        assert_eq!(
            got,
            [
                at(TouchPhase::Down, 0.0),
                at(TouchPhase::Move, 0.6),
                at(TouchPhase::Up, 0.6)
            ]
        );
    }

    struct TouchFailure(Arc<AtomicBool>);
    impl Drop for TouchFailure {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    impl Session for TouchFailure {
        fn frame(&mut self) -> Result<Option<Frame>, Error> {
            Ok(None)
        }
        fn touch(&mut self, _: Touch) -> Result<(), Error> {
            Err(Error("injection denied".into()))
        }
    }

    #[test]
    fn injection_failure_reports_error_and_releases_the_os_session() {
        let dropped = Arc::new(AtomicBool::new(false));
        let observer = dropped.clone();
        let stream =
            Stream::spawn(DEFAULT_FRAME_RATE, move |_, _| Ok(TouchFailure(dropped))).unwrap();
        stream
            .touch(Touch {
                id: 0,
                phase: TouchPhase::Down,
                x: 0.5,
                y: 0.5,
            })
            .unwrap();
        until(|| {
            stream
                .poll()
                .is_some_and(|u| u.status == Status::Failed(Error("injection denied".into())))
        });
        assert!(observer.load(Ordering::Acquire));
    }
}
