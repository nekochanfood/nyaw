//! 全体の調整: UI スケール・手ブレ抑え・レーザーの長さ・カーソルの大きさ・ゲーム入力の遮断。

use super::*;

impl Overlay {
    /// 手ブレ抑えの強さ（0 = 抑えない 〜 [`POINTER_SMOOTHING_MAX`]）。
    pub fn set_pointer_smoothing(&mut self, smoothing: f32) {
        self.smoothing = Some(smoothing.clamp(0.0, POINTER_SMOOTHING_MAX));
    }

    /// 今の手ブレ抑えの強さ（設定していなければ既定値 [`POINTER_SMOOTHING`]）。
    pub fn pointer_smoothing(&self) -> f32 {
        self.smoothing.unwrap_or(POINTER_SMOOTHING)
    }

    /// UI スケール: 全パネルの VR 上の大きさに掛ける倍率（等倍 = 1。数でない・0 以下は等倍）。解像度は変えないので、
    /// 中の組み方は変わらない。[`Self::add_panel`]・[`Self::fit_height`]・[`Self::set_grab_anchor`] に渡す長さは等倍の
    /// つもりで書き、ここの倍率が掛かる（[`Panel::size`] は掛けた後の実寸）。変えると、開いているパネルも中心を動かさずに
    /// 同じ割合で大きさが変わり、親から見た子の位置も同じ割合で離れる（組は相似のまま）。詳しくは AGENT.md の「UI スケール」。
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
                // 掴む基準を持つパネルは、中心ではなく基準（ダッシュボードなら画面が立つ所）を動かさずに大きさを変える。
                let fixed = p.pose * *anchor;
                anchor.position *= ratio;
                if p.parent.is_none() && p.nav_bar_of.is_none() {
                    p.pose = fixed * anchor.inverse();
                }
            }
            match p.pointer {
                // レーザーの太さは作ったときに決めてある。カーソル・ツールチップの幅は毎 tick `size` から決まる。
                Some((_, Pointer::Laser)) => p.surface.set_height(vr, p.size.y),
                Some(_) => {}
                None => p.surface.set_width(vr, p.displayed_size().x),
            }
        }
        // 掴んでいる最中なら、基準から見たパネルの位置も同じ割合で変える（離したときに跳ねない）。
        for h in self.hands.values_mut() {
            if let Some((_, _, _, pivot, _)) = &mut h.dragging {
                pivot.position *= ratio;
            }
        }
    }

    /// 今の UI スケール（設定していなければ等倍）。
    pub fn scale(&self) -> f32 {
        self.scale.unwrap_or(1.0)
    }

    /// 暗転している間（ダッシュボードが出ている間）、ゲームへの入力を止めるか。次の tick から効く。
    pub fn set_block_input(&mut self, block: bool) {
        self.pass_input = !block;
    }

    pub fn block_input(&self) -> bool {
        !self.pass_input
    }

    /// 手ブレ抑えを緩める角度（度。[`POINTER_RELEASE_DEG_MIN`] 〜 [`POINTER_RELEASE_DEG_MAX`]）。
    pub fn set_pointer_release(&mut self, deg: f32) {
        self.release_deg = Some(deg.clamp(POINTER_RELEASE_DEG_MIN, POINTER_RELEASE_DEG_MAX));
    }

    /// 今の手ブレ抑えを緩める角度（度。設定していなければ既定値 [`POINTER_RELEASE_DEG`]）。
    pub fn pointer_release(&self) -> f32 {
        self.release_deg.unwrap_or(POINTER_RELEASE_DEG)
    }

    /// レーザーの見える長さ（m。[`LASER_LENGTH_MIN`] 〜 [`LASER_MAX`]）。
    pub fn set_laser_length(&mut self, length: f32) {
        self.laser_length = Some(length.clamp(LASER_LENGTH_MIN, LASER_MAX));
    }

    /// 今のレーザーの見える長さ（設定していなければ既定値 [`LASER_LENGTH`]）。
    pub fn laser_length(&self) -> f32 {
        self.laser_length.unwrap_or(LASER_LENGTH)
    }

    /// カーソル（[`Pointer::Cursor`]）の大きさに掛ける倍率（等倍 = 1。数でない・0 以下は等倍）。次の tick から効く。
    pub fn set_cursor_scale(&mut self, scale: f32) {
        self.cursor_scale = Some(scale).filter(|s| s.is_finite() && *s > 0.0);
    }

    /// 今のカーソルの大きさの倍率（設定していなければ等倍）。
    pub fn cursor_scale(&self) -> f32 {
        self.cursor_scale.unwrap_or(1.0)
    }
}

#[cfg(test)]
mod tests;
