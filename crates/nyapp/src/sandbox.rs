//! サンドボックス: ニャプリの wasm を wasmi（インタプリタ）で動かす。
//!
//! ニャプリに見えるのは [`linker`] に並べたホスト関数（import モジュール `nyaw`）だけで、ファイル・ネットワーク・
//! 環境変数・ほかのアプリには届かない（WASI も渡さない）。呼ぶたびに燃料（命令数）を [`CALL_FUEL`] だけ渡し、
//! メモリは [`MEMORY_LIMIT`] まで。尽きる・壊れる（trap）とエラーを返し、本体は落ちない。
//! ニャプリから受け取る位置と長さはすべて範囲を確かめ、外れていれば [`code::INVALID`] を返す。

use std::collections::BTreeMap;
use std::hash::BuildHasher;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use nyapp_sdk::{Event, Node, code};
use wasmi::{
    Caller, Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

use crate::{Error, Manifest, Permission};

/// メモリの上限。
pub const MEMORY_LIMIT: usize = 64 << 20;
/// 保存領域の上限（キーと値の合計バイト数）。
pub const STORAGE_QUOTA: usize = 1 << 20;
/// 起動（初期化と最初の画面）に渡す燃料。起動は別スレッドなので多めに渡す。
const INIT_FUEL: u64 = 500_000_000;
/// 1 回の呼び出し（出来事 + 画面）に渡す燃料。メインスレッドで呼ぶので、尽きても 1 フレームの引っ掛かりで済む程度。
// ponytail: 呼び出しはメインスレッド。重いニャプリが出てきたら、ニャプリごとのスレッドで回して結果だけ受け取る。
pub const CALL_FUEL: u64 = 20_000_000;
/// 画面の木の JSON の上限。
const MAX_VIEW: usize = 4 << 20;
/// ログ 1 件の上限。
const MAX_LOG: usize = 4 << 10;

/// ホスト関数から見える、ニャプリ 1 つ分の状態。
pub struct Host {
    pub id: String,
    permissions: Vec<Permission>,
    limits: StoreLimits,
    storage: Storage,
    /// 画面の言語（`en` / `ja`）。呼ぶ前にビューが書く。
    pub language: &'static str,
    /// 再生中のメディア（「タイトル - アーティスト」）。呼ぶ前にビューが書く。
    pub now_playing: Option<String>,
    /// ニャプリが頼んだ周期（ms。0 で止める）。呼んだ後にビューが取る。
    pub ticker: Option<u32>,
    /// 鳴らしてと頼まれた回数。呼んだ後にビューが取る。
    pub beeps: u32,
    random: u64,
    /// ニャプリのメモリ（インスタンスを作った後に入れる）。ホスト関数はこれで読み書きする。
    // `Caller::get_export` はインスタンスを生のポインタで引くので、Store を動かした後に呼ぶと壊れたポインタを読む
    // （Windows でアクセス違反になった）。Store の添字で引くハンドルを持っておく。
    memory: Option<Memory>,
}

pub struct Sandbox {
    store: Store<Host>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    view: TypedFunc<(), i64>,
    event: TypedFunc<(i32, i32), ()>,
}

impl Sandbox {
    /// wasm を検証して動かし、初期化して最初の画面を返す。ファイルを読むので別スレッドで呼ぶ。
    pub fn load(
        manifest: &Manifest,
        wasm: &[u8],
        appdata: Option<PathBuf>,
        language: &'static str,
    ) -> Result<(Self, Node), Error> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm)?;
        let host = Host {
            id: manifest.id.clone(),
            permissions: manifest.permissions.clone(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT)
                .instances(1)
                .build(),
            storage: Storage::open(appdata.map(|d| d.join("storage.json"))),
            language,
            now_playing: None,
            ticker: None,
            beeps: 0,
            random: 0,
            memory: None,
        };
        let mut store = Store::new(&engine, host);
        store.limiter(|h| &mut h.limits);
        store.set_fuel(INIT_FUEL)?;
        let instance = linker(&engine)?.instantiate_and_start(&mut store, &module)?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| Error::new("the nyapp exports no memory"))?;
        store.data_mut().memory = Some(memory);
        let alloc = instance.get_typed_func(&store, "nyapp_alloc")?;
        let view = instance.get_typed_func(&store, "nyapp_view")?;
        let event = instance.get_typed_func(&store, "nyapp_event")?;
        instance
            .get_typed_func::<(), ()>(&store, "nyapp_init")?
            .call(&mut store, ())?;
        let mut sandbox = Self {
            store,
            memory,
            alloc,
            view,
            event,
        };
        let node = sandbox.read_view()?;
        Ok((sandbox, node))
    }

    /// 出来事を渡し、その後の画面を返す。
    pub fn event(&mut self, event: &Event) -> Result<Node, Error> {
        let json = serde_json::to_vec(event)?;
        self.store.set_fuel(CALL_FUEL)?;
        let ptr = self.alloc.call(&mut self.store, json.len() as i32)?;
        self.memory
            .write(&mut self.store, ptr as u32 as usize, &json)
            .map_err(|e| Error::new(format!("cannot pass the event: {e}")))?;
        self.event.call(&mut self.store, (ptr, json.len() as i32))?;
        self.read_view()
    }

    /// 画面を受け取る（燃料は呼ぶ側が渡した残り）。
    fn read_view(&mut self) -> Result<Node, Error> {
        let packed = self.view.call(&mut self.store, ())? as u64;
        let (ptr, len) = ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize);
        if len > MAX_VIEW {
            return Err(Error::new(format!(
                "the view is larger than {MAX_VIEW} bytes"
            )));
        }
        let mut bytes = vec![0; len];
        self.memory
            .read(&self.store, ptr, &mut bytes)
            .map_err(|e| Error::new(format!("cannot read the view: {e}")))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn host(&mut self) -> &mut Host {
        self.store.data_mut()
    }
}

