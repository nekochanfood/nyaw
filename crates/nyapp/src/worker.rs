//! 外部ニャプリ専用プロセスとの通信。権限境界を越える値は全て長さを確かめる。

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak, mpsc};
use std::time::{Duration, Instant};

use nyapp_sdk::{Activity, Event, Node};
use serde::{Deserialize, Serialize};

use crate::{Error, Sandbox, installed, package, sandbox::STORAGE_QUOTA};

mod platform;

const MAX_PACKAGE: usize = 64 << 20;
const MAX_STORAGE_FILE: usize = STORAGE_QUOTA * 2;
const MAX_MESSAGE: usize = 6 << 20;
const START_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
static STORAGE_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Start {
    id: String,
    language: String,
    utc_offset: i32,
}

#[derive(Serialize, Deserialize)]
struct Call {
    event: Event,
    language: String,
    now_playing: Option<String>,
    utc_offset: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Step {
    pub view: Node,
    pub ticker: Option<u32>,
    pub beeps: u32,
    pub activity: Option<Option<Activity>>,
    pub storage: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Ok(Step),
    Err(String),
}

/// このハンドルを落とすと、ワーカーを必ず終了させる。
pub struct RemoteSandbox {
    tx: mpsc::SyncSender<Call>,
    guard: Arc<platform::Guard>,
}

impl RemoteSandbox {
    pub fn start(
        path: &Path,
        id: &str,
        language: &'static str,
    ) -> Result<(Self, Step, mpsc::Receiver<Result<Step, Error>>), Error> {
        Self::start_with_executable(&std::env::current_exe()?, path, id, language)
    }

