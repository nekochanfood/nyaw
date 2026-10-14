//! Layer 3: オーバーレイの土台。3D 空間上のパネルを持ち、各コントローラのレーザーとの交差を判定し、
//! ヒットを UI 層向けの 2D ポインタイベントに変換する。パネルを掴んで動かす操作も扱う。
//! パネルはデバイス（HMD・コントローラ・トラッカー）に取り付けて追従させられる。
//! [`Overlay::tick`] が下位パイプライン1フレーム分: VR poll → 取り付け先への追従 → 入力マップ → ヒット判定 → イベント。

use std::collections::HashMap;
use std::time::Instant;

use animato::{Easing, Tween, Update};

use vr::{Device, Frame, Hand, Mat3, OverlayId, Pose, Quat, Vec2, Vec3, VrBackend, VrError};

pub use input::ControllerState;

pub type PanelId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// クアッドの中心（トラッキング空間）。表面はローカル +Z を向く。
    /// 取り付け中は毎 tick `取り付け先の姿勢 * オフセット` で更新される。
    pub pose: Pose,
    /// メートル単位。
    pub size: Vec2,
    /// クアッドに貼る UI サーフェスのピクセル数。
    pub resolution: (u32, u32),
    /// 取り付け先のデバイスと、そこから見たパネルの相対姿勢。
    pub attached: Option<(Device, Pose)>,
    /// 掴んでも動かない。取り付けていなければ空間に固定される。
    pub locked: bool,
    /// 閉じられた。ヒット判定の対象外で、PanelId は詰めずに欠番のまま残る。
    pub closed: bool,
    /// どの手のレーザーを描くパネルか。ヒット判定の対象外で、配置は毎 tick ここで決める。
    pub pointer: Option<(Hand, Pointer)>,
    /// このパネルのイン・アウトアニメーション。`Overlay::set_animated` で個別に無効化できる。
    pub animated: bool,
    /// 表示の進行度。サイズは 75%〜100%、不透明度は 0%〜100%。
    opacity: f32,
    /// フェードアウトにも使う、加工前の最後の描画。
    texture: Option<(u32, u32, Vec<u8>)>,
    overlay: OverlayId,
    /// この tick で追従させるデバイスと相対姿勢。ランタイムに任せられればそうする。
    follow: Option<(Device, Pose)>,
    /// ランタイムへ最後に送った（相対の先, 姿勢）。変わったときだけ送る。
    sent: Option<(Option<Device>, Pose)>,
    /// ランタイムへ最後に送った（表示, 幅）。ポインタのパネルだけが使う。
    shown: Option<(bool, f32)>,
}

/// レーザーを描くパネルの役割。見た目は上の層の画面が描き、置き場所はオーバーレイ層が決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// 手元からヒット点（掴み中は掴んだパネル）まで伸びる線。何も指していなければ隠す。幅 = 長さで、テクスチャの横方向が
    /// レーザーの向き（左端が手元）。テクスチャの面はコントローラの上を向く。
    Laser,
    /// ヒット点に貼り付く印。パネルより手前に描く。ヒットしていなければ隠す。
    Cursor,
    /// ドラッグ中の要素の見た目。中心をヒット点に置き、[`Overlay::set_drag`] で幅が決まっている間だけ出す。
    /// パネルの外へも描けるので、ウィンドウをまたいで運べる。
    Drag,
}

/// レーザーのパネルを作るときの長さ（m）。以降は毎 tick ヒット点までの長さに伸び縮みする。
pub const LASER_MAX: f32 = 1.5;
/// カーソルをパネル面からレーザーの来る側へ浮かせる距離（m）。描画順でもパネルより手前に出すが、
/// 奥行きで並べるランタイムでも同じ面に乗って Z ファイティングしないように少し離す。
const CURSOR_LIFT: f32 = 0.003;
/// レーザーのクアッドの向き: ローカル +X（テクスチャの右）を前方 -Z へ、面の法線 +Z を上 +Y へ。
const LASER_ROT: Quat = Quat::from_xyzw(-0.5, 0.5, 0.5, 0.5);
/// 見えていそうかの見積もり（[`Overlay::in_view`]）に使う視野の半角（ラジアン、70°。実機の視野より広めに取る）。
const VIEW_HALF_ANGLE: f32 = 1.221_730_5;
/// 描画順。パネルは 0。
const LASER_SORT: u32 = 1;
const DRAG_SORT: u32 = 2;
const CURSOR_SORT: u32 = 3;
/// ドラッグの見た目をパネル面から浮かせる距離（m）。カーソルより奥。
const DRAG_LIFT: f32 = 0.002;
/// 掴み移動で、目標へ 1 tick に近づく割合。目標との差が [`SNAP`] 未満なら目標に合わせる。
// ponytail: tick 単位なのでリフレッシュレートで速さが変わる（144Hz で約 0.1 秒）。気になれば経過時間で決める。
const SMOOTH: f32 = 0.3;
const SNAP: f32 = 1e-5;
/// 手ブレ抑えの既定値: レーザーの向きが 1 tick ごとに前の向きを残す割合（0 = 抑えない）。
/// ヒット判定もこの向きで行うので、クリック位置も揺れない。
// ponytail: tick 単位なのでリフレッシュレートで効き方が変わる。気になれば経過時間で決める。
const POINTER_SMOOTHING: f32 = 0.8;
/// 掴み中のスティックを倒し切ったとき、1 tick に距離が変わる割合（遠いほど速い）と、効き始める倒し量。
const PUSH: f32 = 0.02;
const PUSH_DEADZONE: f32 = 0.2;
/// 掴んだパネルを手から離せる距離（m）。
const GRAB_MIN: f32 = 0.15;
const GRAB_MAX: f32 = 10.0;

/// この tick のレーザー。
#[derive(Debug, Clone, Copy)]
struct Ray {
    /// 手（コントローラ）の姿勢。
    hand: Pose,
    /// レーザー: 位置は手、向きは手ブレを抑えたもの。ヒット判定もこれで行う。
    aim: Pose,
    len: f32,
    /// 当たったパネルとトラッキング空間での点。
    hit: Option<(PanelId, Vec3)>,
}

impl Panel {
    /// アニメーションを反映した表示サイズ。中心の位置・姿勢は変えない。
    pub fn displayed_size(&self) -> Vec2 {
        self.size * (0.75 + 0.25 * self.opacity)
    }

    /// パネル上のピクセル（左上原点）をトラッキング空間の点に変換する。ヒット判定の逆。
    pub fn pixel_to_world(&self, px: Vec2) -> Vec3 {
        let (w, h) = (self.resolution.0 as f32, self.resolution.1 as f32);
        let local = Vec3::new(
            (px.x / w - 0.5) * self.displayed_size().x,
            (0.5 - px.y / h) * self.displayed_size().y,
            0.0,
        );
        self.pose.transform_point(local)
    }

    /// ランタイム側のオーバーレイ。実ランタイムから描画を読み戻して確かめるとき用。
    pub fn overlay_id(&self) -> OverlayId {
        self.overlay
    }
}

