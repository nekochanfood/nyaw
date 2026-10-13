//! Playwright 風の E2E テスト基盤。テストは「どの要素を・どう操作するか」だけを書き、
//! プロファイル名や座標計算は書かない。
//!
//! ランタイムは対等な「手段」なので、キットもランタイムごとに対等に用意する:
//!
//! - [`NoVrKit`]: novr。パネルはウィンドウで、OS のマウス入力をネイティブに受ける。
//! - [`VrKit`]: VR（モック）。レーザー越しに操作し、描画は VR パネルへ送られる。
//!
//! 基本操作（[`Driver`]）はどのランタイムでもネイティブに行えなければならないので、基本操作の
//! シナリオは1回だけ書いて [`on_every_runtime!`] で全ランタイムで実行する。VR 固有の機能
//! （手・機種・パネル移動など）は [`VrKit`] の固有メソッドで、UI のロジックだけを見たいものは
//! [`NoVrKit`] だけで検証する。
//!
//! ```ignore
//! fn click_counts<D: Driver<Home>>(kit: &mut D) {
//!     kit.click("click-me");
//!     assert_eq!(kit.read(|h| h.clicks), 1);
//! }
//! on_every_runtime!(click_counts, |_, _| Home::default());
//! ```
//!
//! 要素は GPUI の `.debug_selector(|| "名前".into())` で指定する。

pub mod novr;
pub mod raster;
pub mod vr_kit;

pub use input::Action;
pub use novr::NoVrKit;
pub use vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
pub use vr_kit::{DEFAULT_PROFILE, VrKit};

/// どのランタイムでもネイティブに行える基本操作。
pub trait Driver<V> {
    /// 要素の中心を指す（novr はマウス移動、VR はレーザー）。
    fn hover(&mut self, selector: &'static str) -> &mut Self;
    /// 要素の中心をクリックする（novr は左クリック、VR は選択ボタン）。
    fn click(&mut self, selector: &'static str) -> &mut Self;
    /// ビューの状態を読む。
    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R;
    /// ユーザーに見えている要素の中心の色（0xRRGGBB）。novr は画面、VR はパネルのテクスチャ。
    fn color_at(&mut self, selector: &'static str) -> u32;
}

/// 基本操作のシナリオ `scenario(&mut impl Driver<V>)` を、全ランタイムのキットで実行するテストを作る。
/// `scenario::novr` / `scenario::vr` という名前のテストになる。
#[macro_export]
macro_rules! on_every_runtime {
    ($scenario:ident, $build:expr) => {
        mod $scenario {
            // `$build` は呼び出し側のスコープの名前（ビューの型など）を使うので取り込む
            #[allow(unused_imports)]
            use super::*;

            #[gpui::test]
            fn novr(cx: &mut gpui::TestAppContext) {
                super::$scenario(&mut $crate::NoVrKit::new(cx, $build));
            }

            #[gpui::test]
            fn vr(cx: &mut gpui::TestAppContext) {
                super::$scenario(&mut $crate::VrKit::new(cx, $build));
            }
        }
    };
}
