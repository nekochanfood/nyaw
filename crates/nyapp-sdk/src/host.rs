//! ホスト関数（import モジュール `nyaw`）。ニャプリが外の世界に触れる手段はこれだけで、ファイル・ネットワーク・
//! 環境変数・ほかのアプリには届かない。権限の要るもの（`audio` / `media`）は、マニフェストの `permissions` に
//! 書いていなければ [`Error::Denied`] になる。
//!
//! 文字列を受け取る関数は「バッファに書けるだけ書き、本当の長さを返す」形（足りなければ広げて呼び直す）。

use crate::code;

/// ホスト関数の失敗。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// マニフェストに権限を書いていない。
    Denied,
    /// 保存領域の上限を超える。
    Quota,
    /// 引数がおかしい（空のキーなど）。
    Invalid,
    Other(i32),
}

impl Error {
    fn from_code(c: i32) -> Self {
        match c {
            code::DENIED => Error::Denied,
            code::QUOTA => Error::Quota,
            code::INVALID => Error::Invalid,
            c => Error::Other(c),
        }
    }
}

fn check(c: i32) -> Result<(), Error> {
    if c < 0 {
        Err(Error::from_code(c))
    } else {
        Ok(())
    }
}

/// ログのレベル（ホストのロガーの `target` はニャプリの ID になる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

pub fn log(level: Level, message: &str) {
    unsafe { sys::log(level as i32, message.as_ptr(), message.len() as i32) }
}

/// 今の時刻（UNIX 時間のミリ秒）。
pub fn now_ms() -> i64 {
    unsafe { sys::now_ms() }
}

/// 地方時と UTC の差（秒。日本なら 32400）。
pub fn utc_offset() -> i32 {
    unsafe { sys::utc_offset() }
}

pub fn random() -> u64 {
    unsafe { sys::random() as u64 }
}

/// nyaw の画面の言語（`en` / `ja`）。
pub fn language() -> String {
    read_string(|buf, cap| unsafe { sys::language(buf, cap) }).unwrap_or_default()
}

/// `ms` ごとに [`crate::Event::Tick`] を送ってもらう。0 で止める。
pub fn set_ticker(ms: u32) {
    unsafe { sys::set_ticker(ms as i32) }
}

/// マニフェストに `permission` を書いたか。
pub fn has_permission(permission: &str) -> bool {
    unsafe { sys::has_permission(permission.as_ptr(), permission.len() as i32) == 1 }
}

/// このニャプリだけの保存領域から読む（次の起動にも残る）。
pub fn storage_get(key: &str) -> Option<String> {
    read_string(|buf, cap| unsafe { sys::storage_get(key.as_ptr(), key.len() as i32, buf, cap) })
        .ok()
}

/// 保存領域に書く。キーと値の合計がホストの上限を超えると [`Error::Quota`]。
pub fn storage_set(key: &str, value: &str) -> Result<(), Error> {
    check(unsafe {
        sys::storage_set(
            key.as_ptr(),
            key.len() as i32,
            value.as_ptr(),
            value.len() as i32,
        )
    })
}

pub fn storage_remove(key: &str) -> Result<(), Error> {
    check(unsafe { sys::storage_remove(key.as_ptr(), key.len() as i32) })
}

/// 知らせの音をパネルから鳴らす（権限 `audio`）。
pub fn beep() -> Result<(), Error> {
    check(unsafe { sys::beep() })
}

/// 再生中のメディアの「タイトル - アーティスト」（権限 `media`）。何も再生していなければ `None`。
pub fn now_playing() -> Result<Option<String>, Error> {
    match read_string(|buf, cap| unsafe { sys::now_playing(buf, cap) }) {
        Ok(s) => Ok(Some(s)),
        Err(code::NONE) => Ok(None),
        Err(c) => Err(Error::from_code(c)),
    }
}

/// 進行中の作業の知らせ（[`crate::Activity`]）を出す・置き換える。ニャプリを閉じても残る。
pub fn set_activity(activity: &crate::Activity) -> Result<(), Error> {
    let json = serde_json::to_vec(activity).map_err(|_| Error::Invalid)?;
    check(unsafe { sys::set_activity(json.as_ptr(), json.len() as i32) })
}

/// 進行中の作業の知らせを消す。
pub fn clear_activity() {
    unsafe { sys::clear_activity() }
}

fn read_string(call: impl Fn(*mut u8, i32) -> i32) -> Result<String, i32> {
    let mut buf = vec![0u8; 256];
    loop {
        let n = call(buf.as_mut_ptr(), buf.len() as i32);
        if n < 0 {
            return Err(n);
        }
        let n = n as usize;
        if n <= buf.len() {
            buf.truncate(n);
            return Ok(String::from_utf8_lossy(&buf).into_owned());
        }
        buf.resize(n, 0);
    }
}

#[cfg(target_arch = "wasm32")]
mod sys {
    #[link(wasm_import_module = "nyaw")]
    unsafe extern "C" {
        pub fn log(level: i32, ptr: *const u8, len: i32);
        pub fn now_ms() -> i64;
        pub fn utc_offset() -> i32;
        pub fn random() -> i64;
        pub fn language(buf: *mut u8, cap: i32) -> i32;
        pub fn set_ticker(ms: i32);
        pub fn has_permission(ptr: *const u8, len: i32) -> i32;
        pub fn storage_get(key: *const u8, key_len: i32, buf: *mut u8, cap: i32) -> i32;
        pub fn storage_set(key: *const u8, key_len: i32, value: *const u8, value_len: i32) -> i32;
        pub fn storage_remove(key: *const u8, key_len: i32) -> i32;
        pub fn beep() -> i32;
        pub fn now_playing(buf: *mut u8, cap: i32) -> i32;
        pub fn set_activity(ptr: *const u8, len: i32) -> i32;
        pub fn clear_activity();
    }
}

/// wasm の外（ホストのビルド・ニャプリの単体テスト）では、外の世界に触れないものとして振る舞う。
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::missing_safety_doc)]
mod sys {
    use crate::code;

    pub unsafe fn log(_: i32, _: *const u8, _: i32) {}
    pub unsafe fn now_ms() -> i64 {
        0
    }
    pub unsafe fn utc_offset() -> i32 {
        0
    }
    pub unsafe fn random() -> i64 {
        0
    }
    pub unsafe fn language(_: *mut u8, _: i32) -> i32 {
        code::NONE
    }
    pub unsafe fn set_ticker(_: i32) {}
    pub unsafe fn has_permission(_: *const u8, _: i32) -> i32 {
        0
    }
    pub unsafe fn storage_get(_: *const u8, _: i32, _: *mut u8, _: i32) -> i32 {
        code::NONE
    }
    pub unsafe fn storage_set(_: *const u8, _: i32, _: *const u8, _: i32) -> i32 {
        code::DENIED
    }
    pub unsafe fn storage_remove(_: *const u8, _: i32) -> i32 {
        code::DENIED
    }
    pub unsafe fn beep() -> i32 {
        code::DENIED
    }
    pub unsafe fn now_playing(_: *mut u8, _: i32) -> i32 {
        code::DENIED
    }
    pub unsafe fn set_activity(_: *const u8, _: i32) -> i32 {
        code::DENIED
    }
    pub unsafe fn clear_activity() {}
}
