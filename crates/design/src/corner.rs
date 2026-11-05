//! 角丸の形。UI 全域の角丸をスーパーエリプス（CSS `superellipse(k)`）にする。
//! 呼び出し側は `.rounded(..)` だけを書けばよい。形はここで一括して決まる（個別に変えるときだけ `.corner_superellipse(k)`、円に戻すときは `.circular_corners()`）。

/// スーパーエリプスの強さ k。1 が円、2 がスクワークル、大きいほど四角に近い。
pub const SUPERELLIPSE: f32 = 1.2;

/// 全域の切り替え。`false` なら従来どおりの円の角丸（k = 1）に戻る。
pub const ENABLED: bool = true;

/// 角丸の既定の形を GPUI に登録する。ウィンドウを作る前に一度呼ぶ。
pub fn install() {
    gpui::set_default_corner_superellipse(if ENABLED { SUPERELLIPSE } else { 1.0 });
}