/// 座標はパネルのピクセル、原点は左上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Move { pos: Vec2 },
    Down { pos: Vec2 },
    Up { pos: Vec2 },
    Scroll { pos: Vec2, delta: Vec2 },
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiEvent {
    pub panel: PanelId,
    pub hand: Hand,
    pub event: PointerEvent,
}

#[derive(Debug, Default)]
struct HandState {
    hover: Option<PanelId>,
    select: bool,
    grab_held: bool,
    menu: bool,
    /// ドラッグ中のパネルと、レーザーから見たその中心、掴んだ点までのレーザー上の距離。
    dragging: Option<(PanelId, Vec3, f32)>,
    /// ドラッグの見た目の幅（m）。ドラッグ中だけ。
    drag: Option<f32>,
    /// 前の tick に描いたレーザーの向き。隠れていたら `None`（次に出すときは追わずに合わせる）。
    drawn: Option<Quat>,
    /// この tick のレーザー。コントローラが見えていない・何も指していなければ `None`。
    ray: Option<Ray>,
    /// 押している間のキャプチャを外した（離すまで）。
    free: bool,
}

#[derive(Debug, Default)]
pub struct Overlay {
    panels: Vec<Panel>,
    hands: HashMap<Hand, HandState>,
    /// 手ごとの前フレームの入力（ボタンのヒステリシス用）。
    inputs: HashMap<Hand, ControllerState>,
    /// 最後の tick で見えていたトラッカーの ID（取り付け先の候補）。
    trackers: Vec<String>,
    quit: bool,
    /// 最後の tick の HMD の姿勢。
    hmd: Pose,
    /// 押している間のポインタのキャプチャを使うか（[`Overlay::set_capture`]）。
    capture: bool,
    /// 手ブレ抑え（[`POINTER_SMOOTHING`]）。`None` で既定値。
    smoothing: Option<f32>,
    animations: HashMap<PanelId, (Tween<f32>, Instant)>,
}

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
        let overlay = vr.create_overlay(key, size.x)?;
        vr.set_overlay_pose(overlay, pose);
        self.panels.push(Panel {
            pose,
            size,
            resolution,
            attached: None,
            locked: false,
            closed: false,
            pointer: None,
            animated: true,
            opacity: 1.0,
            texture: None,
            overlay,
            follow: None,
            sent: None,
            shown: None,
        });
        Ok(self.panels.len() - 1)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        &self.panels[id]
    }

    /// 最後の tick でランタイムが終了を求めていたか。
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// 手ブレ抑えの強さ（0 = 抑えない 〜 1 未満）。
    pub fn set_pointer_smoothing(&mut self, smoothing: f32) {
        self.smoothing = Some(smoothing.clamp(0.0, 0.99));
    }

    /// パネルが頭の向きから見えていそうか（ランタイムが教えてくれないときの見積もり。視野を広めに取った円錐とパネルを囲む球で判定）。
    pub fn in_view(&self, id: PanelId) -> bool {
        let p = &self.panels[id];
        if p.closed {
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

    /// 押している間のポインタのキャプチャ（はみ出してもスライドが続く）を使うか。タッチ方式で使う。
    pub fn set_capture(&mut self, capture: bool) {
        self.capture = capture;
    }

    /// `hand` が押している間のキャプチャを外す（ドラッグ＆ドロップでほかのパネルへ運べるように）。離すと戻る。
    pub fn release_capture(&mut self, hand: Hand) {
        self.hands.entry(hand).or_default().free = true;
    }

    /// 最後の tick の HMD の姿勢（新しいパネルを目の前に出すため）。
    pub fn hmd(&self) -> Pose {
        self.hmd
    }

    /// 最後の tick で見えていたトラッカーの ID。
    pub fn trackers(&self) -> &[String] {
        &self.trackers
    }

    /// どちらかの手でメニューボタンを押している。
    pub fn menu_held(&self) -> bool {
        self.hands.values().any(|h| h.menu)
    }

    /// `id` をどちらかの手で掴んでいる。
    pub fn grabbed(&self, id: PanelId) -> bool {
        self.hands
            .values()
            .any(|h| h.dragging.is_some_and(|d| d.0 == id))
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

    /// 掴んで動かせないようにする（`false` で戻す）。レーザーでの操作はそのまま使える。
    pub fn set_locked(&mut self, id: PanelId, locked: bool) {
        self.panels[id].locked = locked;
    }

    /// パネルの解像度（UI のピクセル数）を変える。1m あたりのピクセル数は保ち、VR 上の大きさも同じ割合で変える
    /// （ウィンドウを広げると、中身が縮まずにパネルが広がる）。中心の位置は変えない。
    pub fn set_resolution(&mut self, vr: &mut dyn VrBackend, id: PanelId, resolution: (u32, u32)) {
        let p = &mut self.panels[id];
        let (w, h) = (resolution.0.max(1), resolution.1.max(1));
        if p.resolution == (w, h) {
            return;
        }
        p.size = Vec2::new(
            p.size.x * w as f32 / p.resolution.0 as f32,
            p.size.y * h as f32 / p.resolution.1 as f32,
        );
        p.resolution = (w, h);
        vr.set_overlay_width(p.overlay, p.displayed_size().x);
    }

    /// パネルごとのオプトアウト。再生中に無効化した場合も、直ちに最終状態にする。
    pub fn set_animated(&mut self, vr: &mut dyn VrBackend, id: PanelId, animated: bool) {
        self.panels[id].animated = animated;
        if !animated {
            self.animations.remove(&id);
            let p = &mut self.panels[id];
            p.opacity = if p.closed { 0.0 } else { 1.0 };
            Self::present(vr, p, false);
        }
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
        };
        vr.set_overlay_sort(p.overlay, sort);
        // レーザーは長さ（幅）だけ変え、太さ（高さ）は作ったときのまま。
        if part == Pointer::Laser {
            vr.set_overlay_height(p.overlay, p.size.y);
        }
    }

    /// `hand` でドラッグしている間、その見た目（[`Pointer::Drag`] のパネル）を幅 `width_m` で出す。`None` で隠す。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.hands.entry(hand).or_default().drag = width_m;
    }

    /// パネルを閉じる。入力を即座に止め、最後の描画をフェードアウトしてから隠す。
    // ponytail: ランタイム側のオーバーレイは残る。数が問題になったら VrBackend に destroy を足す。
    pub fn close(&mut self, vr: &mut dyn VrBackend, id: PanelId) {
        let p = &mut self.panels[id];
        if p.closed {
            return;
        }
        p.closed = true;
        if p.animated && p.texture.is_some() && p.opacity > 0.0 {
            self.animations.insert(
                id,
                (
                    Tween::new(p.opacity, 0.0)
                        .duration(0.15)
                        .easing(Easing::EaseInCubic)
                        .build(),
                    Instant::now(),
                ),
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

    /// `panel` の描画済み UI フレームを VR ランタイムへ送る。
    /// テクスチャの大きさは `resolution`（ヒット判定用の論理ピクセル）と独立してよい。
    /// HiDPI では UI が `resolution × 拡大率` で描かれるため。
    pub fn submit(
        &mut self,
        vr: &mut dyn VrBackend,
        panel: PanelId,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "invalid RGBA buffer size"
        );
        let p = &mut self.panels[panel];
        if p.closed {
            return;
        }
        let entering = p.texture.is_none() && p.animated;
        if entering {
            p.opacity = 0.0;
        }
        p.texture = Some((width, height, rgba.to_vec()));
        Self::present(vr, p, true);
        if entering {
            // 初回の GPU 転送が終わってから時計を始める。
            self.animations.insert(
                panel,
                (
                    Tween::new(0.0, 1.0)
                        .duration(0.2)
                        .easing(Easing::EaseOutCubic)
                        .build(),
                    Instant::now(),
                ),
            );
        }
    }

    fn present(vr: &mut dyn VrBackend, p: &mut Panel, upload: bool) {
        if p.pointer.is_none() {
            vr.set_overlay_width(p.overlay, p.displayed_size().x);
        }
        let native = vr.set_overlay_alpha(p.overlay, p.opacity);
        if p.closed && p.opacity == 0.0 {
            vr.set_overlay_visible(p.overlay, false);
            vr.submit_overlay_pixels(p.overlay, 1, 1, &[0; 4]);
            p.texture = None;
            return;
        }
        if native && !upload {
            return;
        }
        if let Some((width, height, rgba)) = &p.texture {
            if native || p.opacity == 1.0 {
                vr.submit_overlay_pixels(p.overlay, *width, *height, rgba);
            } else {
                // 両 VR バックエンドの事前乗算アルファに合わせ、RGB も同じ割合で減らす。
                // 不透明度 API が無いランタイムだけ、画素の減衰で補う。
                let opacity = (p.opacity * 65536.0).round() as u32;
                let faded: Vec<u8> = rgba
                    .iter()
                    .map(|v| ((*v as u32 * opacity + 32768) >> 16) as u8)
                    .collect();
                vr.submit_overlay_pixels(p.overlay, *width, *height, &faded);
            }
        }
    }

    fn animate(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        self.animations.retain(|id, (tween, last)| {
            tween.update((now - *last).as_secs_f32());
            *last = now;
            let p = &mut self.panels[*id];
            p.opacity = tween.value();
            Self::present(vr, p, false);
            !tween.is_complete()
        });
    }

    pub fn tick(&mut self, vr: &mut dyn VrBackend) -> Vec<UiEvent> {
        let frame = vr.poll();
        self.tick_frame(vr, frame)
    }

    /// [`Self::tick`] の poll の後半（ランタイムの待ちとオーバーレイの処理を別々に測るため分けてある）。
    pub fn tick_frame(&mut self, vr: &mut dyn VrBackend, frame: Frame) -> Vec<UiEvent> {
        self.animate(vr, Instant::now());
        self.quit = frame.quit;
        self.hmd = frame.hmd;
        self.trackers = frame.trackers.iter().map(|(id, _)| id.clone()).collect();
        // 掴み中のパネルは手に従う（離したフレームで update_hand がオフセットを付け直す）。
        // 取り付け先が見失われたフレームは最後の姿勢のまま。
        let dragged: Vec<PanelId> = self
            .hands
            .values()
            .filter_map(|h| h.dragging)
            .map(|d| d.0)
            .collect();
        for (id, p) in self.panels.iter_mut().enumerate() {
            p.follow = None;
            if dragged.contains(&id) {
                continue;
            }
            p.follow = p.attached.clone();
            if let Some(pose) = p
                .attached
                .as_ref()
                .and_then(|(d, o)| Some(frame.pose(d)? * *o))
            {
                p.pose = pose;
            }
        }
        let mut events = Vec::new();
        for h in self.hands.values_mut() {
            h.ray = None;
        }
        for raw in &frame.controllers {
            let prev = self.inputs.get(&raw.hand).copied();
            let c = input::map(raw, prev.as_ref());
            self.inputs.insert(raw.hand, c);
            self.update_hand(&c, &frame, &mut events);
            let buttons = |c: &ControllerState| (c.select, c.grab, c.menu);
            if prev.is_none_or(|p| buttons(&p) != buttons(&c)) {
                logger::debug!(
                    "controller buttons",
                    hand = ?c.hand,
                    profile = raw.profile,
                    select = c.select,
                    grab = c.grab,
                    menu = c.menu,
                    hover = ?self.hands[&c.hand].hover,
                );
            }
        }
        // 見えなくなった手（電源が切れた・novr のマウスが画面の外へ出た）は、指していたパネルから離れる。
        for (hand, st) in &mut self.hands {
            if frame.controllers.iter().any(|c| c.hand == *hand) {
                continue;
            }
            if let Some(panel) = st.hover.take() {
                events.push(UiEvent {
                    panel,
                    hand: *hand,
                    event: PointerEvent::Leave,
                });
            }
            st.select = false;
        }
        self.place_pointers(vr);
        for p in &mut self.panels {
            if let Some((device, offset)) = &p.follow {
                let want = Some((Some(device.clone()), *offset));
                if p.sent == want {
                    continue;
                }
                if vr.set_overlay_pose_relative(p.overlay, device, *offset) {
                    p.sent = want;
                    continue;
                }
            }
            let want = Some((None, p.pose));
            if p.sent != want {
                vr.set_overlay_pose(p.overlay, p.pose);
                p.sent = want;
            }
        }
        events
    }

    fn update_hand(&mut self, c: &ControllerState, frame: &Frame, out: &mut Vec<UiEvent>) {
        // 模したデバイス（novr のマウス）は手ブレしない。抑えると遅れるだけで、ドラッグで離した位置がずれる。
        let smoothing = if frame.emulated {
            0.0
        } else {
            self.smoothing.unwrap_or(POINTER_SMOOTHING)
        };
        let st = self.hands.entry(c.hand).or_default();
        // 手ブレを抑えたレーザー。隠れていた後は追わずに手に合わせる。
        let aim = Pose::new(
            c.aim.position,
            st.drawn.map_or(c.aim.orientation, |q| {
                q.slerp(c.aim.orientation, 1.0 - smoothing)
            }),
        );
        let grab_started = c.grab && !st.grab_held;
        st.grab_held = c.grab;
        st.menu = c.menu;

        if let Some((id, offset, len)) = st.dragging {
            if c.grab {
                // スティックの上下で、掴んだ点をレーザーに沿って遠く・近くへ。
                let push = if c.scroll.y.abs() < PUSH_DEADZONE {
                    0.0
                } else {
                    c.scroll.y
                };
                let new_len = (len * (1.0 + PUSH * push)).clamp(GRAB_MIN, GRAB_MAX);
                let offset = offset + Vec3::NEG_Z * (new_len - len);
                st.dragging = Some((id, offset, new_len));
                // 手に付いて動き、頭の方を向く。頭を傾けてもパネルは傾けない（水平にスナップ）。
                // 目標へはなめらかに追いつく。
                let at = aim.transform_point(offset);
                let p = &mut self.panels[id];
                let target = Pose::new(
                    at,
                    facing(at, frame.hmd.position, Vec3::Y).unwrap_or(p.pose.orientation),
                );
                p.pose = approach(p.pose, target);
                st.ray = Some(Ray {
                    hand: c.aim,
                    aim,
                    len: new_len,
                    hit: None,
                });
                st.drawn = Some(aim.orientation);
                return;
            }
            st.dragging = None;
            // 取り付け中なら、離した位置を新しいオフセットにする。
            let p = &mut self.panels[id];
            if let Some((device, offset)) = &mut p.attached
                && let Some(dp) = frame.pose(device)
            {
                *offset = dp.inverse() * p.pose;
            }
        }

        // キャプチャが有効なら、押している間は押したパネルの面を指し続ける（はみ出しても離すまでそのパネルへ送る）。
        // 離したフレームも押していたパネルへ送り、Up を届ける。ドラッグ＆ドロップでほかのパネルへ運ぶときは外す（release_capture）。
        let captured = st
            .hover
            .filter(|&id| self.capture && !st.free && st.select && !self.panels[id].closed)
            .and_then(|id| plane_hit(&self.panels[id], &aim).map(|(pos, t)| (id, pos, t)));
        // そうでなければ、レーザー上で最も近いパネル。
        let hit = captured.or_else(|| {
            self.panels
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.closed && p.pointer.is_none())
                .filter_map(|(id, p)| hit_test(p, &aim).map(|(pos, t)| (id, pos, t)))
                .min_by(|a, b| a.2.total_cmp(&b.2))
        });
        // 何も指していなければレーザーは出さない。
        st.ray = hit.map(|(id, _, t)| Ray {
            hand: c.aim,
            aim,
            len: t,
            hit: Some((id, aim.position + aim.forward() * t)),
        });
        st.drawn = st.ray.map(|r| r.aim.orientation);
        // ロック中のパネルは掴めない（掴みボタンは無視して普通に指す）。
        let grab_started = grab_started && hit.is_some_and(|h| !self.panels[h.0].locked);

        let mut emit = |panel, event| {
            out.push(UiEvent {
                panel,
                hand: c.hand,
                event,
            })
        };
        let new_hover = hit.map(|h| h.0);
        if let Some(old) = st
            .hover
            .filter(|&old| Some(old) != new_hover || grab_started)
        {
            emit(old, PointerEvent::Leave);
        }

        let Some((id, pos, _)) = hit else {
            st.hover = None;
            st.select = false;
            return;
        };
        if grab_started {
            let t = hit.map_or(0.0, |h| h.2);
            let offset = aim.inverse().transform_point(self.panels[id].pose.position);
            st.dragging = Some((id, offset, t));
            st.hover = None;
            st.select = false;
            return;
        }

        emit(id, PointerEvent::Move { pos });
        if c.select != st.select {
            emit(
                id,
                if c.select {
                    PointerEvent::Down { pos }
                } else {
                    PointerEvent::Up { pos }
                },
            );
        }
        if c.scroll != Vec2::ZERO {
            emit(
                id,
                PointerEvent::Scroll {
                    pos,
                    delta: c.scroll,
                },
            );
        }
        st.hover = Some(id);
        st.select = c.select;
        if !c.select {
            st.free = false;
        }
    }
}

impl Overlay {
    /// レーザーとカーソルのパネルを、この tick のレーザーに合わせて置く・伸ばす・隠す。
    fn place_pointers(&mut self, vr: &mut dyn VrBackend) {
        for id in 0..self.panels.len() {
            let p = &self.panels[id];
            let Some((hand, part)) = p.pointer.filter(|_| !p.closed) else {
                continue;
            };
            let ray = self.hands.get(&hand).and_then(|h| h.ray);
            let drag = self.hands.get(&hand).and_then(|h| h.drag);
            let drawn = ray.map(|r| (r, r.aim, r.len));
            let placed = match (part, drawn) {
                (Pointer::Laser, Some((r, aim, len))) => {
                    // 横から見ると面が見えなくなるので、レーザーの軸回りに面を頭へ向ける。
                    let h = aim.inverse().transform_point(self.hmd.position);
                    let roll = Quat::from_rotation_z((-h.x).atan2(h.y));
                    let pose = aim * Pose::new(Vec3::new(0.0, 0.0, -len / 2.0), roll * LASER_ROT);
                    Some((r, pose, len))
                }
                (Pointer::Cursor, Some((r, aim, len))) => r.hit.map(|(target, _)| {
                    let pose = on_panel(&self.panels[target].pose, &aim, len, CURSOR_LIFT);
                    // 頭から見て同じ大きさに見えるよう、離れるほど大きくする（`size` は 1m 先での大きさ）。
                    let width = self.panels[id].size.x * pose.position.distance(self.hmd.position);
                    (r, pose, width)
                }),
                (Pointer::Drag, Some((r, aim, len))) => r.hit.zip(drag).map(|((target, _), w)| {
                    (
                        r,
                        on_panel(&self.panels[target].pose, &aim, len, DRAG_LIFT),
                        w,
                    )
                }),
                _ => None,
            }
            .map(|(r, pose, width)| {
                // 手からの相対で置き、ランタイムの追従でレーザーとカーソルがずれないようにする。
                self.panels[id].follow = Some((Device::Controller(hand), r.hand.inverse() * pose));
                (pose, width)
            });
            let p = &mut self.panels[id];
            let shown = match placed {
                Some((pose, width)) => {
                    p.pose = pose;
                    (true, width)
                }
                None => (false, p.shown.map_or(p.size.x, |s| s.1)),
            };
            if p.shown != Some(shown) {
                if p.shown.map(|s| s.0) != Some(shown.0) {
                    vr.set_overlay_visible(p.overlay, shown.0);
                }
                if p.shown.map(|s| s.1) != Some(shown.1) {
                    vr.set_overlay_width(p.overlay, shown.1);
                }
                p.shown = Some(shown);
            }
        }
    }
}

/// レーザーの先（`aim` の前方 `len`）で、パネルの面に沿わせてレーザーの来る側（裏から指していれば裏側）へ `lift` 浮かせた姿勢。
fn on_panel(panel: &Pose, aim: &Pose, len: f32, lift: f32) -> Pose {
    let point = aim.position + aim.forward() * len;
    let normal = panel.orientation * Vec3::Z;
    let side = (aim.position - point).dot(normal).signum();
    Pose::new(point + normal * side * lift, panel.orientation)
}

/// `from` から `to` へ [`SMOOTH`] だけ近づける。十分近ければ `to`。
fn approach(from: Pose, to: Pose) -> Pose {
    if from.position.distance(to.position) < SNAP
        && from.orientation.angle_between(to.orientation) < SNAP
    {
        return to;
    }
    Pose::new(
        from.position.lerp(to.position, SMOOTH),
        from.orientation.slerp(to.orientation, SMOOTH),
    )
}

/// `at` に置いて表面（+Z）を `eye` に向け、上（+Y）を `up` に寄せた向き。`up` と視線が平行なら `None`。
fn facing(at: Vec3, eye: Vec3, up: Vec3) -> Option<Quat> {
    let z = (eye - at).try_normalize()?;
    let x = up.cross(z).try_normalize()?;
    Some(Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)))
}

