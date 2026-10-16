//! OS の取得・注入を使わず、上位層から本番と同じ受け渡しを検証する。
use super::*;

pub fn source(name: &str, kind: SourceKind) -> Source {
    Source {
        name: name.into(),
        kind,
        target: 0,
    }
}

pub struct Controller {
    update: Arc<Mutex<Option<Update>>>,
    input: Receiver<Input>,
    stop: Arc<AtomicBool>,
}

pub fn stream() -> (Stream, Controller) {
    let update = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(64);
    (
        Stream {
            update: update.clone(),
            input: tx,
            stop: stop.clone(),
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
                Input::Frame => None,
            })
            .collect()
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}
