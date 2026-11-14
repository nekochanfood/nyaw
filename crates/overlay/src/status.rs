//! 状態の問い合わせ（上の層が読む）: パネル・頭と手・指している手・掴んでいる手・外側を押した回数。

use super::*;

/// 見えていそうかの見積もり（[`Overlay::in_view`]）に使う視野の半角（ラジアン、70°。実機の視野より広めに取る）。
const VIEW_HALF_ANGLE: f32 = 1.221_730_5;

impl Overlay {
    /// パネルの数（閉じたものも含む。PanelId は 0..len）。
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

    /// 最後の tick でランタイムが終了を求めていたか。
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// パネルが頭の向きから見えていそうか（ランタイムが教えてくれないときの見積もり。視野を広めに取った円錐とパネルを囲む球で判定）。
    pub fn in_view(&self, id: PanelId) -> bool {
        let p = &self.panels[id];
        // 隠しているポインタのパネルは最後に出した所に残るので、そちらを向くたびに見えた扱いで描き直していた。
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

    /// `hand` のツールチップをカーソルの上下どちらに出すか。頭から見て、レーザーの来る側（手）に出す。
    pub fn tooltip_anchor(&self, hand: Hand) -> Anchor {
        self.hands.get(&hand).map(|h| h.anchor).unwrap_or_default()
    }

    /// 最後の tick の HMD の姿勢（新しいパネルを目の前に出すため）。
    pub fn hmd(&self) -> Pose {
        self.hmd
    }

    /// 最後の tick で見えていたトラッカーの ID。
    pub fn trackers(&self) -> &[String] {
        &self.trackers
    }

    /// 最後の tick で骨格が取れていた手（スケルタル入力）。
    pub fn hands(&self) -> &[HandSkeleton] {
        &self.skeletons
    }

    /// 最後の tick の手ごとの入力の状態。
    pub fn controllers(&self) -> &[ControllerState] {
        &self.controllers
    }

    /// どちらかの手でメニューボタンを押している。
    pub fn menu_held(&self) -> bool {
        self.hands.values().any(|h| h.menu)
    }

    /// どのパネルにも当たっていない所でトリガーを押した回数（起動から数える）。
    pub fn outside_presses(&self) -> u32 {
        self.outside_presses
    }

    /// どちらかの手のレーザーが `id` を指している。ナビゲーションバーは、持ち主かバー自身を指していれば（持ち主から
    /// バーへ移っても途切れない。バーの上端は持ち主の影の余白に重なっている）。子が開いて入力を受けない間は指していない扱い。
    /// 掴んでいる間も指している扱い（掴んで動かしている間もバーは広がったまま）。
    pub fn pointed(&self, id: PanelId) -> bool {
        let owner = self.panels[id].nav_bar_of;
        let is = |x: PanelId| x == id || Some(x) == owner;
        self.hands
            .values()
            .any(|h| h.hover.is_some_and(is) || h.dragging.is_some_and(|d| is(d.0)))
    }

    /// `id` そのもの（ナビゲーションバーや子ではなく）をレーザーで指している手。
    pub fn pointing_hands(&self, id: PanelId) -> Vec<Hand> {
        self.hands
            .iter()
            .filter(|(_, h)| h.hover == Some(id))
            .map(|(&hand, _)| hand)
            .collect()
    }

    /// `id` をどちらかの手で掴んでいる。
    pub fn grabbed(&self, id: PanelId) -> bool {
        self.hands
            .values()
            .any(|h| h.dragging.is_some_and(|d| d.0 == id))
    }
}
