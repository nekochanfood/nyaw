//! 角丸の形状。UI 全域の角丸をスーパーエリプス（CSS `superellipse(k)`）に設定する。
//! 呼び出し側は `.rounded(..)` のみで済む。形状はここで一括決定する（個別変更時のみ `.corner_superellipse(k)`、円に戻す場合は `.circular_corners()`）。

/// スーパーエリプスの強度 k。1 が円、2 がスクワークル、大きいほど矩形に近づく。
pub const SUPERELLIPSE: f32 = 1.2;

/// 全域の切り替えフラグ。`false` にすると従来の円の角丸（k = 1）に戻る。
pub const ENABLED: bool = true;

/// 角丸の既定形状を GPUI に登録する。ウィンドウ生成前に一度呼び出す。
pub fn install() {
    gpui::set_default_corner_superellipse(if ENABLED { SUPERELLIPSE } else { 1.0 });
}