    /// 実プロセスを使う結合テスト用。`executable` は信頼できる nyaw バイナリを指定する。
    #[doc(hidden)]
    pub fn start_with_executable(
        executable: &Path,
        path: &Path,
        id: &str,
        language: &'static str,
    ) -> Result<(Self, Step, mpsc::Receiver<Result<Step, Error>>), Error> {
        if !crate::manifest::valid_id(id) || crate::system::is_system(id) {
            return Err(Error::new("invalid external nyapp id"));
        }
        let file = fs::File::open(path)?;
        if file.metadata()?.len() > MAX_PACKAGE as u64 {
            return Err(Error::new("the nyapp package is too large"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_PACKAGE as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_PACKAGE {
            return Err(Error::new("the nyapp package is too large"));
        }
        let storage_path = installed::appdata(id).map(|p| p.join("storage.json"));
        let storage = storage_path
            .as_ref()
            .and_then(|p| read_storage(p).ok())
            .unwrap_or_default();
        let (mut input, mut output, guard) = platform::spawn(executable, id)?;
        let guard = Arc::new(guard);
        watchdog(Arc::downgrade(&guard));
        guard.deadline(Some(Instant::now() + START_TIMEOUT));
        write_json(
            &mut input,
            &Start {
                id: id.into(),
                language: language.into(),
                utc_offset: chrono::Local::now().offset().local_minus_utc(),
            },
        )?;
        write_frame(&mut input, &bytes)?;
        write_frame(&mut input, &storage)?;
        input.flush()?;
        let initial = read_reply(&mut output)?;
        guard.deadline(None);
        validate_step(&initial)?;
        persist(&storage_path, &initial)?;

        let (tx, requests) = mpsc::sync_channel::<Call>(32);
        let (results, rx) = mpsc::channel();
        let weak = Arc::downgrade(&guard);
        std::thread::spawn(move || {
            while let Ok(call) = requests.recv() {
                let Some(g) = weak.upgrade() else { break };
                g.deadline(Some(Instant::now() + CALL_TIMEOUT));
                drop(g);
                let result = write_json(&mut input, &call)
                    .and_then(|_| input.flush().map_err(Error::from))
                    .and_then(|_| read_reply(&mut output))
                    .and_then(|step| {
                        validate_step(&step)?;
                        persist(&storage_path, &step)?;
                        Ok(step)
                    });
                if let Some(g) = weak.upgrade() {
                    g.deadline(None);
                }
                let failed = result.is_err();
                if results.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok((Self { tx, guard }, initial, rx))
    }

    pub fn event(
        &self,
        event: Event,
        language: &'static str,
        now_playing: Option<String>,
    ) -> Result<(), Error> {
        self.tx
            .try_send(Call {
                event,
                language: language.into(),
                now_playing,
                utc_offset: chrono::Local::now().offset().local_minus_utc(),
            })
            .map_err(|_| Error::new("the nyapp worker is not responding"))
    }

    pub fn is_alive(&self) -> bool {
        self.guard.is_alive()
    }
}

fn watchdog(guard: Weak<platform::Guard>) {
    std::thread::spawn(move || {
        while let Some(g) = guard.upgrade() {
            if g.expired() {
                g.kill();
                break;
            }
            drop(g);
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

fn persist(path: &Option<PathBuf>, step: &Step) -> Result<(), Error> {
    let (Some(path), Some(bytes)) = (path, &step.storage) else {
        return Ok(());
    };
    if bytes.len() > MAX_STORAGE_FILE {
        return Err(Error::new("the nyapp storage is too large"));
    }
    let values: std::collections::BTreeMap<String, String> = serde_json::from_slice(bytes)?;
    if values.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() > STORAGE_QUOTA {
        return Err(Error::new("the nyapp storage quota was exceeded"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("invalid nyapp storage path"))?;
    let base = parent
        .parent()
        .ok_or_else(|| Error::new("invalid nyapp storage path"))?;
    fs::create_dir_all(base)?;
    reject_reparse(base)?;
    if parent.exists() {
        reject_reparse(parent)?;
    } else {
        fs::create_dir(parent)?;
    }
    reject_reparse(parent)?;
    if path.exists() {
        reject_reparse(path)?;
    }
    let suffix = STORAGE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let partial = path.with_extension(format!("{}.{}.partial", std::process::id(), suffix));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = output.write_all(bytes).and_then(|_| output.sync_all());
    drop(output);
    let result = result.and_then(|_| fs::rename(&partial, path));
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result?;
    Ok(())
}

fn read_storage(path: &Path) -> Result<Vec<u8>, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("invalid nyapp storage path"))?;
    let base = parent
        .parent()
        .ok_or_else(|| Error::new("invalid nyapp storage path"))?;
    reject_reparse(base)?;
    reject_reparse(parent)?;
    reject_reparse(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let input = options.open(path)?;
    let mut bytes = Vec::new();
    input
        .take(MAX_STORAGE_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_STORAGE_FILE {
        return Err(Error::new("the nyapp storage is too large"));
    }
    Ok(bytes)
}

fn reject_reparse(path: &Path) -> Result<(), Error> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = meta.file_type().is_symlink();
    if linked {
        return Err(Error::new("nyapp storage directory must not be a symlink"));
    }
    Ok(())
}

fn validate_step(step: &Step) -> Result<(), Error> {
    let mut stack = vec![(&step.view, 0usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = stack.pop() {
        count += 1;
        if count > 10_000 || depth > 64 {
            return Err(Error::new("the nyapp view is too complex"));
        }
        let children = match node {
            Node::Column(s) | Node::Row(s) | Node::Card(s) => Some(&s.children),
            Node::Grid { children, .. } => Some(children),
            _ => None,
        };
        if let Some(children) = children {
            stack.extend(children.iter().map(|n| (n, depth + 1)));
        }
    }
    if step.beeps > 1_000 {
        return Err(Error::new("too many nyapp beep requests"));
    }
    Ok(())
}

fn read_reply(input: &mut impl Read) -> Result<Step, Error> {
    match read_json::<Reply>(input, MAX_MESSAGE)? {
        Reply::Ok(step) => Ok(step),
        Reply::Err(e) => Err(Error::new(e)),
    }
}

fn write_json<T: Serialize>(out: &mut impl Write, value: &T) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(Error::new("nyapp message is too large"));
    }
    write_frame(out, &bytes)
}

fn read_json<T: for<'a> Deserialize<'a>>(input: &mut impl Read, max: usize) -> Result<T, Error> {
    Ok(serde_json::from_slice(&read_frame(input, max)?)?)
}

fn write_frame(out: &mut impl Write, bytes: &[u8]) -> Result<(), Error> {
    let len = u32::try_from(bytes.len()).map_err(|_| Error::new("nyapp message is too large"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(bytes)?;
    Ok(())
}

fn read_frame(input: &mut impl Read, max: usize) -> Result<Vec<u8>, Error> {
    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len > max {
        return Err(Error::new("nyapp message is too large"));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// `--nyapp-worker` の入口。入力を読む前に OS の制限を掛ける。
pub fn main() -> Result<(), Error> {
    platform::confine()?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let start: Start = read_json(&mut input, MAX_MESSAGE)?;
    if !crate::manifest::valid_id(&start.id) {
        return Err(Error::new("invalid nyapp id"));
    }
    let bytes = read_frame(&mut input, MAX_PACKAGE)?;
    let storage = read_frame(&mut input, MAX_STORAGE_FILE)?;
    let result = (|| {
        let (manifest, wasm) = package::read_bytes(&bytes)?;
        if manifest.id != start.id || crate::system::is_system(&manifest.id) {
            return Err(Error::new("nyapp identity mismatch"));
        }
        let (sandbox, view) = Sandbox::load_with_storage(
            &manifest,
            &wasm,
            None,
            Some(&storage),
            language(&start.language),
            start.utc_offset,
        )?;
        Ok((sandbox, view))
    })();
    let (mut sandbox, first) = match result {
        Ok(v) => v,
        Err(e) => {
            write_json(&mut output, &Reply::Err(e.to_string()))?;
            output.flush()?;
            return Ok(());
        }
    };
    write_json(&mut output, &Reply::Ok(step(&mut sandbox, first)))?;
    output.flush()?;
    loop {
        let call: Call = match read_json(&mut input, MAX_MESSAGE) {
            Ok(call) => call,
            Err(_) => return Ok(()),
        };
        let host = sandbox.host();
        host.language = language(&call.language);
        host.now_playing = call.now_playing;
        host.utc_offset = call.utc_offset;
        let reply = match sandbox.event(&call.event) {
            Ok(view) => Reply::Ok(step(&mut sandbox, view)),
            Err(e) => Reply::Err(e.to_string()),
        };
        write_json(&mut output, &reply)?;
        output.flush()?;
        if matches!(reply, Reply::Err(_)) {
            return Ok(());
        }
    }
}

fn language(value: &str) -> &'static str {
    if value == "ja" { "ja" } else { "en" }
}

pub(crate) fn step(sandbox: &mut Sandbox, view: Node) -> Step {
    let host = sandbox.host();
    Step {
        view,
        ticker: host.ticker.take(),
        beeps: std::mem::take(&mut host.beeps),
        activity: host.activity.take(),
        storage: host.take_storage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_reject_oversized_input() {
        let header = (7u32).to_le_bytes();
        let mut input = header.as_slice();
        assert!(read_frame(&mut input, 6).is_err());
    }

    #[test]
    fn view_depth_is_bounded() {
        let mut view = Node::Spacer;
        for _ in 0..65 {
            view = Node::Column(nyapp_sdk::Stack {
                children: vec![view],
                ..Default::default()
            });
        }
        assert!(
            validate_step(&Step {
                view,
                ticker: None,
                beeps: 0,
                activity: None,
                storage: None,
            })
            .is_err()
        );
    }

    #[test]
    fn storage_snapshots_replace_the_previous_file() {
        let root = std::env::temp_dir().join(format!(
            "nyapp-worker-storage-{}-{}",
            std::process::id(),
            STORAGE_TEMP_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let path = root
            .join("appdata")
            .join("net.bettaworx.test")
            .join("storage.json");
        let mut step = Step {
            view: Node::Spacer,
            ticker: None,
            beeps: 0,
            activity: None,
            storage: Some(br#"{"one":"1"}"#.to_vec()),
        };
        persist(&Some(path.clone()), &step).unwrap();
        step.storage = Some(br#"{"one":"2"}"#.to_vec());
        persist(&Some(path.clone()), &step).unwrap();
        assert_eq!(fs::read(path).unwrap(), br#"{"one":"2"}"#);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn storage_does_not_follow_a_symlink() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "nyapp-worker-link-{}-{}",
            std::process::id(),
            STORAGE_TEMP_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let parent = root.join("appdata").join("net.bettaworx.test");
        fs::create_dir_all(&parent).unwrap();
        let secret = root.join("secret.json");
        fs::write(&secret, br#"{"secret":"private"}"#).unwrap();
        let storage = parent.join("storage.json");
        symlink(secret, &storage).unwrap();
        assert!(read_storage(&storage).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