/// ホスト関数の表（[`nyapp_sdk::host`] と同じ並び）。
fn linker(engine: &Engine) -> Result<Linker<Host>, Error> {
    let mut l = Linker::<Host>::new(engine);
    l.func_wrap(
        "nyaw",
        "log",
        |caller: Caller<'_, Host>, level: i32, ptr: i32, len: i32| {
            let Some(bytes) = read(&caller, ptr, len.min(MAX_LOG as i32)) else {
                return;
            };
            let (app, message) = (&caller.data().id, String::from_utf8_lossy(&bytes));
            match level {
                ..=0 => logger::trace!("nyapp", app = app, message = message),
                1 => logger::debug!("nyapp", app = app, message = message),
                2 => logger::info!("nyapp", app = app, message = message),
                3 => logger::warn!("nyapp", app = app, message = message),
                _ => logger::error!("nyapp", app = app, message = message),
            }
        },
    )?;
    l.func_wrap("nyaw", "now_ms", || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    })?;
    l.func_wrap("nyaw", "utc_offset", || {
        chrono::Local::now().offset().local_minus_utc()
    })?;
    // 暗号には使えない（予測されてよいもの: ゲームの乱数・ID など）。
    l.func_wrap("nyaw", "random", |mut caller: Caller<'_, Host>| {
        let host = caller.data_mut();
        host.random += 1;
        std::collections::hash_map::RandomState::new().hash_one(host.random) as i64
    })?;
    l.func_wrap(
        "nyaw",
        "language",
        |mut caller: Caller<'_, Host>, buf: i32, cap: i32| {
            let language = caller.data().language;
            write(&mut caller, buf, cap, language.as_bytes())
        },
    )?;
    l.func_wrap(
        "nyaw",
        "set_ticker",
        |mut caller: Caller<'_, Host>, ms: i32| {
            caller.data_mut().ticker = Some(ms.max(0) as u32);
        },
    )?;
    l.func_wrap(
        "nyaw",
        "has_permission",
        |caller: Caller<'_, Host>, ptr: i32, len: i32| {
            let granted = read(&caller, ptr, len)
                .and_then(|b| Permission::parse(&String::from_utf8_lossy(&b)))
                .is_some_and(|p| caller.data().permissions.contains(&p));
            granted as i32
        },
    )?;
    l.func_wrap(
        "nyaw",
        "storage_get",
        |mut caller: Caller<'_, Host>, key: i32, key_len: i32, buf: i32, cap: i32| {
            let Some(key) = read_str(&caller, key, key_len) else {
                return code::INVALID;
            };
            match caller.data().storage.values.get(&key).cloned() {
                Some(value) => write(&mut caller, buf, cap, value.as_bytes()),
                None => code::NONE,
            }
        },
    )?;
    l.func_wrap(
        "nyaw",
        "storage_set",
        |mut caller: Caller<'_, Host>, key: i32, key_len: i32, value: i32, value_len: i32| {
            let (Some(key), Some(value)) = (
                read_str(&caller, key, key_len),
                read_str(&caller, value, value_len),
            ) else {
                return code::INVALID;
            };
            caller.data_mut().storage.set(key, value)
        },
    )?;
    l.func_wrap(
        "nyaw",
        "storage_remove",
        |mut caller: Caller<'_, Host>, key: i32, key_len: i32| match read_str(&caller, key, key_len)
        {
            Some(key) => caller.data_mut().storage.remove(&key),
            None => code::INVALID,
        },
    )?;
    l.func_wrap("nyaw", "beep", |mut caller: Caller<'_, Host>| {
        let host = caller.data_mut();
        if !host.permissions.contains(&Permission::Audio) {
            return code::DENIED;
        }
        host.beeps += 1;
        0
    })?;
    l.func_wrap(
        "nyaw",
        "now_playing",
        |mut caller: Caller<'_, Host>, buf: i32, cap: i32| {
            let host = caller.data();
            if !host.permissions.contains(&Permission::Media) {
                return code::DENIED;
            }
            match host.now_playing.clone() {
                Some(title) => write(&mut caller, buf, cap, title.as_bytes()),
                None => code::NONE,
            }
        },
    )?;
    Ok(l)
}

