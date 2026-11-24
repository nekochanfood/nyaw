//! パネルの追加・クローズ・非表示とパネル単位の設定（アタッチ・ロック・配置・サイズ・ポインタ・キャプチャ）。

use super::*;

/// ハンドアタッチパネル（[`Overlay::attach_to_hand`]）の面下端とレーザー発射点との隙間（m）。下部のナビバー
/// （腕の先端で約 2cm）がコントローラに重ならず収まる距離。
// ponytail: 目分量。バーがコントローラに重なる・離れすぎて見えればこの値で調整。
const HAND_GAP: f32 = 0.03;
/// ハンドアタッチパネルの長辺上限（m。等倍設計値で UI スケールが適用される）。これより大きければ
/// アスペクト比を維持して縮小する（腕の先端では、据え置きサイズのままだと視界からはみ出す）。
// ponytail: 目分量。大きすぎる・小さすぎればこの値で調整。
const HAND_SIZE: f32 = 0.4;
/// ハンドアタッチパネルをコントローラ面から手前（天面側）へ起こす角度。手首を完全に起こさなくても表面が見える。
const HAND_TILT: f32 = std::f32::consts::FRAC_PI_4;

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_panel(
        &mut self,
        vr: &mut dyn VrBackend,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        // サイズは等倍設計で渡される。UI スケールはここで適用する（[`Self::set_scale`]）。
        let size = size * self.scale();
        let mut surface = Surface::new(vr, key, size.x, pose)?;
        surface.set_sort(vr, PANEL_SORT);
        self.panels.push(Panel {
            pose,
            size,
            resolution,
            attached: None,
            locked: false,
            resizable: true,
            corner_resize: None,
            reach: None,
            grab_anchor: None,
            closed: false,
            hidden: false,
            pointer: None,
            animated: true,
            veil: false,
            pass_through: false,
            fade_only: false,
            capture: false,
            os_cursor: None,
            parent: None,
            nav_bar_of: None,
            nav_bar_size: size,
            awake: 1.0,
            wake: Transition::rest(1.0),
            pointed_at: Instant::now(),
            offset: Pose::IDENTITY,
            dim: 0.0,
            dim_to: 0.0,
            opacity: 1.0,
            stand_in: 0.0,
            surface,
            follow: None,
            lazy_follow: None,
            dockable: false,
        });
        let id = self.panels.len() - 1;
        self.present.push(id);
        Ok(id)
    }

    /// プレス中のポインタキャプチャ（はみ出してもスライド継続）の有効化。タッチ方式で使用する。
    pub fn set_capture(&mut self, capture: bool) {
        self.capture = capture;
    }

    pub fn set_panel_capture(&mut self, id: PanelId, capture: bool) {
        self.panels[id].capture = capture;
    }

    /// キャプチャ元の解像度に合わせてパネルの幅を維持しアスペクト比のみ変更する（スロット画面は高さを維持）。
    pub fn set_content_resolution(&mut self, id: PanelId, resolution: (u32, u32)) {
        assert!(resolution.0 > 0 && resolution.1 > 0);
        let p = &mut self.panels[id];
        p.resolution = resolution;
        let aspect = resolution.1 as f32 / resolution.0 as f32;
        // スロット画面（ダッシュボードにドックしたもの）は高さを揃えるため、縦を保って横を合わせる。
        if p.parent.is_some_and(|q| q.slot.is_some()) {
            p.size.x = p.size.y / aspect;
        } else {
            p.size.y = p.size.x * aspect;
        }
    }

    /// 現在のアスペクト比のまま高さを `height`（m。等倍設計値で UI スケールが適用される）に揃える。
    pub fn fit_height(&mut self, vr: &mut dyn VrBackend, id: PanelId, height: f32) {
        if !height.is_finite() || height <= 0.0 {
            return;
        }
        let height = height * self.scale();
        let p = &mut self.panels[id];
        p.size = Vec2::new(
            height * p.resolution.0 as f32 / p.resolution.1 as f32,
            height,
        );
        p.surface.set_width(vr, p.displayed_size().x);
    }

    /// `hand` のプレス中キャプチャを解除する（ドラッグ＆ドロップで他パネルへ搬送可能にする）。リリースで復帰。
    pub fn release_capture(&mut self, hand: Hand) {
        self.hands.entry(hand).or_default().free = true;
    }

    /// パネルを `device` にアタッチする。以降の tick で `device` の姿勢 * `offset` に追従する。
    /// アタッチ中のパネルをグラブして移動するとリリース位置でオフセットが再計算される。
    // 追従はランタイムに委譲可能なら委譲する（`VrBackend::set_overlay_pose_relative`）。
    // ponytail: 委譲不可（OpenXR）の場合は tick の姿勢を送るため 1 フレーム遅延する。
    pub fn attach(&mut self, id: PanelId, device: Device, offset: Pose) {
        self.panels[id].attached = Some((device, offset));
        // ドック解除時のトランジションが残っていれば中止し、即座にアタッチ先に配置する。
        self.glides.remove(&id);
    }

    /// パネルを `hand` のコントローラにアタッチする。レーザー発射点を根元に前方へ伸ばし（上から見るとコントローラ先端に
    /// 面がある）、手前へ [`HAND_TILT`] 起こす。手を起こすと直立して表面が見える。面下端の左右中央が面に沿って
    /// 根元から [`HAND_GAP`] の位置に来る。長辺が [`HAND_SIZE`] を超える場合はアスペクト比を維持して縮小する。
    pub fn attach_to_hand(&mut self, vr: &mut dyn VrBackend, id: PanelId, hand: Hand) {
        let limit = HAND_SIZE * self.scale();
        let p = &mut self.panels[id];
        let fit = (limit / p.size.max_element()).min(1.0);
        if fit < 1.0 {
            p.size *= fit;
            // グラブ基準はコンテンツの固定点のためサイズに合わせて移動する。
            if let Some(anchor) = &mut p.grab_anchor {
                anchor.position *= fit;
            }
            p.surface.set_width(vr, p.displayed_size().x);
        }
        // 表面（+Z）をコントローラ天面（+Y）へ、上（+Y）を前方（-Z）へ向けてから上を天面側へ起こす。
        let lay = Quat::from_rotation_x(HAND_TILT - std::f32::consts::FRAC_PI_2);
        let reach = p.size.y / 2.0 - face_margin(p) + HAND_GAP;
        let offset = Pose::new(lay * Vec3::Y * reach, lay);
        self.attach(id, Device::Controller(hand), offset);
    }

    /// アタッチを解除する。パネルはその場（最終姿勢）に残る。
    pub fn detach(&mut self, id: PanelId) {
        self.panels[id].attached = None;
    }

    /// `id` を `owner` のナビバーにする。以降は毎 tick 持ち主の下端左右中央にバーの上端左右中央を
    /// 合わせて配置する。現在の `size` を頭から 1m 先でのサイズとし、距離に比例して拡大する（ツールチップと同様）。
    /// 自分自身・クローズ済みパネルは持ち主に指定不可（何もしない）。
    pub fn set_nav_bar(&mut self, vr: &mut dyn VrBackend, id: PanelId, owner: PanelId) {
        if owner == id || self.panels[owner].closed {
            return;
        }
        let p = &mut self.panels[id];
        // 持ち主のシャドウマージンに重なるため表示専用子と同じく手前に出す（place_nav_bars）。
        p.surface.set_sort(vr, HUD_SORT);
        p.nav_bar_of = Some(owner);
        p.nav_bar_size = p.size;
        p.attached = None;
    }

    /// パネルをその姿勢に配置する（次の tick でランタイムへ送信）。アタッチ中ならアタッチ先に戻される。
    pub fn set_pose(&mut self, id: PanelId, pose: Pose) {
        self.panels[id].pose = pose;
    }

    /// 頭の向き（ヨーのみ）と歩行に緩やかに追従させる（`None` で解除）。アタッチより優先する。グラブ中は手に従う。
    pub fn set_lazy_follow(&mut self, id: PanelId, follow: Option<LazyFollow>) {
        self.panels[id].lazy_follow = follow;
    }

    /// 等倍設計の姿勢 `pose` にパネルを配置する。グラブ基準（[`Self::set_grab_anchor`]）があれば基準が等倍時と
    /// 同じ位置に来るよう配置する（[`anchored`]）。無ければ [`Self::set_pose`] と同じ。
    pub fn place(&mut self, id: PanelId, pose: Pose) {
        let scale = self.scale();
        let p = &mut self.panels[id];
        p.pose = match p.grab_anchor {
            // 保持している基準は UI スケール適用後の値。
            Some(a) => anchored(pose, Pose::new(a.position / scale, a.orientation), scale),
            None => pose,
        };
    }

    /// グラブ移動を禁止する（`false` で解除）。レーザー操作はそのまま使用可能。
    /// グラブ中にロックするとグラブを取り消す（アタッチ中ならグラブ前のオフセットに復帰。サイズもグラブ前に復帰）。
    pub fn set_locked(&mut self, vr: &mut dyn VrBackend, id: PanelId, locked: bool) {
        self.panels[id].locked = locked;
        if !locked {
            return;
        }
        for h in self.hands.values_mut() {
            if h.dragging.is_some_and(|d| d.0 == id) {
                h.dragging = None;
                h.scaling = None;
                let p = &mut self.panels[id];
                (p.size, p.grab_anchor) = h.before;
                p.surface.set_width(vr, p.displayed_size().x);
            }
        }
    }

    /// アタッチ状態でグラブ移動できる範囲をアタッチ先から `reach` m 以内に制限する（`None` で無制限）。
    pub fn set_reach(&mut self, id: PanelId, reach: Option<f32>) {
        self.panels[id].reach = reach;
    }

    /// グラブ時にパネル本体ではなく `anchor`（パネルローカルの姿勢）を手で移動して頭方向を向け、パネルはそこに追従する。
    /// ダッシュボードのバーはドック画面がなくても画面の立ち位置を基準に移動する。`anchor` の位置は等倍設計値で
    /// UI スケールが適用される。
    pub fn set_grab_anchor(&mut self, id: PanelId, anchor: Option<Pose>) {
        let scale = self.scale();
        self.panels[id].grab_anchor = anchor.map(|a| Pose::new(a.position * scale, a.orientation));
    }

    /// グラブ基準を `anchor` に変更し、現在の基準（無ければ中心）の位置に新しい基準が来るようパネルを移動する。
    /// レイアウト再構築で基準点が移動しても（キーボード種別変更時のホームポジション）空間上ではその点を維持する。
    /// 向きは不変。位置は等倍設計（[`Self::set_grab_anchor`]）。親・持ち主追従中のパネルは基準のみ変更。
    pub fn move_grab_anchor(&mut self, id: PanelId, anchor: Pose) {
        let anchor = Pose::new(anchor.position * self.scale(), anchor.orientation);
        let p = &mut self.panels[id];
        let fixed = p.pose * p.grab_anchor.unwrap_or(Pose::IDENTITY);
        if p.parent.is_none() && p.nav_bar_of.is_none() {
            p.pose = fixed * anchor.inverse();
        }
        p.grab_anchor = Some(anchor);
    }

    /// グラブでリサイズ可能か（ダッシュボードのバーのようにサイズ固定のものは `false`）。
    pub fn set_resizable(&mut self, id: PanelId, resizable: bool) {
        self.panels[id].resizable = resizable;
    }

    /// パネルの解像度（UI のピクセル数）を変更する。1m あたりのピクセル密度を維持し VR 上のサイズも同率で変更する
    /// （ウィンドウ拡大時にコンテンツが縮小せずパネルが拡大する）。中心位置は不変。
    pub fn set_resolution(&mut self, vr: &mut dyn VrBackend, id: PanelId, resolution: (u32, u32)) {
        let p = &mut self.panels[id];
        let (w, h) = (resolution.0.max(1), resolution.1.max(1));
        if p.resolution == (w, h) || p.surface.released() {
            return;
        }
        p.size = Vec2::new(
            p.size.x * w as f32 / p.resolution.0 as f32,
            p.size.y * h as f32 / p.resolution.1 as f32,
        );
        p.resolution = (w, h);
        p.surface.set_width(vr, p.displayed_size().x);
    }

    /// パネル単位のアニメーション無効化。再生中に無効化した場合も即座に最終状態にする。
    pub fn set_animated(&mut self, vr: &mut dyn VrBackend, id: PanelId, animated: bool) {
        self.panels[id].animated = animated;
        if !animated {
            self.animations.remove(&id);
            self.dims.remove(&id);
            let p = &mut self.panels[id];
            p.dim = p.dim_to;
            p.opacity = if p.closed || p.hidden { 0.0 } else { 1.0 };
            Self::present(vr, p, false);
        }
    }

    /// `id` が表示中（初回描画到着からクローズまで）空間全体を暗転する。暗転・復帰はパネルの出入りと同じ動きで進む。
    /// 幕はパネルより奥に描画するためパネル・レーザーは暗転しない。
    pub fn set_veil(&mut self, id: PanelId, veil: bool) {
        self.panels[id].veil = veil;
    }

    /// レーザーを透過させる（ポイント不可、背後のパネルをポイント可能）。視界端に出す表示専用 HUD 用。
    pub fn set_pass_through(&mut self, id: PanelId, pass: bool) {
        self.panels[id].pass_through = pass;
    }

    /// 出入り（開く・クローズ・非表示・再表示）を不透明度のみにする（`false` で解除）。サイズを 75%〜100% に変えず
    /// ドック画面のように後退しない。移動方向はコンテンツ（GPUI）が描画するパネル（通知トースト）に委ねる。
    pub fn set_fade_only(&mut self, id: PanelId, fade_only: bool) {
        self.panels[id].fade_only = fade_only;
    }

    /// `hand` のレーザーを描画するパネルにする。以降はヒット判定対象外になり毎 tick レーザーに合わせて配置される。
    pub fn set_pointer(&mut self, vr: &mut dyn VrBackend, id: PanelId, hand: Hand, part: Pointer) {
        self.set_animated(vr, id, false);
        let p = &mut self.panels[id];
        p.pointer = Some((hand, part));
        let sort = match part {
            Pointer::Laser => LASER_SORT,
            Pointer::Cursor => CURSOR_SORT,
            Pointer::Drag => DRAG_SORT,
            Pointer::Tooltip => TOOLTIP_SORT + tooltip_step(hand),
            Pointer::Dock | Pointer::Corner => GHOST_SORT,
        };
        p.surface.set_sort(vr, sort);
        // レーザーは長さ（幅）のみ変更、太さ（高さ）は生成時のまま。
        if part == Pointer::Laser {
            p.surface.set_height(vr, p.size.y);
        }
    }

    /// `id` を手 `hand` でポイント中、その手のカーソルを OS カーソル `cursor`（パネルと同縮尺、ホットスポットをヒット点に）に
    /// 差し替える（もう片方の手は nyaw のカーソルのまま）。`None` で復帰。
    pub fn set_os_cursor(&mut self, id: PanelId, cursor: Option<(Hand, Arc<Cursor>)>) {
        self.panels[id].os_cursor = cursor;
    }

    /// `hand` でドラッグ中、ドラッグ表示（[`Pointer::Drag`] パネル）を幅 `width_m` で表示する。`None` で非表示。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.hands.entry(hand).or_default().drag = width_m;
    }

    /// パネルをクローズする。入力を即座に停止し、最終描画をフェードアウトしてから非表示にする。
    /// フェードアウト完了後にランタイム側のオーバーレイも破棄する（[`VrBackend::destroy_overlay`]）。
    pub fn close(&mut self, vr: &mut dyn VrBackend, id: PanelId) {
        let p = &mut self.panels[id];
        if p.closed {
            return;
        }
        p.closed = true;
        self.retreat(id);
        // 子とナビバーは親と一緒にクローズする。
        let kids: Vec<PanelId> = self
            .present
            .iter()
            .copied()
            .filter(|&c| {
                let q = &self.panels[c];
                q.parent.is_some_and(|q| q.panel == id) || q.nav_bar_of == Some(id)
            })
            .collect();
        for c in kids {
            self.close(vr, c);
        }
        let p = &mut self.panels[id];
        if p.nav_bar_of.is_some() {
            // 持ち主と一緒にフェードアウトし、持ち主が消えたら解放される（place_nav_bars）。
        } else if p.animated && p.surface.has_texture() && p.opacity > 0.0 {
            self.animations.insert(
                id,
                (Animation::new(p.opacity, 0.0, Motion::Exit), Instant::now()),
            );
        } else {
            self.animations.remove(&id);
            p.opacity = 0.0;
            Self::present(vr, p, false);
        }
        for h in self.hands.values_mut() {
            if h.hover == Some(id) {
                h.hover = None;
            }
            if h.dragging.is_some_and(|d| d.0 == id) {
                h.dragging = None;
            }
        }
    }

    /// 親の上端に立てた画面（[`Parent::slot`]）が見えたまま消えるとき（閉じる・隠す）、フェードアウトしながら後ろへ下がらせる。
    /// 入れ替わりにはまる画面と同じ所で重なってちらつかないように。
    fn retreat(&mut self, id: PanelId) {
        let p = &self.panels[id];
        if !p.parent.is_some_and(|q| q.slot.is_some())
            || !p.animated
            || p.fade_only
            || p.opacity == 0.0
        {
            return;
        }
        self.glides.insert(
            id,
            Glide {
                from: p.pose,
                to: Some(p.pose * Pose::new(Vec3::NEG_Z * SLOT_RETREAT, Quat::IDENTITY)),
                tween: Animation::new(0.0, 1.0, Motion::Exit),
                last: Instant::now(),
            },
        );
    }

    /// パネルを隠す・出し直す。閉じずにフェードアウトし（ビュー・配信・音はそのまま）、出し直すとフェードインする。
    /// 隠している間は入力・暗転の対象外。子（固定した画面・ダイアログ）も一緒に隠れ、ナビゲーションバーは持ち主を写す。
    pub fn set_hidden(&mut self, vr: &mut dyn VrBackend, id: PanelId, hidden: bool) {
        let p = &mut self.panels[id];
        if p.closed || p.hidden == hidden {
            return;
        }
        p.hidden = hidden;
        if hidden {
            self.retreat(id);
        } else {
            // 隠すときの退き（隠した時の場所へ向かう）が残っていれば、出し直した場所から引き戻さないよう打ち切る。
            self.glides.remove(&id);
        }
        let kids: Vec<PanelId> = self
            .present
            .iter()
            .copied()
            .filter(|&c| self.panels[c].parent.is_some_and(|q| q.panel == id))
            .collect();
        for c in kids {
            self.set_hidden(vr, c, hidden);
        }
        let p = &mut self.panels[id];
        let to = if hidden { 0.0 } else { 1.0 };
        if p.animated && p.surface.has_texture() {
            let motion = if hidden { Motion::Exit } else { Motion::Enter };
            self.animations
                .insert(id, (Animation::new(p.opacity, to, motion), Instant::now()));
        } else {
            self.animations.remove(&id);
            p.opacity = to;
            Self::present(vr, p, false);
        }
        if hidden {
            for h in self.hands.values_mut() {
                if h.hover == Some(id) {
                    h.hover = None;
                }
                if h.dragging.is_some_and(|d| d.0 == id) {
                    h.dragging = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
