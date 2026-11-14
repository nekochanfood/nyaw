//! パネルを足す・閉じる・隠すと、パネルごとの設定（取り付け・ロック・置き場所・大きさ・ポインタ・取り込み）。

use super::*;

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
        // 大きさは等倍のつもりで渡される。UI スケールはここで掛ける（[`Self::set_scale`]）。
        let size = size * self.scale();
        let surface = Surface::new(vr, key, size.x, pose)?;
        surface.set_sort(vr, PANEL_SORT);
        self.panels.push(Panel {
            pose,
            size,
            resolution,
            attached: None,
            locked: false,
            resizable: true,
            reach: None,
            grab_anchor: None,
            closed: false,
            hidden: false,
            pointer: None,
            animated: true,
            veil: false,
            capture: false,
            os_cursor: None,
            parent: None,
            nav_bar_of: None,
            nav_bar_size: size,
            offset: Pose::IDENTITY,
            dim: 0.0,
            dim_to: 0.0,
            opacity: 1.0,
            surface,
            follow: None,
            lazy_follow: None,
        });
        let id = self.panels.len() - 1;
        self.present.push(id);
        Ok(id)
    }

    /// 押している間のポインタのキャプチャ（はみ出してもスライドが続く）を使うか。タッチ方式で使う。
    pub fn set_capture(&mut self, capture: bool) {
        self.capture = capture;
    }

    pub fn set_panel_capture(&mut self, id: PanelId, capture: bool) {
        self.panels[id].capture = capture;
    }

    /// 配信元の解像度に合わせ、パネルの幅を保って縦横比だけ変える（親の上端に立てた画面は縦を保つ）。
    pub fn set_content_resolution(&mut self, id: PanelId, resolution: (u32, u32)) {
        assert!(resolution.0 > 0 && resolution.1 > 0);
        let p = &mut self.panels[id];
        p.resolution = resolution;
        let aspect = resolution.1 as f32 / resolution.0 as f32;
        // 親の上端に立てた画面（ダッシュボードにはめたもの）は高さをそろえるので、縦を保って横を合わせる。
        if p.parent.is_some_and(|q| q.slot.is_some()) {
            p.size.x = p.size.y / aspect;
        } else {
            p.size.y = p.size.x * aspect;
        }
    }

    /// 今の縦横比のまま、高さを `height`（m。等倍のつもりの値で、UI スケールが掛かる）に揃える。
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

    /// `hand` が押している間のキャプチャを外す（ドラッグ＆ドロップでほかのパネルへ運べるように）。離すと戻る。
    pub fn release_capture(&mut self, hand: Hand) {
        self.hands.entry(hand).or_default().free = true;
    }

    /// パネルを `device` に取り付ける。以降の tick で `device` の姿勢 * `offset` に追従する。
    /// 取り付け中のパネルを掴んで動かすと、離した位置でオフセットが付け直される。
    // 追従はランタイムに任せられれば任せる（`VrBackend::set_overlay_pose_relative`）。
    // ponytail: 任せられない（OpenXR）ときは tick の姿勢を送るので 1 フレーム分遅れる。
    pub fn attach(&mut self, id: PanelId, device: Device, offset: Pose) {
        self.panels[id].attached = Some((device, offset));
    }

    /// 取り付けを外す。パネルはその場（最後の姿勢）に残る。
    pub fn detach(&mut self, id: PanelId) {
        self.panels[id].attached = None;
    }

    /// `id` を `owner` のナビゲーションバーにする。以降は毎 tick、持ち主の下端の左右中央に、バーの上端の左右中央を
    /// 合わせて置かれる。今の `size` を頭から 1m 先での大きさとし、離れるほど大きくする（ツールチップと同じ）。
    /// 自分自身・閉じたパネルは持ち主にできない（何もしない）。
    pub fn set_nav_bar(&mut self, vr: &mut dyn VrBackend, id: PanelId, owner: PanelId) {
        if owner == id || self.panels[owner].closed {
            return;
        }
        let p = &mut self.panels[id];
        // 持ち主の影の余白に重なるので、表示だけの子と同じく手前に出す（place_nav_bars）。
        p.surface.set_sort(vr, HUD_SORT);
        p.nav_bar_of = Some(owner);
        p.nav_bar_size = p.size;
        p.attached = None;
    }

    /// パネルをその姿勢へ置く（次の tick でランタイムへ送る）。取り付け中なら取り付け先に戻される。
    pub fn set_pose(&mut self, id: PanelId, pose: Pose) {
        self.panels[id].pose = pose;
    }

    /// 頭の向き（左右だけ）と歩きにゆるく追従させる（`None` で外す）。取り付けより優先する。掴んでいる間は手に従う。
    pub fn set_lazy_follow(&mut self, id: PanelId, follow: Option<LazyFollow>) {
        self.panels[id].lazy_follow = follow;
    }

    /// 等倍のつもりで決めた姿勢 `pose` へパネルを置く。掴む基準（[`Self::set_grab_anchor`]）があれば、基準が等倍のときと
    /// 同じ所に来るように置く（[`anchored`]）。無ければ [`Self::set_pose`] と同じ。
    pub fn place(&mut self, id: PanelId, pose: Pose) {
        let scale = self.scale();
        let p = &mut self.panels[id];
        p.pose = match p.grab_anchor {
            // 覚えている基準は UI スケールを掛けた後のもの。
            Some(a) => anchored(pose, Pose::new(a.position / scale, a.orientation), scale),
            None => pose,
        };
    }

    /// 掴んで動かせないようにする（`false` で戻す）。レーザーでの操作はそのまま使える。
    /// 掴んでいる最中にロックすると掴みを取り消す（取り付け中なら掴む前のオフセットへ戻る）。
    pub fn set_locked(&mut self, id: PanelId, locked: bool) {
        self.panels[id].locked = locked;
        if !locked {
            return;
        }
        for h in self.hands.values_mut() {
            if h.dragging.is_some_and(|d| d.0 == id) {
                h.dragging = None;
                h.scaling = None;
            }
        }
    }

    /// 取り付けたまま掴んで動かせる範囲を、取り付け先から `reach` m 以内に限る（`None` で限らない）。
    pub fn set_reach(&mut self, id: PanelId, reach: Option<f32>) {
        self.panels[id].reach = reach;
    }

    /// 掴んだとき、パネルそのものではなく `anchor`（このパネルから見た姿勢）を手で動かして頭へ向け、パネルはそこに付いてくる。
    /// ダッシュボードのバーは、画面がはまっていなくても、画面が立つ所を基準に動く。`anchor` の位置は等倍のつもりの値で、
    /// UI スケールが掛かる。
    pub fn set_grab_anchor(&mut self, id: PanelId, anchor: Option<Pose>) {
        let scale = self.scale();
        self.panels[id].grab_anchor = anchor.map(|a| Pose::new(a.position * scale, a.orientation));
    }

    /// 掴んで大きさを変えられるか（ダッシュボードのバーのように、大きさの決まったものは `false`）。
    pub fn set_resizable(&mut self, id: PanelId, resizable: bool) {
        self.panels[id].resizable = resizable;
    }

    /// パネルの解像度（UI のピクセル数）を変える。1m あたりのピクセル数は保ち、VR 上の大きさも同じ割合で変える
    /// （ウィンドウを広げると、中身が縮まずにパネルが広がる）。中心の位置は変えない。
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

    /// パネルごとのオプトアウト。再生中に無効化した場合も、直ちに最終状態にする。
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

    /// `id` が出ている間（最初の描画が届いてから閉じるまで）、空間全体を暗くする。暗転・復帰はパネルの出入りと同じ動きで進む。
    /// 幕はパネルより奥に描くので、パネル・レーザーは暗くならない。
    pub fn set_veil(&mut self, id: PanelId, veil: bool) {
        self.panels[id].veil = veil;
    }

    /// `hand` のレーザーを描くパネルにする。以降はヒット判定の対象外になり、毎 tick レーザーに合わせて置かれる。
    pub fn set_pointer(&mut self, vr: &mut dyn VrBackend, id: PanelId, hand: Hand, part: Pointer) {
        self.set_animated(vr, id, false);
        let p = &mut self.panels[id];
        p.pointer = Some((hand, part));
        let sort = match part {
            Pointer::Laser => LASER_SORT,
            Pointer::Cursor => CURSOR_SORT,
            Pointer::Drag => DRAG_SORT,
            Pointer::Tooltip => TOOLTIP_SORT + tooltip_step(hand),
        };
        p.surface.set_sort(vr, sort);
        // レーザーは長さ（幅）だけ変え、太さ（高さ）は作ったときのまま。
        if part == Pointer::Laser {
            p.surface.set_height(vr, p.size.y);
        }
    }

    /// `id` を手 `hand` で指している間、その手のカーソルを OS のカーソル `cursor`（パネルと同じ縮尺、ホットスポットをヒット点に）に
    /// 差し替える（もう片方の手は nyaw のカーソルのまま）。`None` で戻す。
    pub fn set_os_cursor(&mut self, id: PanelId, cursor: Option<(Hand, Arc<Cursor>)>) {
        self.panels[id].os_cursor = cursor;
    }

    /// `hand` でドラッグしている間、その見た目（[`Pointer::Drag`] のパネル）を幅 `width_m` で出す。`None` で隠す。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.hands.entry(hand).or_default().drag = width_m;
    }

    /// パネルを閉じる。入力を即座に止め、最後の描画をフェードアウトしてから隠す。
    /// フェードアウトが終わると、ランタイム側のオーバーレイも捨てる（[`VrBackend::destroy_overlay`]）。
    pub fn close(&mut self, vr: &mut dyn VrBackend, id: PanelId) {
        let p = &mut self.panels[id];
        if p.closed {
            return;
        }
        p.closed = true;
        self.retreat(id);
        // 子とナビゲーションバーは親と一緒に閉じる。
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
        if !p.parent.is_some_and(|q| q.slot.is_some()) || !p.animated || p.opacity == 0.0 {
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