fn memory(caller: &Caller<'_, Host>) -> Option<Memory> {
    caller.data().memory
}

/// ニャプリのメモリの `ptr..ptr+len` を写す。範囲外なら `None`。
fn read(caller: &Caller<'_, Host>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let (start, len) = (ptr as u32 as usize, usize::try_from(len).ok()?);
    memory(caller)?
        .data(caller)
        .get(start..start.checked_add(len)?)
        .map(<[u8]>::to_vec)
}

fn read_str(caller: &Caller<'_, Host>, ptr: i32, len: i32) -> Option<String> {
    String::from_utf8(read(caller, ptr, len)?).ok()
}

/// `bytes` を `buf` に `cap` バイトまで書き、本当の長さを返す（足りなければニャプリが広げて呼び直す）。
fn write(caller: &mut Caller<'_, Host>, buf: i32, cap: i32, bytes: &[u8]) -> i32 {
    let Some(memory) = memory(caller) else {
        return code::INVALID;
    };
    let (start, n) = (buf as u32 as usize, bytes.len().min(cap.max(0) as usize));
    let Some(dst) = memory
        .data_mut(caller)
        .get_mut(start..start.saturating_add(n))
    else {
        return code::INVALID;
    };
    dst.copy_from_slice(&bytes[..n]);
    bytes.len() as i32
}

/// ニャプリだけの保存領域（JSON の 1 ファイル）。書くたびに別スレッドへ送り、最新のものだけを書く。
struct Storage {
    values: BTreeMap<String, String>,
    writer: Option<mpsc::Sender<Vec<u8>>>,
}

impl Storage {
    fn open(path: Option<PathBuf>) -> Self {
        let values = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            values,
            writer: path.map(spawn_writer),
        }
    }

    fn size(&self) -> usize {
        self.values.iter().map(|(k, v)| k.len() + v.len()).sum()
    }

    fn set(&mut self, key: String, value: String) -> i32 {
        if key.is_empty() {
            return code::INVALID;
        }
        let old = self.values.get(&key).map_or(0, |v| key.len() + v.len());
        if self.size() - old + key.len() + value.len() > STORAGE_QUOTA {
            return code::QUOTA;
        }
        self.values.insert(key, value);
        self.save();
        0
    }

    fn remove(&mut self, key: &str) -> i32 {
        if self.values.remove(key).is_some() {
            self.save();
        }
        0
    }

    fn save(&self) {
        if let (Some(tx), Ok(bytes)) = (&self.writer, serde_json::to_vec(&self.values)) {
            let _ = tx.send(bytes);
        }
    }
}

