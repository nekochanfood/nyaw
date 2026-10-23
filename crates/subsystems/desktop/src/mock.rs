//! OS の取得・注入を使わず、上位層から本番と同じ受け渡しを検証する。
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
}

pub fn stream() -> (Stream, Controller) {
    stream_of(None)
}

/// ウィンドウ `window` を映している配信（[`Stream::window`]）。
pub fn window_stream(window: u64) -> (Stream, Controller) {
    stream_of(Some(window))
}

fn stream_of(window: Option<u64>) -> (Stream, Controller) {
    let update = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(64);
    (
        Stream {
            update: update.clone(),
            input: tx,
            stop: stop.clone(),
            window,
        },
        Controller {
            update,
            input: rx,
            stop,
        },
    )
}

impl Controller {
    pub fn publish(&self, update: Update) {
        *self.update.lock().unwrap() = Some(update);
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
    /// 前に出すよう頼まれた回数（[`Stream::focus`]）。
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
}
