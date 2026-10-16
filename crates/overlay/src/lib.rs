//! Layer 3: オーバーレイの土台。3D 空間上のパネルを持ち、各コントローラのレーザーとの交差を判定し、
//! ヒットを UI 層向けの 2D ポインタイベントに変換する。パネルを掴んで動かす操作も扱う。
//! パネルはデバイス（HMD・コントローラ・トラッカー）に取り付けて追従させられる。
//! [`Overlay::tick`] が下位パイプライン1フレーム分: Subsystems の共通フレーム → 取り付け先への追従 → ヒット判定 → イベント。

use std::collections::HashMap;
use std::time::Instant;

use design::animation::{Animation, Motion};
use subsystems::{Frame, Subsystems, rendering::Surface, vr};

use vr::{Device, Hand, Mat3, OverlayId, Pose, Quat, Vec2, Vec3, VrBackend, VrError};

pub use subsystems::ControllerState;

pub type PanelId = usize;

/// 親子関係（ダイアログ等）。子は親に追従し、子が開いている間の親は暗くなって入力を受けない（掴んで動かすことはできる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent {
    pub panel: PanelId,
    /// 子を掴んだとき、親も連れていく。`false` なら子だけが離れ、親との相対位置が付け直される。
    pub carry: bool,
    /// 子の外側（親・ほかのパネル・何もない所）を押したとき、子を閉じる。ブロックしたい子だけ `false`。
    pub dismiss: bool,
}

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
    /// 親（[`Overlay::set_parent`]）。取り付けより優先して、親の姿勢 * `offset` に追従する。
    pub parent: Option<Parent>,
    /// 親から見た相対姿勢。
    offset: Pose,
    /// 今の暗さ（0 = そのまま）と、向かっている先。
    dim: f32,
    dim_to: f32,
    /// 表示の進行度。サイズは 75%〜100%、不透明度は 0%〜100%。
    opacity: f32,
    surface: Surface,
    /// この tick で追従させるデバイスと相対姿勢。ランタイムに任せられればそうする。
    follow: Option<(Device, Pose)>,
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
    /// ツールチップ。カーソルの子で、カーソルと同じ点・同じ縮尺（頭から 1m 先での大きさ）に置き、カーソルより手前に描く。
    /// 中心がカーソルで、箱を上下どちらに描くかは [`Overlay::tooltip_anchor`] を見て上の層が決める。
    Tooltip,
}

/// ツールチップをカーソルの上下どちらに出すか。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Anchor {
    /// 上。手（レーザーの来る側）がカーソルの上に見えるとき（下から見上げている）。
    Above,
    /// 下。手が頭から見てカーソルの下にあるとき（ふだんの構え）。
    #[default]
    Below,
}

/// レーザーのパネルを作るときの長さ（m）。以降は毎 tick ヒット点までの長さに伸び縮みする。
pub const LASER_MAX: f32 = 1.5;
/// レーザーをヒット点の手前で止める長さ（m）。奥行きで並べるランタイムで、先の透明な部分がカーソルに重なって
/// カーソルを欠けさせないように、線とカーソルを離す（先は透明なので、見た目の長さはほとんど変わらない）。
const LASER_GAP: f32 = 0.03;
/// レーザーの見える長さ（m）の既定値と範囲。手元からこの長さで消える（カーソルが近ければその手前で消える）。
pub const LASER_LENGTH: f32 = 0.3;
pub const LASER_LENGTH_MIN: f32 = 0.05;
/// カーソルをパネル面からレーザーの来る側へ浮かせる距離（m）。描画順でもパネルより手前に出すが、
/// 奥行きで並べるランタイムでも同じ面に乗って Z ファイティングしないように離す。
/// パネル・ドラッグの見た目・カーソル・ツールチップの順に手前へ、どの間も [`LIFT_STEP`] ずつ均等に空ける。
const CURSOR_LIFT: f32 = LIFT_STEP * 2.0;
/// 面の上に重ねるものどうしの間隔（m）。広いと浮いて見え、狭いとちらつく（Z ファイティング）。
// ponytail: 固定の幅。実機でちらつくなら広げる。
const LIFT_STEP: f32 = 0.003;
/// レーザーのクアッドの向き: ローカル +X（テクスチャの右）を前方 -Z へ、面の法線 +Z を上 +Y へ。
const LASER_ROT: Quat = Quat::from_xyzw(-0.5, 0.5, 0.5, 0.5);
/// 見えていそうかの見積もり（[`Overlay::in_view`]）に使う視野の半角（ラジアン、70°。実機の視野より広めに取る）。
const VIEW_HALF_ANGLE: f32 = 1.221_730_5;
/// 描画順。パネルは 0。
const LASER_SORT: u32 = 1;
const DRAG_SORT: u32 = 2;
const CURSOR_SORT: u32 = 3;
const TOOLTIP_SORT: u32 = 4;
/// ツールチップをパネル面から浮かせる距離（m）。カーソルより手前。
const TOOLTIP_LIFT: f32 = LIFT_STEP * 3.0;
/// ツールチップの向きを変える目安: 頭から見た手の方向が、パネルの上下へ水平からこれ（sin。約 12°）より傾いたら切り替える。
/// 間は前の向きのまま（水平近くで行ったり来たりしないように）。
// ponytail: 固定の幅。切り替わりが敏感・鈍いと感じたらここを調整する。
const ANCHOR_BAND: f32 = 0.2;
/// ドラッグの見た目をパネル面から浮かせる距離（m）。カーソルより奥。
const DRAG_LIFT: f32 = LIFT_STEP;
/// 掴み移動で、目標へ 1/120 秒（[`TICK_MIN`]）に近づく割合。経過時間で換算するので、フレームが落ちても追いつく速さは変わらない。
/// 目標との差が [`SNAP`] 未満なら目標に合わせる。
const SMOOTH: f32 = 0.3;
const SNAP: f32 = 1e-5;
/// 子が開いている間の親の暗さ（黒との混ぜ具合）。子に目が行くよう、読める程度に落とす。
/// 暗転の時間は子のフェードイン（0.2 秒）・アウト（0.15 秒）と同じ。
const DIM: f32 = 0.45;
/// 手ブレ抑えの既定値: レーザーの向きが 1 tick ごとに前の向きを残す割合（0 = 抑えない）。
/// ヒット判定もこの向きで行うので、クリック位置も揺れない。
// ponytail: tick 単位なのでリフレッシュレートで効き方が変わる。気になれば経過時間で決める。
pub const POINTER_SMOOTHING: f32 = POINTER_SMOOTHING_MAX;
/// 手ブレ抑えの上限。1 だとレーザーが動かなくなる。
pub const POINTER_SMOOTHING_MAX: f32 = 0.998;
/// 手ブレ抑えを緩める角度（度）の既定値と範囲。抑えたレーザーが手の向きからこれだけ遅れると、抑えが半分になる。
/// 止めているときはしっかり抑え、大きく動かすときは遅れすぎないように。小さいと手ブレの揺れだけで抑えが崩れる
/// （強い抑えほど遅れがたまるので、抑えを強めても効かなくなる）。
pub const POINTER_RELEASE_DEG: f32 = 11.25;
pub const POINTER_RELEASE_DEG_MIN: f32 = 0.5;
pub const POINTER_RELEASE_DEG_MAX: f32 = 45.0;
/// トリガーを引き切る直前（押下になる直前）の手ブレ抑え。引き始めからこれへ向けて強める。
/// 押下と判定される前の、トリガーを引く動きでクリック位置がずれないように（押した後は [`CLICK_SLOP`]）。
const TRIGGER_STEADY: f32 = 0.97;
/// 押してから、押した点がレーザーからこの角度（ラジアン、2°）ずれるまでは押した位置に留める。
/// トリガーを引く・離すときの手ブレでクリックが外れたりスワイプになったりしないように。
// ponytail: 固定値。きつい・ゆるいと感じたらここを調整する。
const CLICK_SLOP: f32 = 0.035;
/// 掴み中のスティックを倒し切ったとき、距離が変わる速さ（m/s。距離によらず一定）と、効き始める倒し量。
const PUSH: f32 = 2.0;
const PUSH_DEADZONE: f32 = 0.2;
/// 掴んだパネルを手から離せる距離（m）。
const GRAB_MIN: f32 = 0.15;
const GRAB_MAX: f32 = 10.0;
/// 掴み中にトリガーを押しながら手を 1m 前へ出したときの拡大率の指数（e^SCALE_RATE 倍）。後ろへ引くと縮む。
const SCALE_RATE: f32 = 3.0;
/// パネルの幅の範囲（m）。
/// 掴み中の拡大縮小の入力（`ControllerState::resize`）を倒し切ったときの、1 tick あたりの拡大率の指数。
// ponytail: tick 単位なので速さはフレームレート次第（90Hz で毎秒約 2.5 倍）。テスト用の入力なので時間では割らない。
const RESIZE_STEP: f32 = 0.01;
const SCALE_MIN: f32 = 0.05;
const SCALE_MAX: f32 = 10.0;

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
        self.surface.id()
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
    /// 前の tick でトリガーを押していた（押した瞬間の判定用）。
    pressed: bool,
    /// 外側を押して子を閉じた押下。離すまで、その下にあるパネルへは何も送らない（親のボタンを押したことにしない）。
    swallow: bool,
    menu: bool,
    /// ドラッグ中のパネルと、レーザーから見たその中心、掴んだ点までのレーザー上の距離。
    dragging: Option<(PanelId, Vec3, f32)>,
    /// ドラッグの見た目の幅（m）。ドラッグ中だけ。
    drag: Option<f32>,
    /// 掴み中にトリガーで拡大縮小しているとき、前の tick の手の位置。
    scaling: Option<Vec3>,
    /// 前の tick に描いたレーザーの向き。隠れていたら `None`（次に出すときは追わずに合わせる）。
    drawn: Option<Quat>,
    /// この tick のレーザー。コントローラが見えていない・何も指していなければ `None`。
    ray: Option<Ray>,
    /// 押している間のキャプチャを外した（離すまで）。
    free: bool,
    /// 押した位置（ピクセル）とトラッキング空間での点。[`CLICK_SLOP`] を越えて動かすか離すまで。
    press: Option<(Vec2, Vec3)>,
    /// ツールチップを出す向き（毎 tick 見直す）。
    anchor: Anchor,
}

