//! 全体設定: UI スケール・手ブレ抑制・レーザー長・カーソルサイズ・ゲーム入力遮断。

use super::*;

impl Overlay {
    /// 手ブレ抑制の強さ（0 = 抑制なし 〜 [`POINTER_SMOOTHING_MAX`]）。
    pub fn set_pointer_smoothing(&mut self, smoothing: f32) {
        self.smoothing = Some(smoothing.clamp(0.0, POINTER_SMOOTHING_MAX));
    }

    /// 現在の手ブレ抑制の強さ（未設定なら既定値 [`POINTER_SMOOTHING`]）。
    pub fn pointer_smoothing(&self) -> f32 {
        self.smoothing.unwrap_or(POINTER_SMOOTHING)
    }

    /// UI スケール: 全パネルの VR 上のサイズに掛ける倍率（等倍 = 1。非数・0 以下は等倍）。解像度は不変のため
    /// レイアウトは変わらない。[`Self::add_panel`]・[`Self::fit_height`]・[`Self::set_grab_anchor`] に渡す長さは等倍設計で
    /// 記述し、この倍率が適用される（[`Panel::size`] は適用後の実寸）。変更時は既存パネルも中心を維持したまま
    /// 同率でサイズ変更され、親から見た子の位置も同率で離れる（グループは相似を維持）。詳細は AGENT.md の「UI スケール」。
    pub fn set_scale(&mut self, vr: &mut dyn VrBackend, scale: f32) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let ratio = scale / self.scale();
        self.scale = Some(scale);
        if ratio == 1.0 {
            return;
        }
        for &id in &self.present {
            let p = &mut self.panels[id];
            p.size *= ratio;
            p.nav_bar_size *= ratio;
            p.offset.position *= ratio;
            if let Some(anchor) = &mut p.grab_anchor {
                // グラブ基準を持つパネルは、中心ではなく基準（ダッシュボードならスクリーンの立ち位置）を固定してサイズ変更する。
                let fixed = p.pose * *anchor;
                anchor.position *= ratio;
                if p.parent.is_none() && p.nav_bar_of.is_none() {
                    p.pose = fixed * anchor.inverse();
                }
            }
            match p.pointer {
                // レーザーの太さは生成時に固定。カーソル・ツールチップの幅は毎 tick `size` から算出。
                Some((_, Pointer::Laser)) => p.surface.set_height(vr, p.size.y),
                Some(_) => {}
                None => p.surface.set_width(vr, p.displayed_size().x),
            }
        }
        // グラブ中なら基準から見たパネル位置も同率で変更（リリース時のジャンプを防止）。
        for h in self.hands.values_mut() {
            if let Some((_, _, _, pivot, _)) = &mut h.dragging {
                pivot.position *= ratio;
            }
        }
    }

    /// 現在の UI スケール（未設定なら等倍）。
    pub fn scale(&self) -> f32 {
        self.scale.unwrap_or(1.0)
    }

    /// 暗転中（ダッシュボード表示中）にゲームへの入力を遮断するか。次の tick から有効。
    pub fn set_block_input(&mut self, block: bool) {
        self.pass_input = !block;
    }

    pub fn block_input(&self) -> bool {
        !self.pass_input
    }

    /// 手ブレ抑制緩和角度（度。[`POINTER_RELEASE_DEG_MIN`] 〜 [`POINTER_RELEASE_DEG_MAX`]）。
    pub fn set_pointer_release(&mut self, deg: f32) {
        self.release_deg = Some(deg.clamp(POINTER_RELEASE_DEG_MIN, POINTER_RELEASE_DEG_MAX));
    }

    /// 現在の手ブレ抑制緩和角度（度。未設定なら既定値 [`POINTER_RELEASE_DEG`]）。
    pub fn pointer_release(&self) -> f32 {
        self.release_deg.unwrap_or(POINTER_RELEASE_DEG)
    }

    /// レーザーの可視長（m。[`LASER_LENGTH_MIN`] 〜 [`LASER_MAX`]）。
    pub fn set_laser_length(&mut self, length: f32) {
        self.laser_length = Some(length.clamp(LASER_LENGTH_MIN, LASER_MAX));
    }

    /// 現在のレーザー可視長（未設定なら既定値 [`LASER_LENGTH`]）。
    pub fn laser_length(&self) -> f32 {
        self.laser_length.unwrap_or(LASER_LENGTH)
    }

    /// カーソル（[`Pointer::Cursor`]）のサイズ倍率（等倍 = 1。非数・0 以下は等倍）。次の tick から有効。
    pub fn set_cursor_scale(&mut self, scale: f32) {
        self.cursor_scale = Some(scale).filter(|s| s.is_finite() && *s > 0.0);
    }

    /// 現在のカーソルサイズ倍率（未設定なら等倍）。
    pub fn cursor_scale(&self) -> f32 {
        self.cursor_scale.unwrap_or(1.0)
    }
}

#[cfg(test)]
mod tests;