/// レイ（aim の前方）とパネルの交差判定。ヒットしたピクセルとレイ上の距離を返す。
fn hit_test(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    let (px, t) = plane_hit(p, aim)?;
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    ((0.0..=w).contains(&px.x) && (0.0..=h).contains(&px.y)).then_some((px, t))
}

/// レイとパネルを含む平面の交点（パネルの外でもよい）のピクセル座標とレイ上の距離。レイが面と平行か後ろなら `None`。
fn plane_hit(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    let normal = p.pose.orientation * Vec3::Z;
    let dir = aim.forward();
    let denom = dir.dot(normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (p.pose.position - aim.position).dot(normal) / denom;
    if t < 0.0 {
        return None;
    }
    let local = p.pose.inverse().transform_point(aim.position + dir * t);
    let size = p.displayed_size();
    let (w, h) = (p.resolution.0 as f32, p.resolution.1 as f32);
    Some((
        Vec2::new((local.x / size.x + 0.5) * w, (0.5 - local.y / size.y) * h),
        t,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::{Quat, RawController, mock::MockBackend};

    fn advance(ov: &mut Overlay, vr: &mut MockBackend, seconds: f32) {
        let now = Instant::now();
        for (_, last) in ov.animations.values_mut() {
            *last = now - std::time::Duration::from_secs_f32(seconds);
        }
        ov.animate(vr, now);
    }

    #[test]
    fn entrance_and_exit_reach_backend_without_new_ui_frames() {
        let (mut vr, mut ov) = setup();
        vr.unsupported_alpha = true;
        let pose = ov.panel(0).pose;
        ov.submit(&mut vr, 0, 2, 1, &[80, 40, 20, 128, 200, 100, 50, 255]);
        assert_eq!(vr.overlays[0].width_m, 0.75);
        assert_eq!(vr.overlays[0].texture.as_ref().unwrap().rgba, [0; 8]);
        advance(&mut ov, &mut vr, 0.1);
        assert!((vr.overlays[0].width_m - 0.96875).abs() < 1e-6);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [70, 35, 18, 112]
        );
        // UI の次の描画が届いても、インを最初からやり直さない。
        ov.submit(&mut vr, 0, 2, 1, &[80, 40, 20, 128, 200, 100, 50, 255]);
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.overlays[0].width_m, 1.0);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [80, 40, 20, 128]
        );
        assert!(ov.animations.is_empty());

        ov.close(&mut vr, 0);
        assert!(ov.panel(0).closed && vr.overlays[0].visible);
        advance(&mut ov, &mut vr, 0.075);
        assert!((vr.overlays[0].width_m - 0.96875).abs() < 1e-6);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [70, 35, 18, 112]
        );
        // 閉じた UI から遅れて届く描画は、アウトの絵を上書きしない。
        ov.submit(&mut vr, 0, 1, 1, &[255; 4]);
        advance(&mut ov, &mut vr, 0.075);
        assert!(!vr.overlays[0].visible);
        assert_eq!(vr.overlays[0].width_m, 0.75);
        assert_eq!(vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0), [0; 4]);
        assert!(ov.panel(0).texture.is_none() && ov.animations.is_empty());
        assert_eq!(ov.panel(0).pose, pose);
    }

    #[test]
    fn native_alpha_does_not_modify_or_resubmit_pixels() {
        let (mut vr, mut ov) = setup();
        ov.submit(&mut vr, 0, 1, 1, &[80, 40, 20, 128]);
        assert_eq!(vr.overlays[0].alpha, 0.0);
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.overlays[0].alpha, 0.875);
        assert_eq!(vr.overlays[0].pixel_submissions, 1);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [80, 40, 20, 128]
        );
        ov.submit(&mut vr, 0, 1, 1, &[40, 20, 10, 64]);
        assert_eq!(vr.overlays[0].alpha, 0.875);
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.overlays[0].alpha, 1.0);
        ov.close(&mut vr, 0);
        advance(&mut ov, &mut vr, 0.075);
        assert_eq!(vr.overlays[0].alpha, 0.875);
        assert_eq!(vr.overlays[0].pixel_submissions, 2);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [40, 20, 10, 64]
        );
        advance(&mut ov, &mut vr, 0.075);
        assert_eq!(vr.overlays[0].alpha, 0.0);
        assert!(!vr.overlays[0].visible);
    }

    #[test]
    fn close_interrupts_entrance_and_repeated_close_does_not_restart() {
        let (mut vr, mut ov) = setup();
        ov.submit(&mut vr, 0, 1, 1, &[255; 4]);
        advance(&mut ov, &mut vr, 0.05);
        let before = vr.overlays[0].width_m;
        ov.close(&mut vr, 0);
        assert_eq!(vr.overlays[0].width_m, before);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        assert!(ov.tick(&mut vr).is_empty());
        advance(&mut ov, &mut vr, 0.075);
        assert!(vr.overlays[0].width_m < before);
        ov.close(&mut vr, 0);
        advance(&mut ov, &mut vr, 0.075);
        assert!(!vr.overlays[0].visible && ov.animations.is_empty());
    }

    #[test]
    fn opt_out_is_per_panel_and_finishes_running_animation() {
        let (mut vr, mut ov) = setup();
        let other = ov
            .add_panel(&mut vr, "other", Pose::IDENTITY, Vec2::ONE, (100, 100))
            .unwrap();
        ov.set_animated(&mut vr, other, false);
        for id in [0, other] {
            ov.submit(&mut vr, id, 1, 1, &[255; 4]);
        }
        assert_eq!(vr.overlays[0].width_m, 0.75);
        assert_eq!(vr.overlays[other].width_m, 1.0);
        assert_eq!(
            vr.overlays[other].texture.as_ref().unwrap().pixel(0, 0),
            [255; 4]
        );
        ov.set_animated(&mut vr, 0, false);
        assert_eq!(vr.overlays[0].width_m, 1.0);
        assert!(ov.animations.is_empty());
        ov.close(&mut vr, other);
        assert!(!vr.overlays[other].visible);
        ov.set_animated(&mut vr, 0, true);
        ov.close(&mut vr, 0);
        assert!(vr.overlays[0].visible);
        ov.set_animated(&mut vr, 0, false);
        assert!(!vr.overlays[0].visible && ov.animations.is_empty());
    }

    #[test]
    fn scale_keeps_center_and_hit_coordinates_when_resized() {
        let (mut vr, mut ov) = setup();
        let pose = ov.panel(0).pose;
        ov.submit(&mut vr, 0, 1, 1, &[255; 4]);
        ov.set_resolution(&mut vr, 0, (200, 100));
        assert_eq!(vr.overlays[0].width_m, 1.5);
        let p = ov.panel(0);
        assert_eq!(p.pixel_to_world(Vec2::new(100.0, 50.0)), pose.position);
        assert_eq!(
            p.pixel_to_world(Vec2::ZERO),
            pose.transform_point(Vec3::new(-0.75, 0.375, 0.0))
        );
        let (px, _) = hit_test(p, &at(0.375, 0.0, 0.0)).unwrap();
        assert_eq!(px, Vec2::new(150.0, 50.0));
        assert!(hit_test(p, &at(0.8, 0.0, 0.0)).is_none());
        advance(&mut ov, &mut vr, 0.2);
        assert_eq!(vr.overlays[0].width_m, 2.0);
        assert_eq!(ov.panel(0).pose, pose);
    }

    #[test]
    fn tick_uses_elapsed_time_and_pointers_opt_out() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        for id in [laser, cursor] {
            assert!(!ov.panel(id).animated);
        }
        ov.submit(&mut vr, 0, 1, 1, &[255; 4]);
        ov.animations.get_mut(&0).unwrap().1 =
            Instant::now() - std::time::Duration::from_millis(250);
        ov.tick(&mut vr);
        assert_eq!(vr.overlays[0].width_m, 1.0);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [255; 4]
        );
        assert!(ov.animations.is_empty());
    }

    fn setup() -> (MockBackend, Overlay) {
        let mut vr = MockBackend::default();
        let mut ov = Overlay::new();
        let pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
        ov.add_panel(&mut vr, "main", pose, Vec2::new(1.0, 1.0), (100, 100))
            .unwrap();
        (vr, ov)
    }

    fn controller(vr: &mut MockBackend, aim: Pose, inputs: &[(&str, f32)]) {
        vr.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: "valve/index_controller".into(),
            aim,
            inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }];
    }

    /// 掴み移動がなめらかに追いつくまで、同じ入力のまま tick する。
    fn settle(vr: &mut MockBackend, ov: &mut Overlay) {
        for _ in 0..60 {
            ov.tick(vr);
        }
    }

    fn kinds(ev: &[UiEvent]) -> Vec<PointerEvent> {
        ev.iter().map(|e| e.event).collect()
    }

    #[test]
    fn laser_click_on_panel() {
        let (mut vr, mut ov) = setup();
        let center = Vec2::new(50.0, 50.0);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move { pos: center },
                PointerEvent::Down { pos: center }
            ]
        );
        controller(&mut vr, Pose::IDENTITY, &[]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move { pos: center },
                PointerEvent::Up { pos: center }
            ]
        );
    }

    #[test]
    fn hit_maps_to_top_left_pixels_and_leave() {
        let (mut vr, mut ov) = setup();
        controller(
            &mut vr,
            Pose::new(Vec3::new(-0.25, 0.25, 0.0), Quat::IDENTITY),
            &[],
        );
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(25.0, 25.0)
            }]
        );
        controller(
            &mut vr,
            Pose::new(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY),
            &[],
        );
        assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
    }

    #[test]
    fn emulated_pointer_is_not_smoothed() {
        let (mut vr, mut ov) = setup();
        vr.frame.emulated = true;
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        // 右へ向けると、手ブレ抑えなしでその場でヒット点が動く。
        let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.2));
        controller(&mut vr, aim, &[]);
        let PointerEvent::Move { pos } = ov.tick(&mut vr)[0].event else {
            panic!()
        };
        assert_eq!(Some(pos), hit_test(ov.panel(0), &aim).map(|h| h.0));
    }

    #[test]
    fn pressed_pointer_keeps_its_panel_past_the_edge() {
        let (mut vr, mut ov) = setup();
        ov.set_capture(true);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        // 押したまま右へはみ出しても、Leave にならずパネルの外の座標で Move が届く。
        let outside = Pose::new(Vec3::new(0.8, 0.0, 0.0), Quat::IDENTITY);
        controller(&mut vr, outside, &[("trigger/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(130.0, 50.0)
            }]
        );
        // 離すと外で Up、次の tick で離れる。
        controller(&mut vr, outside, &[]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move {
                    pos: Vec2::new(130.0, 50.0)
                },
                PointerEvent::Up {
                    pos: Vec2::new(130.0, 50.0)
                }
            ]
        );
        assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
    }

    #[test]
    fn in_view_estimates_from_head_direction() {
        let (mut vr, mut ov) = setup();
        ov.tick(&mut vr);
        assert!(ov.in_view(0), "1m straight ahead");
        // 真後ろを向くと見えない。
        vr.frame.hmd = Pose::new(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::PI));
        ov.tick(&mut vr);
        assert!(!ov.in_view(0));
    }

    #[test]
    fn released_capture_lets_the_pointer_move_to_another_panel() {
        let (mut vr, mut ov) = setup();
        ov.set_capture(true);
        let right = Pose::new(Vec3::new(1.2, 0.0, -1.0), Quat::IDENTITY);
        ov.add_panel(&mut vr, "right", right, Vec2::new(1.0, 1.0), (100, 100))
            .unwrap();
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        // ドラッグに切り替わった（キャプチャを外した）ので、押したまま隣のパネルへ移れる。
        ov.release_capture(Hand::Right);
        let over_right = Pose::new(Vec3::new(1.2, 0.0, 0.0), Quat::IDENTITY);
        controller(&mut vr, over_right, &[("trigger/value", 1.0)]);
        let ev = ov.tick(&mut vr);
        assert!(
            ev.iter()
                .any(|e| e.panel == 1 && matches!(e.event, PointerEvent::Move { .. })),
            "{ev:?}"
        );
    }

    #[test]
    fn lost_controller_leaves_the_panel_once() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        vr.frame.controllers.clear();
        assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
        assert!(ov.tick(&mut vr).is_empty());
    }

    #[test]
    fn resize_keeps_pixel_density_and_hit_mapping() {
        let (mut vr, mut ov) = setup();
        // 100px = 1m のパネルを 200x50 にすると 2m x 0.5m になる。
        ov.set_resolution(&mut vr, 0, (200, 50));
        assert_eq!(ov.panel(0).size, Vec2::new(2.0, 0.5));
        assert_eq!(vr.overlays[0].width_m, 2.0);
        // 中心から右へ 0.75m・上へ 0.125m は、新しい解像度で (175, 12.5)。
        controller(
            &mut vr,
            Pose::new(Vec3::new(0.75, 0.125, 0.0), Quat::IDENTITY),
            &[],
        );
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(175.0, 12.5)
            }]
        );
    }

    #[test]
    fn grab_moves_panel_and_syncs_to_vr() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        let moved = Pose::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
        controller(&mut vr, moved, &[("squeeze/value", 1.0)]);
        assert!(ov.tick(&mut vr).is_empty());
        settle(&mut vr, &mut ov);
        let expected = Vec3::new(1.0, 0.0, -1.0);
        assert!(ov.panel(0).pose.position.abs_diff_eq(expected, 1e-5));
        assert!(vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn pixel_to_world_inverts_hit_test() {
        let (_, ov) = setup();
        let p = ov.panel(0);
        let px = Vec2::new(10.0, 70.0);
        let target = p.pixel_to_world(px);
        let aim = Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_arc(Vec3::NEG_Z, target.normalize()),
        );
        let (hit, _) = hit_test(p, &aim).unwrap();
        assert!(hit.abs_diff_eq(px, 1e-3), "{hit}");
    }

    fn at(x: f32, y: f32, z: f32) -> Pose {
        Pose::new(Vec3::new(x, y, z), Quat::IDENTITY)
    }

    fn assert_at(ov: &Overlay, vr: &MockBackend, expected: Vec3) {
        let got = ov.panel(0).pose.position;
        assert!(got.abs_diff_eq(expected, 1e-5), "{got}");
        assert!(vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn attached_to_hmd_follows_head_including_rotation() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -0.5));
        vr.frame.hmd = Pose::new(
            Vec3::new(1.0, 1.5, 0.0),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        ov.tick(&mut vr);
        // 左を向いた頭の 0.5m 前方 = -X 方向。
        assert_at(&ov, &vr, Vec3::new(0.5, 1.5, 0.0));
        assert!(
            ov.panel(0)
                .pose
                .orientation
                .abs_diff_eq(vr.frame.hmd.orientation, 1e-5)
        );
    }

    #[test]
    fn attached_to_controller_follows_hand() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Controller(Hand::Right), at(0.0, 0.1, 0.0));
        controller(&mut vr, at(0.3, -0.2, -0.4), &[]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.3, -0.1, -0.4));
    }

    #[test]
    fn attached_to_tracker_and_keeps_pose_when_lost() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Tracker("waist".into()), at(0.0, 0.0, -0.2));
        vr.frame.trackers = vec![("waist".into(), at(0.0, 1.0, 0.0))];
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
        vr.frame.trackers.clear();
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
    }

    #[test]
    fn detach_leaves_panel_in_place() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        vr.frame.hmd = at(2.0, 0.0, 0.0);
        ov.tick(&mut vr);
        ov.detach(0);
        vr.frame.hmd = at(5.0, 0.0, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(2.0, 0.0, -1.0));
    }

    #[test]
    fn dragging_attached_panel_reanchors_offset() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        // 右手で掴んで 1m 右へ動かして離す。
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
        // 頭が動けば、新しいオフセットのまま追従する。
        vr.frame.hmd = at(0.0, 0.5, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.5, -1.0));
    }

    #[test]
    fn locked_panel_ignores_grab_but_still_takes_pointer() {
        let (mut vr, mut ov) = setup();
        ov.set_locked(0, true);
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(50.0, 50.0)
            }]
        );
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn unlocked_panel_can_be_grabbed_again() {
        let (mut vr, mut ov) = setup();
        ov.set_locked(0, true);
        ov.set_locked(0, false);
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
    }

    #[test]
    fn locked_attached_panel_still_follows_device() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        ov.set_locked(0, true);
        vr.frame.hmd = at(0.0, 0.5, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 0.5, -1.0));
    }

    #[test]
    fn reports_trackers_menu_and_grab() {
        let (mut vr, mut ov) = setup();
        vr.frame.trackers = vec![("LHR-1".into(), Pose::IDENTITY)];
        controller(
            &mut vr,
            Pose::IDENTITY,
            &[("b/click", 1.0), ("squeeze/value", 1.0)],
        );
        ov.tick(&mut vr);
        assert_eq!(ov.trackers(), ["LHR-1"]);
        assert!(!ov.quit_requested());
        vr.frame.quit = true;
        ov.tick(&mut vr);
        assert!(ov.quit_requested());
        vr.frame.quit = false;
        assert!(ov.menu_held() && ov.grabbed(0));
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(!ov.menu_held() && !ov.grabbed(0));
    }

    #[test]
    fn laser_rot_lays_texture_along_aim_facing_up() {
        assert!((LASER_ROT * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!((LASER_ROT * Vec3::Z).abs_diff_eq(Vec3::Y, 1e-6));
    }

    /// パネル（z = -1）の前にレーザーとカーソルを置き、右手で中心を指す。
    fn with_pointers() -> (MockBackend, Overlay, PanelId, PanelId) {
        let (mut vr, mut ov) = setup();
        let mut add = |ov: &mut Overlay, part| {
            let id = ov
                .add_panel(&mut vr, "p", Pose::IDENTITY, Vec2::new(0.02, 0.02), (8, 8))
                .unwrap();
            ov.set_pointer(&mut vr, id, Hand::Right, part);
            id
        };
        let laser = add(&mut ov, Pointer::Laser);
        let cursor = add(&mut ov, Pointer::Cursor);
        (vr, ov, laser, cursor)
    }

    #[test]
    fn laser_ends_at_hit_and_cursor_floats_in_front_of_panel() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        // ポインタのパネルはレーザーの手前にあってもヒットしない。
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(50.0, 50.0)
            }]
        );
        let l = &vr.overlays[laser];
        assert!(l.visible && (l.width_m - 1.0).abs() < 1e-5);
        assert!(l.pose.position.abs_diff_eq(Vec3::new(0.0, 0.0, -0.5), 1e-5));
        let c = &vr.overlays[cursor];
        assert!(c.visible);
        assert!(
            c.pose
                .position
                .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + CURSOR_LIFT), 1e-5)
        );
        // カーソルはパネル（0）とレーザーより手前に描く。
        assert!(c.sort > l.sort && l.sort > vr.overlays[0].sort);
    }

    #[test]
    fn laser_turns_its_face_to_head_around_its_axis() {
        let (mut vr, mut ov, laser, _) = with_pointers();
        // 頭がレーザーの右横にある。
        vr.frame.hmd = at(1.0, 0.0, -0.5);
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        let q = vr.overlays[laser].pose.orientation;
        assert!((q * Vec3::Z).abs_diff_eq(Vec3::X, 1e-5), "{}", q * Vec3::Z);
        assert!((q * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-5));
    }

    fn move_x(ev: &[UiEvent]) -> f32 {
        let PointerEvent::Move { pos } = ev[0].event else {
            panic!()
        };
        pos.x
    }

    #[test]
    fn smoothing_steadies_hits_and_pointer_together() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        // 右へ向けると、ヒット（クリック位置）もポインタも途中から追う。
        let turned = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.3));
        let target = hit_test(ov.panel(0), &turned).unwrap().0.x;
        controller(&mut vr, turned, &[]);
        let x = move_x(&ov.tick(&mut vr));
        assert!(50.0 < x && x < target, "{x} {target}");
        // カーソルはヒット点（パネルの面の上）、レーザーの先端にある。
        let c = vr.overlays[cursor].pose.position;
        let hit = ov.panel(0).pixel_to_world(Vec2::new(x, 50.0));
        assert!(
            c.abs_diff_eq(hit + Vec3::Z * CURSOR_LIFT, 1e-4),
            "{c} {hit}"
        );
        let tip = vr.overlays[laser].pose.position * 2.0;
        assert!(tip.abs_diff_eq(hit, 1e-4), "{tip}");
        for _ in 0..200 {
            ov.tick(&mut vr);
        }
        assert!((move_x(&ov.tick(&mut vr)) - target).abs() < 0.01);
        // 抑えを切れば即座に当たる。
        ov.set_pointer_smoothing(0.0);
        controller(&mut vr, Pose::IDENTITY, &[]);
        assert_eq!(move_x(&ov.tick(&mut vr)), 50.0);
    }

    #[test]
    fn laser_keeps_its_thickness_and_cursor_its_apparent_size() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        assert_eq!(vr.overlays[laser].height_m, Some(0.02));
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        // 頭（原点）から 1m 先のカーソルは 1m 先での大きさ、2m 先なら 2 倍。
        let near = vr.overlays[cursor].width_m;
        assert!((near - 0.02 * (1.0 - CURSOR_LIFT)).abs() < 1e-5, "{near}");
        vr.frame.hmd = at(0.0, 0.0, 1.0);
        ov.tick(&mut vr);
        assert!(
            (vr.overlays[cursor].width_m - near * (2.0 - CURSOR_LIFT) / (1.0 - CURSOR_LIFT)).abs()
                < 1e-4
        );
    }

    #[test]
    fn drag_visual_shows_only_while_dragging_at_given_width() {
        let (mut vr, mut ov, _, cursor) = with_pointers();
        let drag = ov
            .add_panel(&mut vr, "d", Pose::IDENTITY, Vec2::splat(0.4), (400, 400))
            .unwrap();
        ov.set_pointer(&mut vr, drag, Hand::Right, Pointer::Drag);
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(!vr.overlays[drag].visible);
        ov.set_drag(Hand::Right, Some(0.25));
        ov.tick(&mut vr);
        let d = &vr.overlays[drag];
        assert!(d.visible && (d.width_m - 0.25).abs() < 1e-6);
        assert!(
            d.pose
                .position
                .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + DRAG_LIFT), 1e-5)
        );
        // パネル < ドラッグの見た目 < カーソル。
        assert!(vr.overlays[0].sort < d.sort && d.sort < vr.overlays[cursor].sort);
        ov.set_drag(Hand::Right, None);
        ov.tick(&mut vr);
        assert!(!vr.overlays[drag].visible);
    }

    #[test]
    fn missing_panel_hides_laser_and_cursor() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(vr.overlays[laser].visible && vr.overlays[cursor].visible);
        controller(&mut vr, at(5.0, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        assert!(!vr.overlays[laser].visible && !vr.overlays[cursor].visible);
        // 掴み中はパネルの外を向いていてもレーザーを出す。
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(0.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        vr.frame.controllers[0].aim.orientation = Quat::from_rotation_y(1.0);
        ov.tick(&mut vr);
        assert!(vr.overlays[laser].visible);
    }

    #[test]
    fn grabbed_panel_faces_head_and_stays_level() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        // 頭を傾けて横にずらし、手首もひねる。パネルは頭の方を向き、傾かない。
        vr.frame.hmd = Pose::new(Vec3::new(0.5, 0.3, 0.2), Quat::from_rotation_z(0.3));
        let aim = Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_y(0.5) * Quat::from_rotation_z(1.0),
        );
        controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        let p = ov.panel(0).pose;
        assert!(
            p.position
                .abs_diff_eq(aim.transform_point(Vec3::new(0.0, 0.0, -1.0)), 1e-5)
        );
        let normal = p.orientation * Vec3::Z;
        let to_head = (vr.frame.hmd.position - p.position).normalize();
        assert!(normal.abs_diff_eq(to_head, 1e-5), "{normal}");
        assert!((p.orientation * Vec3::X).y.abs() < 1e-5, "horizontal");
    }

    #[test]
    fn grab_glides_toward_hand() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        // 1 tick では途中まで。
        let x = ov.panel(0).pose.position.x;
        assert!(0.0 < x && x < 1.0, "{x}");
    }

    #[test]
    fn stick_pushes_and_pulls_grabbed_panel() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        let dist = |ov: &Overlay| -ov.panel(0).pose.position.z;
        let push = |y| [("squeeze/value", 1.0), ("thumbstick/y", y)];
        controller(&mut vr, Pose::IDENTITY, &push(1.0));
        settle(&mut vr, &mut ov);
        controller(&mut vr, Pose::IDENTITY, &push(0.0));
        settle(&mut vr, &mut ov);
        let far = dist(&ov);
        assert!(far > 1.5, "{far}");
        // 遊びの中では動かない。
        controller(&mut vr, Pose::IDENTITY, &push(0.1));
        settle(&mut vr, &mut ov);
        assert!((dist(&ov) - far).abs() < 1e-4);
        // 引き寄せても手より手前には来ない。
        controller(&mut vr, Pose::IDENTITY, &push(-1.0));
        for _ in 0..1000 {
            ov.tick(&mut vr);
        }
        assert!((dist(&ov) - GRAB_MIN).abs() < 1e-4, "{}", dist(&ov));
    }

    #[test]
    fn stick_moves_grabbed_point_along_the_laser() {
        let (mut vr, mut ov) = setup();
        ov.set_pointer_smoothing(0.0);
        // パネルの右寄り（中心から 0.3m）を斜めに掴み、押し出す。
        let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.3f32.atan()));
        controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        let before = ov.panel(0).pose.position;
        controller(
            &mut vr,
            aim,
            &[("squeeze/value", 1.0), ("thumbstick/y", 1.0)],
        );
        for _ in 0..20 {
            ov.tick(&mut vr);
        }
        controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        // 中心（= 掴んだ点）はレーザーと平行に、手から遠ざかる向きへ動く。
        let moved = ov.panel(0).pose.position - before;
        assert!(moved.length() > 0.3);
        assert!(
            moved.normalize().abs_diff_eq(aim.forward(), 1e-4),
            "{moved}"
        );
    }

    #[test]
    fn facing_points_surface_at_eye() {
        let q = facing(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, Vec3::Y).unwrap();
        assert!(q.abs_diff_eq(Quat::IDENTITY, 1e-6));
        assert!(facing(Vec3::ZERO, Vec3::Y, Vec3::Y).is_none());
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut vr, mut ov) = setup();
        ov.set_animated(&mut vr, 0, false);
        let mut rgba = vec![0; 200 * 200 * 4];
        rgba[..4].copy_from_slice(&[1, 2, 3, 4]);
        ov.submit(&mut vr, 0, 200, 200, &rgba);
        let tex = vr.overlays[0].texture.as_ref().unwrap();
        assert_eq!((tex.width, tex.height), (200, 200));
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }
}