#[derive(Debug, Default)]
pub struct Overlay {
    panels: Vec<Panel>,
    hands: HashMap<Hand, HandState>,
    /// 最後の tick で見えていたトラッカーの ID（取り付け先の候補）。
    trackers: Vec<String>,
    quit: bool,
    /// 最後の tick の HMD の姿勢。
    hmd: Pose,
    /// 押している間のポインタのキャプチャを使うか（[`Overlay::set_capture`]）。
    capture: bool,
    /// 手ブレ抑え（[`POINTER_SMOOTHING`]）。`None` で既定値。
    smoothing: Option<f32>,
    /// レーザーの見える長さ（[`LASER_LENGTH`]）。`None` で既定値。
    laser_length: Option<f32>,
    /// 手ブレ抑えを緩める角度（度。[`POINTER_RELEASE_DEG`]）。`None` で既定値。
    release_deg: Option<f32>,
    /// この tick で手ブレ抑えが効いている（ポインタを手に追従させず、空間に置く）。
    steadied: bool,
    animations: HashMap<PanelId, (Animation, Instant)>,
    /// この tick で外側を押されて閉じる子。
    dismissed: Vec<PanelId>,
    /// 親の暗転。
    dims: HashMap<PanelId, (Animation, Instant)>,
    /// 前の tick の時刻と、そこからの経過（秒。[`TICK_MIN`]〜[`TICK_MAX`] に丸める）。
    last_tick: Option<Instant>,
    dt: f32,
}

