//! ニャプリ（nyapp）の SDK。
//!
//! ニャプリは `wasm32-unknown-unknown` 向けの cdylib で、[`App`] を実装した型を [`export!`] で書き出す。
//! ホスト（`crates/nyapp`）は状態が変わるたびに [`App::view`] で画面の木（[`Node`]）を受け取って描き、
//! 押された・指されたなどを [`App::event`] で返す。
//!
//! ```ignore
//! use nyapp_sdk::*;
//!
//! #[derive(Default)]
//! struct Counter(u32);
//!
//! impl App for Counter {
//!     fn new() -> Self { Self::default() }
//!     fn view(&mut self) -> Node {
//!         column([text(self.0.to_string()).size(Size::Display), button("add", "+1")])
//!     }
//!     fn event(&mut self, event: Event) {
//!         if let Event::Click { id } = event && id == "add" { self.0 += 1 }
//!     }
//! }
//!
//! export!(Counter);
//! ```
//!
//! # ABI（[`API`] = 1）
//!
//! ニャプリが書き出すもの（[`export!`] が作る）:
//! - `memory`
//! - `nyapp_alloc(len: i32) -> i32`: ホストが書き込む `len` バイトの領域を確保する
//! - `nyapp_init()`: 最初に 1 回
//! - `nyapp_view() -> i64`: 画面の木の JSON の場所（上位 32 ビットが位置、下位が長さ）
//! - `nyapp_event(ptr: i32, len: i32)`: `nyapp_alloc` の領域に書いた [`Event`] の JSON（所有権はニャプリへ移る）
//!
//! ホストが用意するもの（import モジュール `nyaw`）は [`host`] を参照。

pub mod host;
mod ui;

use serde::{Deserialize, Serialize};
pub use ui::*;

/// この SDK が話す ABI の版。マニフェストの `api` と同じ値を書く。
pub const API: u32 = 1;

/// ホスト関数が返す失敗の値（成功は 0 以上）。
pub mod code {
    /// 権限が無い。
    pub const DENIED: i32 = -1;
    /// 値が無い。
    pub const NONE: i32 = -2;
    /// 上限を超える。
    pub const QUOTA: i32 = -3;
    /// 引数がおかしい。
    pub const INVALID: i32 = -4;
}

/// ホストから届く出来事。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// ボタン `id` が押された。
    Click { id: String },
    /// スイッチ `id` が押され、`on` になった。
    Toggle { id: String, on: bool },
    /// ポインタがパネルの上を動いた。座標は面 `id`（[`Node::Canvas`]）の左上を原点とする px（面の外なら範囲外の値）。
    Pointer { id: String, x: f32, y: f32 },
    /// [`host::set_ticker`] で頼んだ周期が来た。
    Tick,
}

/// コントローラを持つ手。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    Left,
    Right,
}

/// コントローラ 1 本の入力（[`host::controllers`]）。レーザーがこのパネルを指している手のものだけが届く。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub hand: Hand,
    /// トリガー（UI のクリック・押下）を押している。
    pub select: bool,
    /// トリガーの引き具合（0〜1）。
    pub trigger: f32,
    pub grab: bool,
    pub menu: bool,
    /// 左手の下のボタン。右手では常に `false`。
    pub shortcut: bool,
    /// スティック・トラックパッドのスクロール（x, y）。
    pub scroll: (f32, f32),
}

