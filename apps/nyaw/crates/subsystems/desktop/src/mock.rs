//! OS のキャプチャ・入力注入を使わず、上位層から本番と同一のインターフェースで検証する。
use super::*;

pub fn source(name: &str, kind: SourceKind) -> Source {
    Source {
        name: name.into(),
        kind,
        icon: None,
        app: None,
        target: 0,
    }
}

pub fn with_target(mut source: Source, target: isize) -> Source {
    source.target = target;
    source
}

pub struct Controller {
    update: Arc<Mutex<Option<Update>>>,
    input: Receiver<Input>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    min_interval: Arc<AtomicU64>,
    cursor: Arc<Mutex<Option<Arc<Cursor>>>>,
    popups: Arc<Mutex<Arc<[Popup]>>>,
}

pub fn stream() -> (Stream, Controller) {
    stream_of(None)
}

/// ウィンドウ `window` をキャプチャしているストリーム（[`Stream::window`]）。
pub fn window_stream(window: u64) -> (Stream, Controller) {
    stream_of(Some(window))
}

fn stream_of(window: Option<u64>) -> (Stream, Controller) {
    let update = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let min_interval = Arc::new(AtomicU64::new(0));
    let cursor = Arc::new(Mutex::new(None));
    let popups = Arc::new(Mutex::new(Arc::from([])));
    let (tx, rx) = mpsc::sync_channel(64);
    (
        Stream {
            update: update.clone(),
            input: tx,
            stop: stop.clone(),
            paused: paused.clone(),
            min_interval: min_interval.clone(),
            cursor: cursor.clone(),
            popups: popups.clone(),
            window,
        },
        Controller {
            update,
            input: rx,
            stop,
            paused,
            min_interval,
            cursor,
            popups,
        },
    )
}

impl Controller {
    pub fn publish(&self, update: Update) {
        *self.update.lock().unwrap() = Some(update);
    }
    /// 要求されている取得の間隔の下限（[`Stream::set_min_interval`]）。
    pub fn min_interval(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.min_interval.load(Ordering::Relaxed))
    }
    /// OS カーソルが変更されたことを通知する（[`Stream::cursor`]）。
    pub fn set_cursor(&self, cursor: Option<Cursor>) {
        *self.cursor.lock().unwrap() = cursor.map(Arc::new);
    }
    /// 取得元の手前に出ているメニューが変わったことを通知する（[`Stream::popups`]）。
    pub fn set_popups(&self, popups: Vec<Popup>) {
        *self.popups.lock().unwrap() = popups.into();
    }
    pub fn touches(&self) -> Vec<Touch> {
        self.input
            .try_iter()
            .filter_map(|input| match input {
                Input::Touch(touch) => Some(touch),
                _ => None,
            })
            .collect()
    }
    pub fn buttons(&self) -> Vec<(Touch, Button)> {
        self.input
            .try_iter()
            .filter_map(|input| match input {
                Input::Button(touch, button) => Some((touch, button)),
                _ => None,
            })
            .collect()
    }
    pub fn scrolls(&self) -> Vec<(f32, f32, f32, f32)> {
        self.input
            .try_iter()
            .filter_map(|input| match input {
                Input::Scroll { x, y, dx, dy } => Some((x, y, dx, dy)),
                _ => None,
            })
            .collect()
    }
    /// フォーカス要求を受けた回数（[`Stream::focus`]）。
    pub fn focuses(&self) -> usize {
        self.input
            .try_iter()
            .filter(|input| matches!(input, Input::Focus))
            .count()
    }
    pub fn minimizes(&self) -> usize {
        self.input
            .try_iter()
            .filter(|input| matches!(input, Input::Minimize))
            .count()
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
    /// キャプチャの一時停止が要求されているか（[`Stream::set_paused`]）。
    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }
}