/// 1 tick の経過の下限・上限（秒）。テストのように続けて tick しても 1 フレーム分は進め、止まった後に飛ばない。
const TICK_MIN: f32 = 1.0 / 120.0;
const TICK_MAX: f32 = 1.0 / 30.0;

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
        let surface = Surface::new(vr, key, size.x, pose)?;
        self.panels.push(Panel {
            pose,
            size,
            resolution,
            attached: None,
            locked: false,
            closed: false,
            pointer: None,
            animated: true,
            parent: None,
            offset: Pose::IDENTITY,
            dim: 0.0,
            dim_to: 0.0,
            opacity: 1.0,
            surface,
            follow: None,
        });
        Ok(self.panels.len() - 1)
    }

    /// パネルの数（閉じたものも含む。PanelId は 0..len）。
    pub fn len(&self) -> usize {
        self.panels.len()
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

    /// 手ブレ抑えの強さ（0 = 抑えない 〜 [`POINTER_SMOOTHING_MAX`]）。
    pub fn set_pointer_smoothing(&mut self, smoothing: f32) {
        self.smoothing = Some(smoothing.clamp(0.0, POINTER_SMOOTHING_MAX));
    }

    /// 今の手ブレ抑えの強さ（設定していなければ既定値 [`POINTER_SMOOTHING`]）。
    pub fn pointer_smoothing(&self) -> f32 {
        self.smoothing.unwrap_or(POINTER_SMOOTHING)
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

    /// パネルが頭の向きから見えていそうか（ランタイムが教えてくれないときの見積もり。視野を広めに取った円錐とパネルを囲む球で判定）。
    pub fn in_view(&self, id: PanelId) -> bool {
        let p = &self.panels[id];
        // 隠しているポインタのパネルは最後に出した所に残るので、そちらを向くたびに見えた扱いで描き直していた。
        if p.closed || p.surface.hidden() {
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

    /// `id` を `parent` の子にする（`None` で独立）。今の位置を保ち、以降は親に追従する。取り付けは外す。
    /// 自分自身・子孫・閉じたパネルは親にできない（何もしない）。
    pub fn set_parent(&mut self, id: PanelId, parent: Option<Parent>) {
        if let Some(p) = parent
            && (p.panel == id
                || self.panels[p.panel].closed
                || ancestors(&self.panels, p.panel).contains(&id))
        {
            return;
        }
        let at = parent.map(|p| self.panels[p.panel].pose);
        let q = &mut self.panels[id];
        q.parent = parent;
        if let Some(at) = at {
            q.attached = None;
            q.offset = at.inverse() * q.pose;
        }
    }

    /// 開いている子がいる（暗くして入力を止める）。
    fn has_child(&self, id: PanelId) -> bool {
        self.panels
            .iter()
            .any(|q| !q.closed && q.parent.is_some_and(|p| p.panel == id))
    }

    /// パネルをその姿勢へ置く（次の tick でランタイムへ送る）。取り付け中なら取り付け先に戻される。
    pub fn set_pose(&mut self, id: PanelId, pose: Pose) {
        self.panels[id].pose = pose;
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
            Pointer::Tooltip => TOOLTIP_SORT,
        };
        p.surface.set_sort(vr, sort);
        // レーザーは長さ（幅）だけ変え、太さ（高さ）は作ったときのまま。
        if part == Pointer::Laser {
            p.surface.set_height(vr, p.size.y);
        }
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
        // 子は親と一緒に閉じる。
        let kids: Vec<PanelId> = (0..self.panels.len())
            .filter(|&c| self.panels[c].parent.is_some_and(|q| q.panel == id))
            .collect();
        for c in kids {
            self.close(vr, c);
        }
        let p = &mut self.panels[id];
        if p.animated && p.surface.has_texture() && p.opacity > 0.0 {
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
        let entering = !p.surface.has_texture() && p.animated;
        if entering {
            p.opacity = 0.0;
        }
        p.surface.store(width, height, rgba);
        Self::present(vr, p, true);
        if entering {
            // 初回の GPU 転送が終わってから時計を始める。
            self.animations.insert(
                panel,
                (Animation::new(0.0, 1.0, Motion::Enter), Instant::now()),
            );
        }
    }

    fn present(vr: &mut dyn VrBackend, p: &mut Panel, upload: bool) {
        if p.surface.released() {
            return;
        }
        if p.pointer.is_none() {
            p.surface.set_width(vr, p.displayed_size().x);
        }
        if p.closed && p.opacity == 0.0 {
            p.surface.release(vr);
        } else {
            p.surface.present(vr, p.opacity, p.dim, upload);
        }
    }

    fn animate(&mut self, vr: &mut dyn VrBackend, now: Instant) {
        self.animations.retain(|id, (tween, last)| {
            let v = tween.advance((now - *last).as_secs_f32());
            *last = now;
            let p = &mut self.panels[*id];
            p.opacity = v;
            Self::present(vr, p, false);
            !tween.is_done()
        });
        self.dims.retain(|id, (tween, last)| {
            let v = tween.advance((now - *last).as_secs_f32());
            *last = now;
            let p = &mut self.panels[*id];
            p.dim = v;
            Self::present(vr, p, false);
            !tween.is_done()
        });
    }

    pub fn tick<B: VrBackend>(&mut self, subsystems: &mut Subsystems<B>) -> Vec<UiEvent> {
        let frame = subsystems.poll();
        self.tick_frame(&mut subsystems.backend, frame)
    }

    /// [`Self::tick`] の poll の後半（ランタイムの待ちとオーバーレイの処理を別々に測るため分けてある）。
    pub fn tick_frame(&mut self, vr: &mut dyn VrBackend, frame: Frame) -> Vec<UiEvent> {
        let now = Instant::now();
        self.animate(vr, now);
        self.dt = self
            .last_tick
            .map_or(TICK_MIN, |t| (now - t).as_secs_f32())
            .clamp(TICK_MIN, TICK_MAX);
        self.last_tick = Some(now);
        self.quit = frame.quit;
        self.hmd = frame.hmd;
        self.steadied = frame.stabilize_pointer && self.pointer_smoothing() > 0.0;
        self.trackers = frame.trackers.iter().map(|(id, _)| id.clone()).collect();
        // 掴み中のパネルは手に従う（離したフレームで update_hand がオフセットを付け直す）。
        // 取り付け先が見失われたフレームは最後の姿勢のまま。
        let mut dragged: Vec<PanelId> = self
            .hands
            .values()
            .filter_map(|h| h.dragging)
            .map(|d| d.0)
            .collect();
        // 子を掴んで親を連れていくときは、親も手に従う。
        for d in dragged.clone() {
            dragged.extend(carried(&self.panels, d));
        }
        for (id, p) in self.panels.iter_mut().enumerate() {
            p.follow = None;
            // 子は取り付けではなく親に追従する（下の settle_family）。
            if dragged.contains(&id) || p.parent.is_some() {
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
        for c in &frame.controllers {
            self.update_hand(c, &frame, &mut events);
            if let Some((id, ..)) = self.hands[&c.hand].dragging
                && self.hands[&c.hand].scaling.is_some()
            {
                let p = &self.panels[id];
                p.surface.set_width(vr, p.displayed_size().x);
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
        for id in std::mem::take(&mut self.dismissed) {
            self.close(vr, id);
        }
        self.settle_family(&dragged);
        let parents: Vec<bool> = (0..self.panels.len()).map(|i| self.has_child(i)).collect();
        let now = Instant::now();
        for (id, (p, parent)) in self.panels.iter_mut().zip(parents).enumerate() {
            let to = if parent { DIM } else { 0.0 };
            if p.dim_to == to {
                continue;
            }
            p.dim_to = to;
            if p.animated && !p.closed {
                // 戻るときも ease-out: 終わり際に急に明るくなると、子が消える瞬間と重なってちらつく。
                let tween = Animation::new(
                    p.dim,
                    to,
                    if parent {
                        Motion::Enter
                    } else {
                        Motion::Return
                    },
                );
                self.dims.insert(id, (tween, now));
            } else {
                self.dims.remove(&id);
                p.dim = to;
                Self::present(vr, p, false);
            }
        }
        self.place_pointers(vr);
        for p in &mut self.panels {
            p.surface.place(vr, p.pose, p.follow.as_ref());
        }
        events
    }

    fn update_hand(&mut self, c: &ControllerState, frame: &Frame, out: &mut Vec<UiEvent>) {
        // 手ブレのない入力は補正すると遅れるため、サブシステムが示す能力に従う。
        let base = if !frame.stabilize_pointer {
            0.0
        } else {
            self.pointer_smoothing()
        };
        let release = self.pointer_release().to_radians();
        let st = self.hands.entry(c.hand).or_default();
        // 手ブレを抑えたレーザー。隠れていた後は追わずに手に合わせる。
        let aim = Pose::new(
            c.aim.position,
            st.drawn.map_or(c.aim.orientation, |q| {
                let s = steadiness(base, q.angle_between(c.aim.orientation), release, c);
                q.slerp(c.aim.orientation, 1.0 - s)
            }),
        );
        let grab_started = c.grab && !st.grab_held;
        let press = c.select && !st.pressed;
        st.pressed = c.select;
        if !c.select {
            st.swallow = false;
        }
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
                let mut new_len = len + PUSH * push * self.dt;
                // トリガーを押しながら手を前後に動かすと拡大縮小。パネルは手に付いて動かず、その場に留める。
                let p = &mut self.panels[id];
                if c.select {
                    if let Some(last) = st.scaling {
                        let delta = (c.aim.position - last).dot(aim.forward());
                        let w = (p.size.x * (delta * SCALE_RATE).exp()).clamp(SCALE_MIN, SCALE_MAX);
                        p.size *= w / p.size.x;
                        new_len -= delta;
                    }
                    st.scaling = Some(c.aim.position);
                } else {
                    st.scaling = None;
                }
                // 拡大縮小の入力（novr の PageUp / PageDown）。その場で大きさだけ変える。
                if c.resize != 0.0 {
                    let w = (p.size.x * (c.resize * RESIZE_STEP).exp()).clamp(SCALE_MIN, SCALE_MAX);
                    p.size *= w / p.size.x;
                }
                let new_len = new_len.clamp(GRAB_MIN, GRAB_MAX);
                let offset = offset + Vec3::NEG_Z * (new_len - len);
                st.dragging = Some((id, offset, new_len));
                // 手に付いて動き、頭の方を向く。頭を傾けてもパネルは傾けない（水平にスナップ）。
                // 目標へはなめらかに追いつく。
                let at = aim.transform_point(offset);
                let target = Pose::new(
                    at,
                    facing(at, frame.hmd.position, Vec3::Y).unwrap_or(p.pose.orientation),
                );
                p.pose = approach(p.pose, target, self.dt);
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
            st.scaling = None;
            // 取り付け中なら、離した位置を新しいオフセットにする。
            for q in std::iter::once(id).chain(carried(&self.panels, id)) {
                let p = &mut self.panels[q];
                if let Some((device, offset)) = &mut p.attached
                    && let Some(dp) = frame.pose(device)
                {
                    *offset = dp.inverse() * p.pose;
                }
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
        // 押した瞬間、そこが子の外側ならその子を閉じる（`Parent::dismiss`）。
        if press {
            let on = hit.map(|h| h.0);
            for (i, q) in self.panels.iter().enumerate() {
                let inside = on.is_some_and(|x| x == i || ancestors(&self.panels, x).contains(&i));
                if !q.closed && !inside && q.parent.is_some_and(|p| p.dismiss) {
                    self.dismissed.push(i);
                    st.swallow = true;
                }
            }
        }
        // 何も指していなければレーザーは出さない。
        st.ray = hit.map(|(id, _, t)| Ray {
            hand: c.aim,
            aim,
            len: t,
            hit: Some((id, aim.position + aim.forward() * t)),
        });
        st.drawn = st.ray.map(|r| r.aim.orientation);
        // ロック中のパネルは掴めない（掴みボタンは無視して普通に指す）。
        let grab_started = grab_started
            && hit.is_some_and(|h| {
                std::iter::once(h.0)
                    .chain(carried(&self.panels, h.0))
                    .all(|q| !self.panels[q].locked)
            });

        let mut emit = |panel, event| {
            out.push(UiEvent {
                panel,
                hand: c.hand,
                event,
            })
        };
        // 子が開いている親は入力を受けない（掴むことはできる）。
        let blocked = |id| {
            self.panels
                .iter()
                .any(|q| !q.closed && q.parent.is_some_and(|p| p.panel == id))
        };
        let swallow = st.swallow;
        let new_hover = hit.map(|h| h.0).filter(|&id| !blocked(id) && !swallow);
        if let Some(old) = st
            .hover
            .filter(|&old| Some(old) != new_hover || grab_started)
        {
            emit(old, PointerEvent::Leave);
        }

        let Some((id, pos, t)) = hit else {
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
        if blocked(id) || swallow {
            st.hover = None;
            st.select = false;
            return;
        }

        // 押した点から少し揺れただけなら、離すまで押した位置を指していることにする。
        let pos = match st.press {
            Some((at, point))
                if st.select
                    && st.hover == Some(id)
                    && (point - aim.position).angle_between(aim.forward()) < CLICK_SLOP =>
            {
                at
            }
            _ => {
                st.press = None;
                pos
            }
        };
        emit(id, PointerEvent::Move { pos });
        if c.select != st.select {
            // 手ブレのある入力だけ、押した位置を保持する。
            st.press = (c.select && frame.stabilize_pointer)
                .then(|| (pos, aim.position + aim.forward() * t));
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

/// この tick の手ブレ抑え: 前の向きを残す割合。`moved` は前のレーザーから手の向きまでの角度。
/// 速く振るほど弱め（`release` ラジアン遅れると半分）、押下前にトリガーを引いている間は強める。
fn steadiness(base: f32, moved: f32, release: f32, c: &ControllerState) -> f32 {
    if base == 0.0 {
        return 0.0;
    }
    let s = base / (1.0 + moved / release);
    if c.select || c.grab {
        return s;
    }
    s.max(TRIGGER_STEADY * c.select_progress())
}

impl Overlay {
    /// 親子の姿勢を合わせる。掴まれている子は、親を連れていくなら親を子に合わせ、そうでなければ親との相対位置を付け直す。
    /// そのあと、掴まれていない子を親（浅い順）に合わせる。親が掴まれても取り付けで動いても、子はここで追従する。
    // ponytail: 親の拡大縮小では子の相対位置を変えない（子はその場の大きさのまま）。
    fn settle_family(&mut self, dragged: &[PanelId]) {
        for &id in dragged {
            let mut cur = id;
            while let Some(Parent { panel, carry, .. }) = self.panels[cur].parent {
                if carry {
                    let (pose, offset) = (self.panels[cur].pose, self.panels[cur].offset);
                    self.panels[panel].pose = pose * offset.inverse();
                } else {
                    self.panels[cur].offset =
                        self.panels[panel].pose.inverse() * self.panels[cur].pose;
                    break;
                }
                cur = panel;
            }
        }
        let mut kids: Vec<PanelId> = (0..self.panels.len())
            .filter(|&i| self.panels[i].parent.is_some() && !dragged.contains(&i))
            .collect();
        kids.sort_by_key(|&i| ancestors(&self.panels, i).len());
        for id in kids {
            if let Some(parent) = self.panels[id].parent {
                self.panels[id].pose = self.panels[parent.panel].pose * self.panels[id].offset;
            }
        }
    }

    /// レーザーとカーソルのパネルを、この tick のレーザーに合わせて置く・伸ばす・隠す。
    fn place_pointers(&mut self, vr: &mut dyn VrBackend) {
        // 向きは毎 tick 数回の内積で決まるので、指している間はいつも見直す。
        let eye = self.hmd.position;
        let laser_length = self.laser_length();
        let steadied = self.steadied;
        for h in self.hands.values_mut() {
            if let Some(r) = h.ray
                && let Some((target, point)) = r.hit
            {
                let up = self.panels[target].pose.orientation * Vec3::Y;
                h.anchor = anchor(h.anchor, eye, r.hand.position, point, up);
            }
        }
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
                    let len = (len - LASER_GAP).clamp(0.0, laser_length);
                    let pose = aim * Pose::new(Vec3::new(0.0, 0.0, -len / 2.0), roll * LASER_ROT);
                    Some((r, pose, len))
                }
                (Pointer::Cursor | Pointer::Tooltip, Some((r, aim, len))) => {
                    r.hit.map(|(target, _)| {
                        // ツールチップはカーソルの子: 同じ点に、同じ縮尺で、少しだけ手前に置く。
                        let lift = if part == Pointer::Cursor {
                            CURSOR_LIFT
                        } else {
                            TOOLTIP_LIFT
                        };
                        let pose = on_panel(&self.panels[target].pose, &aim, len, lift);
                        // 頭から見て同じ大きさに見えるよう、離れるほど大きくする（`size` は 1m 先での大きさ）。
                        // 距離はヒット点まで（浮かせた分は含めない）なので、カーソルとツールチップの縮尺はそろう。
                        let hit = aim.position + aim.forward() * len;
                        let width = self.panels[id].size.x * hit.distance(self.hmd.position);
                        (r, pose, width)
                    })
                }
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
                // レーザーは手からの相対で置き、ランタイムの追従で手元から離れないようにする。
                // カーソルなど面の上のものは、手ブレ抑えが効いていれば空間に置く: 手に追従させると、tick の間
                // （ゲームで tick が遅れるほど長い）はランタイムが手ブレどおりに動かし、次の tick で抑えた位置へ
                // 戻されてガタつく。抑えていなければ手に追従させ、レーザーの先とずれないようにする。
                self.panels[id].follow = (part == Pointer::Laser || !steadied)
                    .then(|| (Device::Controller(hand), r.hand.inverse() * pose));
                (pose, width)
            });
            let p = &mut self.panels[id];
            if let Some((pose, _)) = placed {
                p.pose = pose;
            }
            p.surface.show(
                vr,
                placed.is_some(),
                placed.map(|(_, width)| width),
                p.size.x,
            );
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

/// 頭 `eye` から点 `point` を見たとき、手 `hand` がパネルの上 `up` に対して上下どちらに見えるかでツールチップの向きを決める。
/// 手が下に見えれば（レーザーは下から来る）下に、上に見えれば上に出す（手の側）。どちらとも言えなければ `prev` のまま。
fn anchor(prev: Anchor, eye: Vec3, hand: Vec3, point: Vec3, up: Vec3) -> Anchor {
    let view = (point - eye).normalize_or_zero();
    let flat = |v: Vec3| v - view * v.dot(view);
    let (hand, up) = (flat(hand - point), flat(up).normalize_or_zero());
    let len = hand.length();
    // 手が視線の上にある（novr のように目から指す）と、レーザーは点にしか見えない。
    if len < 1e-4 {
        return prev;
    }
    let s = hand.dot(up) / len;
    if s < -ANCHOR_BAND {
        Anchor::Below
    } else if s > ANCHOR_BAND {
        Anchor::Above
    } else {
        prev
    }
}

/// `from` から `to` へ、経過 `dt` 秒の分（[`SMOOTH`]）だけ近づける。十分近ければ `to`。
fn approach(from: Pose, to: Pose, dt: f32) -> Pose {
    if from.position.distance(to.position) < SNAP
        && from.orientation.angle_between(to.orientation) < SNAP
    {
        return to;
    }
    let k = 1.0 - (1.0 - SMOOTH).powf(dt / TICK_MIN);
    Pose::new(
        from.position.lerp(to.position, k),
        from.orientation.slerp(to.orientation, k),
    )
}

/// `id` の親、その親…（近い順）。
fn ancestors(panels: &[Panel], id: PanelId) -> Vec<PanelId> {
    let mut v = Vec::new();
    let mut cur = id;
    while let Some(p) = panels[cur].parent {
        if v.contains(&p.panel) {
            break;
        }
        v.push(p.panel);
        cur = p.panel;
    }
    v
}

/// `id` を掴んだとき一緒に動く親たち（`carry` でつながっている間）。
fn carried(panels: &[Panel], id: PanelId) -> Vec<PanelId> {
    let mut v = Vec::new();
    let mut cur = id;
    while let Some(Parent {
        panel, carry: true, ..
    }) = panels[cur].parent
    {
        if v.contains(&panel) {
            break;
        }
        v.push(panel);
        cur = panel;
    }
    v
}

/// `at` に置いて表面（+Z）を `eye` に向け、上（+Y）を `up` に寄せた向き。`up` と視線が平行なら `None`。
fn facing(at: Vec3, eye: Vec3, up: Vec3) -> Option<Quat> {
    let z = (eye - at).try_normalize()?;
    let x = up.cross(z).try_normalize()?;
    Some(Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)))
}

/// レイ（aim の前方）とパネルの表面（+Z 側）の交差判定。ヒットしたピクセルとレイ上の距離を返す。裏からは当たらない。
fn hit_test(p: &Panel, aim: &Pose) -> Option<(Vec2, f32)> {
    if aim.forward().dot(p.pose.orientation * Vec3::Z) >= 0.0 {
        return None;
    }
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

    fn advance(ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, seconds: f32) {
        let now = Instant::now();
        for (_, last) in ov.animations.values_mut() {
            *last = now - std::time::Duration::from_secs_f32(seconds);
        }
        ov.animate(&mut vr.backend, now);
    }

    #[test]
    fn entrance_and_exit_reach_backend_without_new_ui_frames() {
        let (mut vr, mut ov) = setup();
        vr.backend.unsupported_alpha = true;
        let pose = ov.panel(0).pose;
        ov.submit(
            &mut vr.backend,
            0,
            2,
            1,
            &[80, 40, 20, 128, 200, 100, 50, 255],
        );
        assert_eq!(vr.backend.overlays[0].width_m, 0.75);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().rgba,
            [0; 8]
        );
        advance(&mut ov, &mut vr, 0.1);
        assert!((vr.backend.overlays[0].width_m - 0.96875).abs() < 1e-6);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [70, 35, 18, 112]
        );
        // UI の次の描画が届いても、インを最初からやり直さない。
        ov.submit(
            &mut vr.backend,
            0,
            2,
            1,
            &[80, 40, 20, 128, 200, 100, 50, 255],
        );
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.backend.overlays[0].width_m, 1.0);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [80, 40, 20, 128]
        );
        assert!(ov.animations.is_empty());

        ov.close(&mut vr.backend, 0);
        assert!(ov.panel(0).closed && vr.backend.overlays[0].visible);
        advance(&mut ov, &mut vr, 0.075);
        assert!((vr.backend.overlays[0].width_m - 0.96875).abs() < 1e-6);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [70, 35, 18, 112]
        );
        // 閉じた UI から遅れて届く描画は、アウトの絵を上書きしない。
        ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
        advance(&mut ov, &mut vr, 0.075);
        assert!(!vr.backend.overlays[0].visible);
        assert_eq!(vr.backend.overlays[0].width_m, 0.75);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [0; 4]
        );
        assert!(!ov.panel(0).surface.has_texture() && ov.animations.is_empty());
        assert_eq!(ov.panel(0).pose, pose);
    }

    #[test]
    fn native_alpha_does_not_modify_or_resubmit_pixels() {
        let (mut vr, mut ov) = setup();
        ov.submit(&mut vr.backend, 0, 1, 1, &[80, 40, 20, 128]);
        assert_eq!(vr.backend.overlays[0].alpha, 0.0);
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.backend.overlays[0].alpha, 0.875);
        assert_eq!(vr.backend.overlays[0].pixel_submissions, 1);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [80, 40, 20, 128]
        );
        ov.submit(&mut vr.backend, 0, 1, 1, &[40, 20, 10, 64]);
        assert_eq!(vr.backend.overlays[0].alpha, 0.875);
        advance(&mut ov, &mut vr, 0.1);
        assert_eq!(vr.backend.overlays[0].alpha, 1.0);
        ov.close(&mut vr.backend, 0);
        advance(&mut ov, &mut vr, 0.075);
        assert_eq!(vr.backend.overlays[0].alpha, 0.875);
        assert_eq!(vr.backend.overlays[0].pixel_submissions, 2);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [40, 20, 10, 64]
        );
        advance(&mut ov, &mut vr, 0.075);
        assert_eq!(vr.backend.overlays[0].alpha, 0.0);
        assert!(!vr.backend.overlays[0].visible);
    }

    #[test]
    fn close_interrupts_entrance_and_repeated_close_does_not_restart() {
        let (mut vr, mut ov) = setup();
        ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
        advance(&mut ov, &mut vr, 0.05);
        let before = vr.backend.overlays[0].width_m;
        ov.close(&mut vr.backend, 0);
        assert_eq!(vr.backend.overlays[0].width_m, before);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        assert!(ov.tick(&mut vr).is_empty());
        advance(&mut ov, &mut vr, 0.075);
        assert!(vr.backend.overlays[0].width_m < before);
        ov.close(&mut vr.backend, 0);
        advance(&mut ov, &mut vr, 0.075);
        assert!(!vr.backend.overlays[0].visible && ov.animations.is_empty());
    }

    #[test]
    fn opt_out_is_per_panel_and_finishes_running_animation() {
        let (mut vr, mut ov) = setup();
        let other = ov
            .add_panel(
                &mut vr.backend,
                "other",
                Pose::IDENTITY,
                Vec2::ONE,
                (100, 100),
            )
            .unwrap();
        ov.set_animated(&mut vr.backend, other, false);
        for id in [0, other] {
            ov.submit(&mut vr.backend, id, 1, 1, &[255; 4]);
        }
        assert_eq!(vr.backend.overlays[0].width_m, 0.75);
        assert_eq!(vr.backend.overlays[other].width_m, 1.0);
        assert_eq!(
            vr.backend.overlays[other]
                .texture
                .as_ref()
                .unwrap()
                .pixel(0, 0),
            [255; 4]
        );
        ov.set_animated(&mut vr.backend, 0, false);
        assert_eq!(vr.backend.overlays[0].width_m, 1.0);
        assert!(ov.animations.is_empty());
        ov.close(&mut vr.backend, other);
        assert!(!vr.backend.overlays[other].visible);
        ov.set_animated(&mut vr.backend, 0, true);
        ov.close(&mut vr.backend, 0);
        assert!(vr.backend.overlays[0].visible);
        ov.set_animated(&mut vr.backend, 0, false);
        assert!(!vr.backend.overlays[0].visible && ov.animations.is_empty());
    }

    #[test]
    fn scale_keeps_center_and_hit_coordinates_when_resized() {
        let (mut vr, mut ov) = setup();
        let pose = ov.panel(0).pose;
        ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
        ov.set_resolution(&mut vr.backend, 0, (200, 100));
        assert_eq!(vr.backend.overlays[0].width_m, 1.5);
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
        assert_eq!(vr.backend.overlays[0].width_m, 2.0);
        assert_eq!(ov.panel(0).pose, pose);
    }

    #[test]
    fn tick_uses_elapsed_time_and_pointers_opt_out() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        for id in [laser, cursor] {
            assert!(!ov.panel(id).animated);
        }
        ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
        ov.animations.get_mut(&0).unwrap().1 =
            Instant::now() - std::time::Duration::from_millis(250);
        ov.tick(&mut vr);
        assert_eq!(vr.backend.overlays[0].width_m, 1.0);
        assert_eq!(
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
            [255; 4]
        );
        assert!(ov.animations.is_empty());
    }

    fn setup() -> (Subsystems<MockBackend>, Overlay) {
        let mut vr = Subsystems::with_backend(MockBackend::default());
        let mut ov = Overlay::new();
        let pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
        ov.add_panel(
            &mut vr.backend,
            "main",
            pose,
            Vec2::new(1.0, 1.0),
            (100, 100),
        )
        .unwrap();
        (vr, ov)
    }

    fn controller(vr: &mut Subsystems<MockBackend>, aim: Pose, inputs: &[(&str, f32)]) {
        vr.backend.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: "valve/index_controller".into(),
            aim,
            inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }];
    }

    /// 掴み移動がなめらかに追いつくまで、同じ入力のまま tick する。
    fn settle(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) {
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
    fn small_shake_while_pressed_keeps_the_press_point() {
        let (mut vr, mut ov) = setup();
        let center = Vec2::new(50.0, 50.0);
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        // 1cm（約 0.6°）ずれて離しても、押した所で離したことになる。
        let shaken = at(0.01, 0.0, 0.0);
        controller(&mut vr, shaken, &[("trigger/value", 1.0)]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move { pos: center }]
        );
        controller(&mut vr, shaken, &[]);
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [
                PointerEvent::Move { pos: center },
                PointerEvent::Up { pos: center }
            ]
        );
        // 離した後は手ブレのままの位置を指す。
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(51.0, 50.0)
            }]
        );
        // 大きく動かすと、押したままでも追う。
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(0.1, 0.0, 0.0), &[("trigger/value", 1.0)]);
        assert!((move_x(&ov.tick(&mut vr)) - 60.0).abs() < 1e-3);
    }

    #[test]
    fn panel_is_not_hit_from_behind() {
        let (mut vr, mut ov) = setup();
        // パネル（z = -1、表は +Z）の裏側（z = -2）から +Z へ向けて指しても、当たらない。
        let behind = Pose::new(
            Vec3::new(0.0, 0.0, -2.0),
            Quat::from_rotation_y(std::f32::consts::PI),
        );
        controller(&mut vr, behind, &[("trigger/value", 1.0)]);
        assert_eq!(kinds(&ov.tick(&mut vr)), []);
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
        vr.backend.frame.emulated = true;
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
        vr.backend.frame.hmd = Pose::new(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::PI));
        ov.tick(&mut vr);
        assert!(!ov.in_view(0));
    }

    #[test]
    fn released_capture_lets_the_pointer_move_to_another_panel() {
        let (mut vr, mut ov) = setup();
        ov.set_capture(true);
        let right = Pose::new(Vec3::new(1.2, 0.0, -1.0), Quat::IDENTITY);
        ov.add_panel(
            &mut vr.backend,
            "right",
            right,
            Vec2::new(1.0, 1.0),
            (100, 100),
        )
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
        vr.backend.frame.controllers.clear();
        assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
        assert!(ov.tick(&mut vr).is_empty());
    }

    #[test]
    fn resize_keeps_pixel_density_and_hit_mapping() {
        let (mut vr, mut ov) = setup();
        // 100px = 1m のパネルを 200x50 にすると 2m x 0.5m になる。
        ov.set_resolution(&mut vr.backend, 0, (200, 50));
        assert_eq!(ov.panel(0).size, Vec2::new(2.0, 0.5));
        assert_eq!(vr.backend.overlays[0].width_m, 2.0);
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
        assert!(
            vr.backend.overlays[0]
                .pose
                .position
                .abs_diff_eq(expected, 1e-5)
        );
    }

    /// 親（0 番）の手前に小さな子を置く。
    fn with_child(carry: bool) -> (Subsystems<MockBackend>, Overlay) {
        let (mut vr, mut ov) = setup();
        let child = ov
            .add_panel(
                &mut vr.backend,
                "child",
                at(0.0, 0.0, -0.9),
                Vec2::new(0.4, 0.4),
                (40, 40),
            )
            .unwrap();
        ov.set_parent(
            child,
            Some(Parent {
                panel: 0,
                carry,
                dismiss: false,
            }),
        );
        (vr, ov)
    }

    #[test]
    fn child_follows_its_parent() {
        let (mut vr, mut ov) = with_child(false);
        ov.set_pose(0, at(1.0, 2.0, -1.0));
        ov.tick(&mut vr);
        assert!(
            ov.panel(1)
                .pose
                .position
                .abs_diff_eq(Vec3::new(1.0, 2.0, -0.9), 1e-5)
        );
        assert!(
            vr.backend.overlays[1]
                .pose
                .position
                .abs_diff_eq(Vec3::new(1.0, 2.0, -0.9), 1e-5)
        );
        // 取り付けた親にも付いてくる。
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        vr.backend.frame.hmd = at(3.0, 0.0, 0.0);
        ov.tick(&mut vr);
        assert!(
            ov.panel(1)
                .pose
                .position
                .abs_diff_eq(Vec3::new(3.0, 0.0, -0.9), 1e-5)
        );
    }

    #[test]
    fn grabbing_the_child_carries_the_parent_only_when_asked() {
        for carry in [true, false] {
            let (mut vr, mut ov) = with_child(carry);
            // 子（手前）を掴んで右へ運ぶ。
            controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
            ov.tick(&mut vr);
            controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
            settle(&mut vr, &mut ov);
            assert!(ov.panel(1).pose.position.x > 0.9);
            assert_eq!(ov.panel(0).pose.position.x > 0.9, carry, "carry = {carry}");
            // 離したあとも、親が動けば子は新しい相対位置のまま付いてくる。
            controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
            ov.tick(&mut vr);
            let (parent, child) = (ov.panel(0).pose, ov.panel(1).pose.position);
            ov.set_pose(0, Pose::new(parent.position + Vec3::Y, parent.orientation));
            ov.tick(&mut vr);
            assert!(ov.panel(1).pose.position.abs_diff_eq(child + Vec3::Y, 1e-4));
        }
    }

    #[test]
    fn parent_is_dimmed_and_blocked_while_a_child_is_open() {
        let (mut vr, mut ov) = with_child(false);
        // ランタイムが明るさを掛けられないときは、画素を暗くして送る。
        vr.backend.unsupported_tint = true;
        ov.set_animated(&mut vr.backend, 0, false);
        ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
        ov.tick(&mut vr);
        let pixel = |vr: &Subsystems<MockBackend>| {
            vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0)
        };
        assert_eq!(pixel(&vr), [110, 55, 28, 255]);
        // 子の外（親だけがある場所）を押しても、親には届かない。
        controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
        assert!(ov.tick(&mut vr).is_empty());
        controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        // 子が閉じると戻る。
        ov.close(&mut vr.backend, 1);
        ov.tick(&mut vr);
        assert_eq!(pixel(&vr), [200, 100, 50, 255]);
        controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
        assert!(!ov.tick(&mut vr).is_empty());
    }

    #[test]
    fn pressing_outside_the_child_closes_it_unless_blocked() {
        let press = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, x: f32| {
            controller(vr, at(x, 0.0, 0.0), &[]);
            ov.tick(vr);
            controller(vr, at(x, 0.0, 0.0), &[("trigger/value", 1.0)]);
            ov.tick(vr);
        };
        // 子の中を押しても閉じない。
        let (mut vr, mut ov) = with_child(false);
        ov.panels[1].parent.as_mut().unwrap().dismiss = true;
        press(&mut ov, &mut vr, 0.0);
        assert!(!ov.panel(1).closed);
        // 親の上でも、何もない所でも閉じる。
        for x in [0.45, 3.0] {
            let (mut vr, mut ov) = with_child(false);
            ov.panels[1].parent.as_mut().unwrap().dismiss = true;
            press(&mut ov, &mut vr, x);
            assert!(ov.panel(1).closed && !ov.panel(0).closed, "x = {x}");
        }
        // ブロックした子は閉じない。
        let (mut vr, mut ov) = with_child(false);
        press(&mut ov, &mut vr, 0.45);
        assert!(!ov.panel(1).closed);
        // 閉じた押下は、その下の親には届かない（押したままでも、離しても）。
        let (mut vr, mut ov) = with_child(false);
        ov.panels[1].parent.as_mut().unwrap().dismiss = true;
        let mut seen = Vec::new();
        controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
        seen.extend(ov.tick(&mut vr));
        for _ in 0..3 {
            controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
            seen.extend(ov.tick(&mut vr));
        }
        controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
        seen.extend(ov.tick(&mut vr));
        assert!(ov.panel(1).closed);
        assert!(
            !seen
                .iter()
                .any(|e| matches!(e.event, PointerEvent::Down { .. } | PointerEvent::Up { .. })),
            "{seen:?}"
        );
        // 離したあとは、親は普通に押せる。
        controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
        assert!(
            ov.tick(&mut vr)
                .iter()
                .any(|e| matches!(e.event, PointerEvent::Down { .. }))
        );
    }

    #[test]
    fn pressing_a_nested_child_keeps_its_parent_dialog() {
        let (mut vr, mut ov) = with_child(false);
        ov.panels[1].parent.as_mut().unwrap().dismiss = true;
        let inner = ov
            .add_panel(
                &mut vr.backend,
                "inner",
                at(0.0, 0.0, -0.8),
                Vec2::new(0.2, 0.2),
                (20, 20),
            )
            .unwrap();
        ov.set_parent(
            inner,
            Some(Parent {
                panel: 1,
                carry: false,
                dismiss: true,
            }),
        );
        controller(&mut vr, at(0.0, 0.0, 0.0), &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        assert!(!ov.panel(1).closed && !ov.panel(inner).closed);
        // 外側の子を押すと内側の子だけが閉じる。
        controller(&mut vr, at(0.15, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        controller(&mut vr, at(0.15, 0.0, 0.0), &[("trigger/value", 1.0)]);
        ov.tick(&mut vr);
        assert!(ov.panel(inner).closed && !ov.panel(1).closed);
    }

    #[test]
    fn dimming_fades_in_and_out_like_the_panels() {
        let (mut vr, mut ov) = with_child(false);
        ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
        ov.animations.clear();
        ov.tick(&mut vr);
        let dim = |ov: &Overlay| ov.panel(0).dim;
        assert_eq!(dim(&ov), 0.0);
        let step = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, secs: f32| {
            let now = Instant::now();
            for (_, last) in ov.dims.values_mut() {
                *last = now - std::time::Duration::from_secs_f32(secs);
            }
            ov.animate(&mut vr.backend, now);
        };
        step(&mut ov, &mut vr, 0.1);
        assert!(0.0 < dim(&ov) && dim(&ov) < DIM, "{}", dim(&ov));
        step(&mut ov, &mut vr, 0.1);
        assert_eq!(dim(&ov), DIM);
        assert!(ov.dims.is_empty());
        ov.close(&mut vr.backend, 1);
        ov.tick(&mut vr);
        step(&mut ov, &mut vr, 0.075);
        assert!(0.0 < dim(&ov) && dim(&ov) < DIM);
        step(&mut ov, &mut vr, 0.075);
        assert_eq!(dim(&ov), 0.0);
    }

    #[test]
    fn closing_the_parent_closes_its_children_and_cycles_are_refused() {
        let (mut vr, mut ov) = with_child(false);
        ov.set_parent(
            0,
            Some(Parent {
                panel: 1,
                carry: false,
                dismiss: false,
            }),
        );
        assert_eq!(
            ov.panel(0).parent,
            None,
            "a child cannot be its parent's parent"
        );
        ov.close(&mut vr.backend, 0);
        assert!(ov.panel(1).closed);
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

    fn assert_at(ov: &Overlay, vr: &Subsystems<MockBackend>, expected: Vec3) {
        let got = ov.panel(0).pose.position;
        assert!(got.abs_diff_eq(expected, 1e-5), "{got}");
        assert!(
            vr.backend.overlays[0]
                .pose
                .position
                .abs_diff_eq(expected, 1e-5)
        );
    }

    #[test]
    fn attached_to_hmd_follows_head_including_rotation() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -0.5));
        vr.backend.frame.hmd = Pose::new(
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
                .abs_diff_eq(vr.backend.frame.hmd.orientation, 1e-5)
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
        vr.backend.frame.trackers = vec![("waist".into(), at(0.0, 1.0, 0.0))];
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
        vr.backend.frame.trackers.clear();
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
    }

    #[test]
    fn detach_leaves_panel_in_place() {
        let (mut vr, mut ov) = setup();
        ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
        vr.backend.frame.hmd = at(2.0, 0.0, 0.0);
        ov.tick(&mut vr);
        ov.detach(0);
        vr.backend.frame.hmd = at(5.0, 0.0, 0.0);
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
        vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
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
        vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
        ov.tick(&mut vr);
        assert_at(&ov, &vr, Vec3::new(0.0, 0.5, -1.0));
    }

    #[test]
    fn reports_trackers_menu_and_grab() {
        let (mut vr, mut ov) = setup();
        vr.backend.frame.trackers = vec![("LHR-1".into(), Pose::IDENTITY)];
        controller(
            &mut vr,
            Pose::IDENTITY,
            &[("b/click", 1.0), ("squeeze/value", 1.0)],
        );
        ov.tick(&mut vr);
        assert_eq!(ov.trackers(), ["LHR-1"]);
        assert!(!ov.quit_requested());
        vr.backend.frame.quit = true;
        ov.tick(&mut vr);
        assert!(ov.quit_requested());
        vr.backend.frame.quit = false;
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

    #[test]
    fn tooltip_goes_on_the_hand_side_as_seen_from_the_head() {
        let (point, up) = (Vec3::new(0.0, 1.5, -1.0), Vec3::Y);
        let eye = Vec3::new(0.0, 1.6, 0.0);
        // 右手を腰の高さで構え、上から見下ろす: レーザーは下から来るので下。
        let low = Vec3::new(0.3, 1.0, -0.3);
        assert_eq!(anchor(Anchor::Above, eye, low, point, up), Anchor::Below);
        // 手を頭より高く掲げて見上げる: 上。
        let high = Vec3::new(0.3, 2.2, -0.3);
        assert_eq!(anchor(Anchor::Below, eye, high, point, up), Anchor::Above);
        // 手が真横（水平近く）なら前の向きのまま。
        let side = Vec3::new(0.6, 1.5, -0.6);
        assert_eq!(anchor(Anchor::Below, eye, side, point, up), Anchor::Below);
        assert_eq!(anchor(Anchor::Above, eye, side, point, up), Anchor::Above);
        // 目から指す（novr）: レーザーが見えないので前のまま。
        assert_eq!(anchor(Anchor::Below, eye, eye, point, up), Anchor::Below);
    }

    #[test]
    fn tooltip_follows_the_cursor_in_front_of_it() {
        let (mut vr, mut ov, _, cursor) = with_pointers();
        let tip = ov
            .add_panel(
                &mut vr.backend,
                "t",
                Pose::IDENTITY,
                Vec2::new(0.02, 0.02),
                (8, 8),
            )
            .unwrap();
        ov.set_pointer(&mut vr.backend, tip, Hand::Right, Pointer::Tooltip);
        // 手を目より下に置いて正面のパネルを指す。
        controller(
            &mut vr,
            Pose::new(Vec3::new(0.0, -0.4, 0.0), Quat::IDENTITY),
            &[],
        );
        ov.tick(&mut vr);
        let (c, t) = (&vr.backend.overlays[cursor], &vr.backend.overlays[tip]);
        assert!(t.visible && c.visible);
        assert!((t.width_m - c.width_m).abs() < 1e-5, "同じ縮尺");
        let (pc, pt) = (ov.panel(cursor).pose.position, ov.panel(tip).pose.position);
        assert!((pc.x - pt.x).abs() < 1e-5 && (pc.y - pt.y).abs() < 1e-5);
        assert!(pt.z > pc.z, "カーソルより手前");
        assert_eq!(ov.tooltip_anchor(Hand::Right), Anchor::Below);
    }

    /// パネル（z = -1）の前にレーザーとカーソルを置き、右手で中心を指す。
    fn with_pointers() -> (Subsystems<MockBackend>, Overlay, PanelId, PanelId) {
        let (mut vr, mut ov) = setup();
        let mut add = |ov: &mut Overlay, part| {
            let id = ov
                .add_panel(
                    &mut vr.backend,
                    "p",
                    Pose::IDENTITY,
                    Vec2::new(0.02, 0.02),
                    (8, 8),
                )
                .unwrap();
            ov.set_pointer(&mut vr.backend, id, Hand::Right, part);
            id
        };
        let laser = add(&mut ov, Pointer::Laser);
        let cursor = add(&mut ov, Pointer::Cursor);
        (vr, ov, laser, cursor)
    }

    /// 隠したポインタは最後に出した所（目の前）に残っても見えない扱い（描き直さない）。
    #[test]
    fn hidden_pointer_is_not_in_view() {
        let (mut vr, mut ov, _, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(ov.in_view(cursor));
        vr.backend.frame.controllers.clear();
        ov.tick(&mut vr);
        assert!(!ov.in_view(cursor));
    }

    #[test]
    fn laser_stops_short_of_hit_and_cursor_floats_in_front_of_panel() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        // ポインタのパネルはレーザーの手前にあってもヒットしない。
        assert_eq!(
            kinds(&ov.tick(&mut vr)),
            [PointerEvent::Move {
                pos: Vec2::new(50.0, 50.0)
            }]
        );
        let l = &vr.backend.overlays[laser];
        let shown = (1.0 - LASER_GAP).min(LASER_LENGTH);
        assert!(l.visible && (l.width_m - shown).abs() < 1e-5);
        assert!(
            l.pose
                .position
                .abs_diff_eq(Vec3::new(0.0, 0.0, -shown / 2.0), 1e-5)
        );
        let c = &vr.backend.overlays[cursor];
        assert!(c.visible);
        assert!(
            c.pose
                .position
                .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + CURSOR_LIFT), 1e-5)
        );
        // カーソルはパネル（0）とレーザーより手前に描く。
        assert!(c.sort > l.sort && l.sort > vr.backend.overlays[0].sort);
    }

    #[test]
    fn laser_turns_its_face_to_head_around_its_axis() {
        let (mut vr, mut ov, laser, _) = with_pointers();
        // 頭がレーザーの右横にある。
        vr.backend.frame.hmd = at(1.0, 0.0, -0.5);
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        let q = vr.backend.overlays[laser].pose.orientation;
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
    fn pulling_the_trigger_steadies_and_fast_swings_follow() {
        // 同じだけ向きを変えたとき、1 tick で進んだ量（ピクセル）。
        let step = |turn: f32, trigger: f32| {
            let (mut vr, mut ov) = setup();
            // 仕組みを見るため、既定値によらない強さ・緩める角度（約 1.1°）にする。
            ov.set_pointer_smoothing(0.9);
            ov.set_pointer_release(0.02f32.to_degrees());
            controller(&mut vr, Pose::IDENTITY, &[("trigger/value", trigger)]);
            ov.tick(&mut vr);
            let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-turn));
            controller(&mut vr, aim, &[("trigger/value", trigger)]);
            (move_x(&ov.tick(&mut vr)) - 50.0) / (hit_test(ov.panel(0), &aim).unwrap().0.x - 50.0)
        };
        // 押下前にトリガーを引いている間は、手ブレをほとんど拾わない。
        let (free, pulling) = (step(0.01, 0.0), step(0.01, 0.55));
        assert!(pulling < free / 2.0, "{pulling} {free}");
        // 速く振るほど遅れない。
        let fast = step(0.3, 0.0);
        assert!(fast > 0.8 && fast > free, "{fast} {free}");
    }

    #[test]
    fn smoothing_steadies_hits_and_pointer_together() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        // 仕組みを見るため、既定値によらない強さ・緩める角度・線の長さにする。
        ov.set_pointer_smoothing(0.9);
        ov.set_pointer_release(0.02f32.to_degrees());
        ov.set_laser_length(LASER_MAX);
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        // 右へ向けると、ヒット（クリック位置）もポインタも途中から追う。
        let turned = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.3));
        let target = hit_test(ov.panel(0), &turned).unwrap().0.x;
        controller(&mut vr, turned, &[]);
        let x = move_x(&ov.tick(&mut vr));
        assert!(50.0 < x && x < target, "{x} {target}");
        // カーソルはヒット点（パネルの面の上）、レーザーの先端にある。
        let c = vr.backend.overlays[cursor].pose.position;
        let hit = ov.panel(0).pixel_to_world(Vec2::new(x, 50.0));
        assert!(
            c.abs_diff_eq(hit + Vec3::Z * CURSOR_LIFT, 1e-4),
            "{c} {hit}"
        );
        let tip = vr.backend.overlays[laser].pose.position * 2.0;
        assert!(
            tip.abs_diff_eq(hit - hit.normalize() * LASER_GAP, 1e-4),
            "{tip}"
        );
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
        assert_eq!(vr.backend.overlays[laser].height_m, Some(0.02));
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        // 頭（原点）から 1m 先のカーソルは 1m 先での大きさ、2m 先なら 2 倍。
        let near = vr.backend.overlays[cursor].width_m;
        assert!((near - 0.02).abs() < 1e-5, "{near}");
        vr.backend.frame.hmd = at(0.0, 0.0, 1.0);
        ov.tick(&mut vr);
        assert!((vr.backend.overlays[cursor].width_m - near * 2.0).abs() < 1e-4);
    }

    #[test]
    fn drag_visual_shows_only_while_dragging_at_given_width() {
        let (mut vr, mut ov, _, cursor) = with_pointers();
        let drag = ov
            .add_panel(
                &mut vr.backend,
                "d",
                Pose::IDENTITY,
                Vec2::splat(0.4),
                (400, 400),
            )
            .unwrap();
        ov.set_pointer(&mut vr.backend, drag, Hand::Right, Pointer::Drag);
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(!vr.backend.overlays[drag].visible);
        ov.set_drag(Hand::Right, Some(0.25));
        ov.tick(&mut vr);
        let d = &vr.backend.overlays[drag];
        assert!(d.visible && (d.width_m - 0.25).abs() < 1e-6);
        assert!(
            d.pose
                .position
                .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + DRAG_LIFT), 1e-5)
        );
        // パネル < ドラッグの見た目 < カーソル。
        assert!(vr.backend.overlays[0].sort < d.sort && d.sort < vr.backend.overlays[cursor].sort);
        ov.set_drag(Hand::Right, None);
        ov.tick(&mut vr);
        assert!(!vr.backend.overlays[drag].visible);
    }

    #[test]
    fn missing_panel_hides_laser_and_cursor() {
        let (mut vr, mut ov, laser, cursor) = with_pointers();
        controller(&mut vr, Pose::IDENTITY, &[]);
        ov.tick(&mut vr);
        assert!(vr.backend.overlays[laser].visible && vr.backend.overlays[cursor].visible);
        controller(&mut vr, at(5.0, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        assert!(!vr.backend.overlays[laser].visible && !vr.backend.overlays[cursor].visible);
        // 掴み中はパネルの外を向いていてもレーザーを出す。
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(0.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        vr.backend.frame.controllers[0].aim.orientation = Quat::from_rotation_y(1.0);
        ov.tick(&mut vr);
        assert!(vr.backend.overlays[laser].visible);
    }

    #[test]
    fn grabbed_panel_faces_head_and_stays_level() {
        let (mut vr, mut ov) = setup();
        // 手ブレ抑えが効くと掴んだパネルは抑えたレーザーに付いて遅れるので、ここでは向きだけを確かめる。
        ov.set_pointer_smoothing(0.0);
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        // 頭を傾けて横にずらし、手首もひねる。パネルは頭の方を向き、傾かない。
        vr.backend.frame.hmd = Pose::new(Vec3::new(0.5, 0.3, 0.2), Quat::from_rotation_z(0.3));
        let aim = Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_y(0.5) * Quat::from_rotation_z(1.0),
        );
        controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        let p = ov.panel(0).pose;
        assert!(
            p.position
                .abs_diff_eq(aim.transform_point(Vec3::new(0.0, 0.0, -1.0)), 1e-5),
            "{} {}",
            p.position,
            aim.transform_point(Vec3::new(0.0, 0.0, -1.0))
        );
        let normal = p.orientation * Vec3::Z;
        let to_head = (vr.backend.frame.hmd.position - p.position).normalize();
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
    fn trigger_and_move_scales_grabbed_panel_in_place() {
        let (mut vr, mut ov) = setup();
        ov.set_pointer_smoothing(0.0);
        let both = [("squeeze/value", 1.0), ("trigger/value", 1.0)];
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        let (w0, pos) = (ov.panel(0).size.x, ov.panel(0).pose.position);
        controller(&mut vr, Pose::IDENTITY, &both);
        ov.tick(&mut vr);
        // 前へ出すと大きく、その場に留まる。
        controller(&mut vr, at(0.0, 0.0, -0.2), &both);
        settle(&mut vr, &mut ov);
        let w1 = ov.panel(0).size.x;
        assert!((w1 / w0 - (0.2 * SCALE_RATE).exp()).abs() < 1e-3, "{w1}");
        assert!(ov.panel(0).pose.position.abs_diff_eq(pos, 1e-3));
        // 引くと縮む。
        controller(&mut vr, Pose::IDENTITY, &both);
        settle(&mut vr, &mut ov);
        assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
        // トリガーなしで動かしても大きさは変わらない。
        controller(&mut vr, at(0.0, 0.0, -0.2), &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
    }

    #[test]
    fn resize_input_scales_only_the_grabbed_panel() {
        let (mut vr, mut ov) = setup();
        const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
        let resize = |r| [("squeeze/value", 1.0), (RESIZE, r)];
        controller(&mut vr, Pose::IDENTITY, &resize(0.0));
        settle(&mut vr, &mut ov);
        let (w0, h0, pos) = (
            ov.panel(0).size.x,
            ov.panel(0).size.y,
            ov.panel(0).pose.position,
        );
        // 押している間、tick ごとに大きくなる。縦横比は保ち、その場に留まる。
        controller(&mut vr, Pose::IDENTITY, &resize(1.0));
        for _ in 0..10 {
            ov.tick(&mut vr);
        }
        let w1 = ov.panel(0).size.x;
        assert!((w1 / w0 - (10.0 * RESIZE_STEP).exp()).abs() < 1e-3, "{w1}");
        assert!((ov.panel(0).size.y / h0 - w1 / w0).abs() < 1e-4);
        assert!(ov.panel(0).pose.position.abs_diff_eq(pos, 1e-3));
        // 逆向きで同じだけ縮むと元に戻る。
        controller(&mut vr, Pose::IDENTITY, &resize(-1.0));
        for _ in 0..10 {
            ov.tick(&mut vr);
        }
        assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
        // 掴んでいなければ変わらない。
        controller(&mut vr, Pose::IDENTITY, &[]);
        settle(&mut vr, &mut ov);
        controller(&mut vr, Pose::IDENTITY, &[(RESIZE, 1.0)]);
        settle(&mut vr, &mut ov);
        assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
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

    /// 掴み移動の追いつきは経過時間で決まる: 2 tick 分の時間を 1 回で進めても同じ所に来る。
    #[test]
    fn approach_depends_on_elapsed_time_not_ticks() {
        let (from, to) = (Pose::IDENTITY, Pose::new(Vec3::X, Quat::IDENTITY));
        let twice = approach(approach(from, to, TICK_MIN), to, TICK_MIN);
        let once = approach(from, to, TICK_MIN * 2.0);
        assert!(once.position.abs_diff_eq(twice.position, 1e-6));
    }

    /// 奥行きの変化は距離によらず一定の速さ（近くでも遅くならない）。
    #[test]
    fn stick_push_is_linear_in_distance() {
        let (mut vr, mut ov) = setup();
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        let dist = |ov: &Overlay| -ov.panel(0).pose.position.z;
        let pull = |vr: &mut Subsystems<MockBackend>, ov: &mut Overlay| {
            controller(
                vr,
                Pose::IDENTITY,
                &[("squeeze/value", 1.0), ("thumbstick/y", -1.0)],
            );
            for _ in 0..10 {
                ov.tick(vr);
            }
            controller(vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
            settle(vr, ov);
        };
        let d0 = dist(&ov);
        pull(&mut vr, &mut ov);
        let d1 = dist(&ov);
        pull(&mut vr, &mut ov);
        let d2 = dist(&ov);
        assert!(((d0 - d1) - (d1 - d2)).abs() < 0.02, "{d0} {d1} {d2}");
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
        ov.set_animated(&mut vr.backend, 0, false);
        let mut rgba = vec![0; 200 * 200 * 4];
        rgba[..4].copy_from_slice(&[1, 2, 3, 4]);
        ov.submit(&mut vr.backend, 0, 200, 200, &rgba);
        let tex = vr.backend.overlays[0].texture.as_ref().unwrap();
        assert_eq!((tex.width, tex.height), (200, 200));
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }
}
