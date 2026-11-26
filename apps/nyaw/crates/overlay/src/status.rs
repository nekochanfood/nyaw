//! 状態照会。上位層が参照するパネル・HMD・コントローラ・ポインティング・グラブ・外側プレスの情報。

use super::*;

/// [`Overlay::in_view`] 用の視野半角（rad）。実機より広め（70°）に設定。
const VIEW_HALF_ANGLE: f32 = 1.221_730_5;
/// [`Overlay::gazed`] 判定の閾値。正面→パネル中心の角度（45°、眼球運動の余裕込み）・
/// 面↔頭方向の角度（65°、手首が回りきらない場合を許容）・頭からの距離（m）。腕時計注視姿勢の検出向け。
// ponytail: 頭の向き基準。視線トラッキング対応 HMD なら視線方向で絞れる。
const GAZE_ANGLE: f32 = 0.79;
const GAZE_FACING: f32 = 1.13;
const GAZE_REACH: f32 = 1.0;

impl Overlay {
    /// パネル総数。閉じたパネルを含む。PanelId は 0..len。
    pub fn len(&self) -> usize {
        self.panels.len()
    }

    pub fn present_ids(&self) -> &[PanelId] {
        &self.present
    }

    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        &self.panels[id]
    }

    /// 直前 tick でランタイムが終了を要求したか。
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// パネルが HMD の向きから視野内にあるかの推定。ランタイムが通知しない場合の代替で、広めの視野円錐とパネル外接球で判定する。
    pub fn in_view(&self, id: PanelId) -> bool {
        let p = &self.panels[id];
        // 非表示ポインタパネルは最終表示位置に残るため、その方向を向くたびに可視と誤判定される。
        if p.closed || p.hidden || p.surface.hidden() {
            return false;
        }
        let to = p.pose.position - self.hmd.position;
        let radius = p.displayed_size().length() / 2.0;
        let d = to.length();
        if d <= radius {
            return true;
        }
        let off_axis = self.hmd.forward().angle_between(to);
        off_axis - (radius / d).asin() < VIEW_HALF_ANGLE
    }

    /// パネルを注視しているか判定する。HMD 正面付近・面が頭方向・近距離の三条件で、腕時計注視姿勢を検出する。
    pub fn gazed(&self, id: PanelId) -> bool {
        let p = &self.panels[id];
        let to = p.pose.position - self.hmd.position;
        let d = to.length();
        !p.closed
            && !p.hidden
            && d > 1e-3
            && d < GAZE_REACH
            && self.hmd.forward().angle_between(to) < GAZE_ANGLE
            && (p.pose.orientation * Vec3::Z).angle_between(-to) < GAZE_FACING
    }

    /// `hand` のツールチップの表示位置（カーソルの上下）。頭から見てレーザー入射側に配置する。
    pub fn tooltip_anchor(&self, hand: Hand) -> Anchor {
        self.hands.get(&hand).map(|h| h.anchor).unwrap_or_default()
    }

    /// 直前 tick の HMD 姿勢。新規パネルの初期配置に使用する。
    pub fn hmd(&self) -> Pose {
        self.hmd
    }

    /// 直前 tick で検出されたトラッカー ID 一覧。
    pub fn trackers(&self) -> &[String] {
        &self.trackers
    }

    /// 直前 tick でスケルタル入力が得られた手の一覧。
    pub fn hands(&self) -> &[HandSkeleton] {
        &self.skeletons
    }

    /// 直前 tick の各コントローラ入力状態。
    pub fn controllers(&self) -> &[ControllerState] {
        &self.controllers
    }

    /// いずれかの手がメニューボタンを押下中か。
    pub fn menu_held(&self) -> bool {
        self.hands.values().any(|h| h.menu)
    }

    /// パネル外でのトリガー押下回数（起動時からの累計）。
    pub fn outside_presses(&self) -> u32 {
        self.outside_presses
    }

    /// いずれかの手のレーザーが `id` を指しているか。ナビバーは持ち主またはバー自体を指していれば真
    /// （持ち主→バー間で途切れない）。入力を受けない子パネルは指していない扱い。グラブ中も指している扱い。
    pub fn pointed(&self, id: PanelId) -> bool {
        let owner = self.panels[id].nav_bar_of;
        let is = |x: PanelId| x == id || Some(x) == owner;
        self.hands.values().any(|h| {
            h.hover.is_some_and(is)
                || h.dragging.is_some_and(|d| is(d.0))
                || h.stretch.is_some_and(|s| is(s.panel))
        })
    }

    /// `id` 自体（ナビバー・子を除く）をレーザーで指している手の一覧。
    pub fn pointing_hands(&self, id: PanelId) -> Vec<Hand> {
        self.hands
            .iter()
            .filter(|(_, h)| h.hover == Some(id))
            .map(|(&hand, _)| hand)
            .collect()
    }

    /// `id` をいずれかの手でグラブ中か（角を掴んで大きさを変えている間も含む）。
    pub fn grabbed(&self, id: PanelId) -> bool {
        self.hands.values().any(|h| {
            h.dragging.is_some_and(|d| d.0 == id) || h.stretch.is_some_and(|s| s.panel == id)
        })
    }
}

#[cfg(test)]
mod tests;