/// `path` へ書くスレッド。送り手（[`Storage`]）が無くなれば終わる。
fn spawn_writer(path: PathBuf) -> mpsc::Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(mut bytes) = rx.recv() {
            // 溜まっていれば最新のものだけ書く。
            while let Ok(newer) = rx.try_recv() {
                bytes = newer;
            }
            let partial = path.with_extension("partial");
            let result = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(&partial, &bytes))
                .and_then(|_| std::fs::rename(&partial, &path));
            if let Err(e) = result {
                logger::warn!(
                    "cannot save nyapp storage",
                    path = path.display(),
                    error = e
                );
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(permissions: Vec<Permission>) -> Manifest {
        Manifest {
            id: "net.bettaworx.test".into(),
            name: "Test".into(),
            names: Default::default(),
            version: "1".into(),
            api: nyapp_sdk::API,
            permissions,
            update: None,
        }
    }

    /// SDK の ABI を手で書いた最小のニャプリ（WebAssembly のテキスト形式。テストでだけ wasmi の `wat` を有効にして読む）。
    /// 画面は固定の JSON で、出来事を受けると `event_body` を実行する。
    fn module(event_body: &str) -> String {
        let view = r#"{"type":"text","text":"hi"}"#;
        format!(
            r#"(module
                (import "nyaw" "beep" (func $beep (result i32)))
                (import "nyaw" "language" (func $language (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "{view}")
                (global $next (mut i32) (i32.const 1024))
                (func (export "nyapp_alloc") (param i32) (result i32)
                    (global.get $next)
                    (global.set $next (i32.add (global.get $next) (local.get 0))))
                (func (export "nyapp_init"))
                (func (export "nyapp_view") (result i64)
                    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const {len})))
                (func (export "nyapp_event") (param i32 i32) {event_body}))"#,
            view = view.replace('"', "\\\""),
            len = view.len(),
        )
    }

    fn load(event_body: &str, permissions: Vec<Permission>) -> Result<(Sandbox, Node), Error> {
        Sandbox::load(
            &manifest(permissions),
            module(event_body).as_bytes(),
            None,
            "ja",
        )
    }

    #[test]
    fn shows_the_view_and_gates_permissions() {
        let (mut sandbox, node) = load("(drop (call $beep))", vec![]).unwrap();
        assert_eq!(node, nyapp_sdk::text("hi"));
        sandbox.event(&Event::Tick).unwrap();
        assert_eq!(sandbox.host().beeps, 0, "権限が無ければ鳴らさない");

        let (mut sandbox, _) = load("(drop (call $beep))", vec![Permission::Audio]).unwrap();
        sandbox.event(&Event::Tick).unwrap();
        assert_eq!(sandbox.host().beeps, 1);
    }

    /// 止まらないニャプリは燃料が尽きてエラーになり、ホストは止まらない。
    #[test]
    fn runaway_code_runs_out_of_fuel() {
        let (mut sandbox, _) = load("(loop (br 0))", vec![]).unwrap();
        assert!(sandbox.event(&Event::Tick).is_err());
    }

    /// ホスト関数はニャプリのメモリに書ける（範囲外なら書かずに INVALID）。
    #[test]
    fn host_functions_write_into_guest_memory() {
        let body = "(drop (call $language (i32.const 2048) (i32.const 16)))";
        let (mut sandbox, _) = load(body, vec![]).unwrap();
        sandbox.event(&Event::Tick).unwrap();
        let body = "(drop (call $language (i32.const 70000) (i32.const 16)))";
        let (mut sandbox, _) = load(body, vec![]).unwrap();
        sandbox.event(&Event::Tick).unwrap();
    }

    /// 書ける量には上限があり、空のキーは受け付けない。
    #[test]
    fn storage_has_a_quota() {
        let mut storage = Storage::open(None);
        assert_eq!(storage.set("k".into(), "v".into()), 0);
        assert_eq!(storage.set(String::new(), "v".into()), code::INVALID);
        assert_eq!(
            storage.set("big".into(), "x".repeat(STORAGE_QUOTA)),
            code::QUOTA
        );
        assert_eq!(
            storage.set("k".into(), "w".into()),
            0,
            "置き換えは差の分だけ数える"
        );
        assert_eq!(storage.values.get("k").map(String::as_str), Some("w"));
    }
}
