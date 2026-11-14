//! Layer 3: オーバーレイの土台。3D 空間上のパネルを持ち、各コントローラのレーザーとの交差を判定し、
//! ヒットを UI 層向けの 2D ポインタイベントに変換する。パネルを掴んで動かす操作も扱う。
//! パネルはデバイス（HMD・コントローラ・トラッカー）に取り付けて追従させられる。
//! [`Overlay::tick`] が下位パイプライン1フレーム分: Subsystems の共通フレーム → 取り付け先への追従 → ヒット判定 → イベント。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use animation::{Animation, Motion};
use design::PANEL_MARGIN;
use subsystems::{Frame, Subsystems, desktop::Cursor, rendering::Surface, vr};

use vr::{Device, Hand, HandSkeleton, Mat3, OverlayId, Pose, Quat, Vec2, Vec3, VrBackend, VrError};

mod lazy_follow;
pub use lazy_follow::LazyFollow;
pub use subsystems::ControllerState;

mod family;
mod frame;
mod geometry;
mod hand;
mod options;
mod panels;
mod pointers;
mod status;

use family::*;
use frame::*;
pub use geometry::anchored;
use geometry::*;
use pointers::*;

#[cfg(test)]
mod test_util;

pub type PanelId = usize;

/// 親子関係（ダイアログ等）。子は親に追従し、子が開いている間の親は暗くなって入力を受けない（掴んで動かすことはできる）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Parent {
    pub panel: PanelId,
    /// 子を掴んだとき、親も連れていく。`false` なら子だけが離れ、親との相対位置が付け直される。
    pub carry: bool,
    /// 子の外側（親・ほかのパネル・何もない所）を押したとき、子を閉じる。ブロックしたい子だけ `false`。
    pub dismiss: bool,
    /// 表示だけの子（統計など）。親の左上の角に重ねて置き、親を暗くせず入力も止めない。レーザーは素通りする。
    pub hud: bool,
    /// 親に固定された子（ダッシュボードのバーや、そこにはめた画面）。親を暗くせず入力も止めない。
    pub docked: bool,
    /// 親の上端の上に立てて置く（`docked` の子）。値は親から見て水平軸回りに起こす角度（ラジアン）。
    /// 子の下端に付くナビゲーションバーの分を空けて、子の地の下端を置く。子の大きさが変わっても毎 tick 置き直す。
    pub slot: Option<f32>,
}

impl Parent {
    /// 親を暗くし入力を止める子か（ダイアログなど。表示だけの子・固定された子は止めない）。
    fn blocks(&self) -> bool {
        !self.hud && !self.docked
    }
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
    /// 掴んで大きさを変えられる（[`Overlay::set_resizable`]。既定は変えられる）。
    pub resizable: bool,
    /// 取り付けたまま掴んで動かせる、取り付け先からの距離の上限（m。[`Overlay::set_reach`]）。`None` なら限らない。
    pub reach: Option<f32>,
    /// 掴んだとき、手で動かして頭へ向ける基準（このパネルから見た姿勢。[`Overlay::set_grab_anchor`]）。`None` ならパネルそのもの。
    pub grab_anchor: Option<Pose>,
    /// 閉じられた。ヒット判定の対象外で、PanelId は詰めずに欠番のまま残る。
    pub closed: bool,
    /// 隠した（[`Overlay::set_hidden`]）。閉じずにフェードアウトし、ヒット判定・暗転の対象外になる。
    pub hidden: bool,
    /// どの手のレーザーを描くパネルか。ヒット判定の対象外で、配置は毎 tick ここで決める。
    pub pointer: Option<(Hand, Pointer)>,
    /// このパネルのイン・アウトアニメーション。`Overlay::set_animated` で個別に無効化できる。
    pub animated: bool,
    /// 出ている間、空間全体を暗くする（[`Overlay::set_veil`]）。
    pub veil: bool,
    /// OS へのタッチを解放まで同じパネルへ届ける。
    capture: bool,
    /// その手で指している間、カーソルの代わりに出す OS のカーソル（[`Overlay::set_os_cursor`]）。
    pub os_cursor: Option<(Hand, Arc<Cursor>)>,
    /// 親（[`Overlay::set_parent`]）。取り付けより優先して、親の姿勢 * `offset` に追従する。
    pub parent: Option<Parent>,
    /// このパネルがナビゲーションバーになっている持ち主（[`Overlay::set_nav_bar`]）。持ち主の下端にくっつき、
    /// 頭から見た大きさが変わらないよう離れるほど大きくなる。自分のアニメーションは持たず、出入り（不透明度と 75%〜100% の大きさ）と
    /// 暗さは毎 tick 持ち主のものを写して一体で動く。持ち主と一緒に閉じ、持ち主が暗い間は押せない。
    pub nav_bar_of: Option<PanelId>,
    /// ナビゲーションバーの頭から 1m 先での大きさ（`size` は毎 tick これ * 距離になる）。
    nav_bar_size: Vec2,
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
    /// 頭の向きにゆるく追従する（[`Overlay::set_lazy_follow`]）。取り付けより優先する。
    pub lazy_follow: Option<LazyFollow>,
}