/// 進行中の作業の知らせ（ライブアクティビティ）。ダッシュボードのバーのクイック設定と再生中の間に出る
/// （iPhone の Dynamic Island のような進み具合の表示）。[`host::set_activity`] で出し、[`host::clear_activity`] で消す。
///
/// ニャプリを閉じても残り、nyaw が時刻から数え続ける（ニャプリが居なくても表示を進められるよう、残り時間ではなく
/// 終わる時刻・始めた時刻で渡す）。押すとニャプリが開く。ニャプリごとに 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Activity {
    /// `ends_at_ms`（UNIX 時間の ms）に終わる、長さ `total_ms` のカウントダウン。終わると nyaw が知らせる
    /// （ニャプリを閉じていれば、権限 `audio` があれば手首から音を鳴らす）。
    Countdown {
        title: String,
        ends_at_ms: i64,
        total_ms: i64,
    },
    /// `started_at_ms` から数えるストップウォッチ。
    Stopwatch { title: String, started_at_ms: i64 },
    /// 文字と、あれば進み具合（0〜1）。止めたタイマーなど、時刻で進まないもの。
    Progress {
        title: String,
        text: String,
        #[serde(default)]
        progress: Option<f32>,
    },
}

impl Activity {
    pub fn title(&self) -> &str {
        match self {
            Activity::Countdown { title, .. }
            | Activity::Stopwatch { title, .. }
            | Activity::Progress { title, .. } => title,
        }
    }
}

/// ニャプリ本体。
pub trait App {
    /// 起動したとき（`nyapp_init`）に 1 回作る。
    fn new() -> Self;
    /// 今の画面。起動直後と、[`Self::event`] のたびに呼ばれる。
    fn view(&mut self) -> Node;
    fn event(&mut self, event: Event);
}

/// [`App`] を実装した型を、ホストが呼ぶ関数として書き出す。クレートに 1 回だけ書く。
#[macro_export]
macro_rules! export {
    ($app:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            ::std::thread_local! {
                static APP: ::std::cell::RefCell<::core::option::Option<$app>> =
                    const { ::std::cell::RefCell::new(::core::option::Option::None) };
                static OUT: ::std::cell::RefCell<::std::vec::Vec<u8>> =
                    const { ::std::cell::RefCell::new(::std::vec::Vec::new()) };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn nyapp_alloc(len: i32) -> i32 {
                $crate::__alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn nyapp_init() {
                let app = <$app as $crate::App>::new();
                APP.with_borrow_mut(|a| *a = ::core::option::Option::Some(app));
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn nyapp_view() -> i64 {
                APP.with_borrow_mut(|a| {
                    let app = a.as_mut().expect("nyapp_init is called first");
                    OUT.with_borrow_mut(|out| $crate::__view(<$app as $crate::App>::view(app), out))
                })
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn nyapp_event(ptr: i32, len: i32) {
                if let ::core::option::Option::Some(event) = unsafe { $crate::__event(ptr, len) } {
                    APP.with_borrow_mut(|a| {
                        if let ::core::option::Option::Some(app) = a.as_mut() {
                            <$app as $crate::App>::event(app, event)
                        }
                    });
                }
            }
        };
    };
}

#[doc(hidden)]
pub fn __alloc(len: i32) -> i32 {
    let mut buf = Vec::<u8>::with_capacity(len.max(0) as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as usize as i32
}

/// 画面の木を `out` に JSON で書き、その場所を返す（`out` は次に呼ばれるまで残す）。
#[doc(hidden)]
pub fn __view(node: Node, out: &mut Vec<u8>) -> i64 {
    *out = serde_json::to_vec(&node).unwrap_or_default();
    ((out.as_ptr() as usize as u32 as i64) << 32) | out.len() as u32 as i64
}

/// `__alloc` で確保した領域の JSON を読み、領域を解放する。
///
/// # Safety
/// `ptr` は `__alloc(len)` が返した、まだ解放していない領域でなければならない。
#[doc(hidden)]
pub unsafe fn __event(ptr: i32, len: i32) -> Option<Event> {
    let len = len.max(0) as usize;
    let bytes = unsafe { Vec::from_raw_parts(ptr as usize as *mut u8, len, len) };
    match serde_json::from_slice(&bytes) {
        Ok(e) => Some(e),
        Err(e) => {
            host::log(host::Level::Warn, &format!("unreadable event: {e}"));
            None
        }
    }
}