impl Panel {
    /// 掴める（ロックされておらず、頭にゆるく追従してもいない）。ゆるい追従は毎 tick 位置を決めるので、掴んで動かしても意味がない。
    pub fn grabbable(&self) -> bool {
        !self.locked && self.lazy_follow.is_none()
    }
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
/// 面の上に重ねるものどうしの間隔（m）。広いと浮いて見えて視差で狙いがずれ、狭いとちらつく（Z ファイティング）。
/// 1mm は 10m 先でも 24bit の深度で 8 段ほどの差がある。
// ponytail: 固定の幅。実機でちらつくなら広げる。
const LIFT_STEP: f32 = 0.001;
/// 描画順。一番奥が空間全体の暗転の幕（[`Overlay::set_veil`]）で、その上にパネル。
const VEIL_SORT: u32 = 0;
const PANEL_SORT: u32 = 1;
const LASER_SORT: u32 = 2;
const DRAG_SORT: u32 = 3;
const CURSOR_SORT: u32 = 4;
const TOOLTIP_SORT: u32 = 5;
/// 掴み移動で、目標へ 1/120 秒（[`TICK_MIN`]）に近づく割合。経過時間で換算するので、フレームが落ちても追いつく速さは変わらない。
/// 目標との差が [`SNAP`] 未満なら目標に合わせる。
const SMOOTH: f32 = 0.3;
const SNAP: f32 = 1e-5;
/// 子が開いている間の親の暗さ（黒との混ぜ具合）。子に目が行くよう、読める程度に落とす。
/// 暗転の時間は子のフェードイン（0.2 秒）・アウト（0.15 秒）と同じ。
const DIM: f32 = 0.45;
/// 親の上端に立てた子（[`Parent::slot`]）のナビゲーションバーの下端と、親の地の上端の間（m）。
pub const SLOT_GAP: f32 = 0.02;
/// 親の上端に立てた画面が入れ替わりで消えるとき、後ろへ下がる距離（m）。
const SLOT_RETREAT: f32 = 0.15;
/// 表示だけの子の重なり順。角は親の中心より頭から遠いので、距離で並べると親の裏に回って消える。
const HUD_SORT: u32 = LASER_SORT;
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
/// 押してから、押した点がレーザーからこの角度（ラジアン、2°）ずれるまでは押した位置に留める。
/// トリガーを引く・離すときの手ブレでクリックが外れたりスワイプになったりしないように。
// ponytail: 固定値。きつい・ゆるいと感じたらここを調整する。
const CLICK_SLOP: f32 = 0.035;
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
    Move {
        pos: Vec2,
    },
    Down {
        pos: Vec2,
    },
    Up {
        pos: Vec2,
    },
    DownButton {
        pos: Vec2,
        button: subsystems::input::ClickButton,
    },
    UpButton {
        pos: Vec2,
        button: subsystems::input::ClickButton,
    },
    Scroll {
        pos: Vec2,
        delta: Vec2,
    },
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
    button: subsystems::input::ClickButton,
    grab_held: bool,
    /// 前の tick でトリガーを押していた（押した瞬間の判定用）。
    pressed: bool,
    /// 外側を押して子を閉じた押下。離すまで、その下にあるパネルへは何も送らない（親のボタンを押したことにしない）。
    swallow: bool,
    menu: bool,
    /// 掴んで動かしているパネル・手から見た基準の位置・レーザーに沿った距離・基準から見たパネルの姿勢・大きさを変えてよいか。
    /// 基準（頭へ向けて手で動かすもの）はふつうパネルそのもの（`Pose::IDENTITY`）。[`grab_target`] が決める。
    dragging: Option<(PanelId, Vec3, f32, Pose, bool)>,
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

/// 位置のトランジション: `from` から `to` へ動かす。`to` が `None` なら、毎 tick 決まる置き場所（はまる場所）へ向かう。
#[derive(Debug)]
struct Glide {
    from: Pose,
    to: Option<Pose>,
    tween: Animation,
    last: Instant,
}

#[derive(Debug, Default)]
pub struct Overlay {
    // shortcut: 閉じた ID の小さな記録は終了まで残す。開閉履歴のメモリが問題になれば世代付き ID に替える。
    panels: Vec<Panel>,
    /// 解放前のパネルだけ。ID の履歴は `panels` に残すが、毎 tick はここだけを走査する。
    present: Vec<PanelId>,
    hands: HashMap<Hand, HandState>,
    /// 最後の tick で見えていたトラッカーの ID（取り付け先の候補）。
    trackers: Vec<String>,
    /// 最後の tick で骨格が取れていた手。
    skeletons: Vec<HandSkeleton>,
    /// 最後の tick の手ごとの入力の状態。
    controllers: Vec<ControllerState>,
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
    /// カーソルの大きさの倍率（[`Overlay::set_cursor_scale`]）。`None` で等倍。
    cursor_scale: Option<f32>,
    /// 暗転中もゲームへの入力を通す（[`Overlay::set_block_input`]）。既定（`false`）は止める。
    pass_input: bool,
    /// UI スケール（[`Overlay::set_scale`]）。`None` で等倍。
    scale: Option<f32>,
    /// この tick で手ブレ抑えが効いている（ポインタを手に追従させず、空間に置く）。
    steadied: bool,
    animations: HashMap<PanelId, (Animation, Instant)>,
    /// この tick で外側を押されて閉じる子。
    dismissed: Vec<PanelId>,
    /// 親の暗転。
    dims: HashMap<PanelId, (Animation, Instant)>,
    /// 位置のトランジション（はめる・外すときの移動）。
    glides: HashMap<PanelId, Glide>,
    /// どのパネルにも当たっていない所でトリガーを押した回数（[`Overlay::outside_presses`]）。
    outside_presses: u32,
    /// 空間全体の暗転。
    veil: Veil,
    /// 手ごとの OS のカーソルの面と、送ってある絵。最初に出すときに作る。
    os_cursors: HashMap<Hand, (Surface, Option<Arc<Cursor>>)>,
    /// 前の tick の時刻と、そこからの経過（秒。[`TICK_MIN`]〜[`TICK_MAX`] に丸める）。
    last_tick: Option<Instant>,
    dt: f32,
}

/// 空間全体の暗転の幕（[`Overlay::set_veil`]）。
#[derive(Debug, Default)]
struct Veil {
    /// 頭に付けた黒い幕。最初に暗くするときに作る。
    surface: Option<Surface>,
    /// 今の濃さと、向かっている先。
    opacity: f32,
    to: f32,
    fade: Option<(Animation, Instant)>,
    /// ゲームへの入力を今止めている（バックエンドへ最後に伝えた値）。
    blocked: bool,
}
