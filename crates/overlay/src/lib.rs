//! Layer 3: オーバーレイ基盤。3D 空間上のパネルを管理し、各コントローラのレーザーとの交差判定を行い、
//! ヒットを UI 層向け 2D ポインタイベントに変換する。パネルのグラブ移動も処理する。
//! パネルはデバイス（HMD・コントローラ・トラッカー）に取り付けて追従させることができる。
//! [`Overlay::tick`] が下位パイプライン 1 フレーム分: Subsystems 共通フレーム → 取り付け先追従 → ヒット判定 → イベント。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use animation::{Animation, Motion, Transition};
use design::{BRACKET_PAD_PX, PANEL_MARGIN};
use subsystems::{Frame, Subsystems, desktop::Cursor, rendering::Surface, vr};

use vr::{Device, Hand, HandSkeleton, Mat3, OverlayId, Pose, Quat, Vec2, Vec3, VrBackend, VrError};

mod lazy_follow;
pub use lazy_follow::LazyFollow;
pub use subsystems::ControllerState;

mod corner;
mod family;
mod frame;
mod geometry;
mod hand;
mod layers;
mod magnet;
mod options;
mod panels;
mod pointers;
mod status;

pub use corner::ResizeLimits;
use corner::*;
use family::*;
use frame::*;
pub use geometry::anchored;
use geometry::*;
pub use magnet::{DOCK_HINT_PX, DockHint};
use magnet::{Ghost, Magnet};
use pointers::*;

#[cfg(test)]
mod test_util;

pub type PanelId = usize;

/// 親子関係（ダイアログ等）。子は親に追従し、子が開いている間は親が暗転して入力を受けない（グラブ移動は可能）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Parent {
    pub panel: PanelId,
    /// 子のグラブ時に親も連動する。`false` なら子のみ移動し、親との相対位置が再計算される。
    pub carry: bool,
    /// 子の外側（親・他パネル・空間）を押すと子を閉じる。閉じさせたくない子のみ `false`。
    pub dismiss: bool,
    /// 表示専用の子（統計等）。親の左上隅に重ねて配置し、親を暗転させず入力も止めない。レーザーは素通りする。
    pub hud: bool,
    /// 親に固定された子（ダッシュボードバーやはめ込み画面）。親を暗転させず入力も止めない。
    pub docked: bool,
    /// 親の上端に立てて配置する（`docked` の子）。値は親基準の水平軸回り起こし角（rad）。
    /// ナビバー分の余白を確保し子の下端を配置する。子のサイズ変更時は毎 tick 再配置する。
    pub slot: Option<f32>,
    /// `slot` 子で、他にスロット子（はめ込み画面）があればその上端の上に積む（なければ親の上端）。積み先の
    /// 変更時は滑らかに遷移する。ダッシュボード上の通知など、はめ込み画面と並べて上に表示するもの。
    pub stack: bool,
    /// 親の面の一部に重ねる子。値は親の面の左上を原点、面の幅・高さを 1 とした範囲（`[x, y, 幅, 高さ]`。面の外へ
    /// はみ出してよい）。子の面（影の余白を除く）を、子の解像度の縦横比のままこの範囲に収まる最大の大きさで中央に置く。
    /// 位置と大きさは毎 tick 範囲に合わせ直すので、掴んで子だけを動かすことはできない。親を暗転させず入力も止めない。
    pub over: Option<[f32; 4]>,
}

impl Parent {
    /// 親を暗転させ入力を止める子か（ダイアログ等）。表示専用の子・固定子・面に重ねる子は止めない。
    fn blocks(&self) -> bool {
        !self.hud && !self.docked && self.over.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// クアッドの中心（トラッキング空間）。表面はローカル +Z 方向。
    /// 取り付け中は毎 tick「取り付け先姿勢 × オフセット」で更新される。
    pub pose: Pose,
    /// メートル単位。
    pub size: Vec2,
    /// クアッドに貼る UI サーフェスのピクセル数。
    pub resolution: (u32, u32),
    /// 取り付け先デバイスとデバイスから見たパネルの相対姿勢。
    pub attached: Option<(Device, Pose)>,
    /// グラブで移動不可。取り付け先がなければ空間に固定される。
    pub locked: bool,
    /// グラブでリサイズ可能（[`Overlay::set_resizable`]。既定は可能）。
    pub resizable: bool,
    /// 角を掴んで縦横の解像度を変えられる範囲（[`Overlay::set_corner_resize`]）。`None` なら角では変えない（既定）。
    pub corner_resize: Option<ResizeLimits>,
    /// 取り付け状態でのグラブ移動時の取り付け先からの距離上限（m。[`Overlay::set_reach`]）。`None` なら無制限。
    pub reach: Option<f32>,
    /// グラブ時に手で移動して頭へ向ける基準（パネルローカル姿勢。[`Overlay::set_grab_anchor`]）。`None` ならパネル自体。
    pub grab_anchor: Option<Pose>,
    /// 閉じ済み。ヒット判定対象外で、PanelId は詰めず欠番のまま残る。
    pub closed: bool,
    /// 非表示（[`Overlay::set_hidden`]）。閉じずにフェードアウトし、ヒット判定・暗転の対象外になる。
    pub hidden: bool,
    /// どの手のレーザーを描くパネルか。ヒット判定対象外で、配置は毎 tick 本層で決定する。
    pub pointer: Option<(Hand, Pointer)>,
    /// イン・アウトアニメーション有効。`Overlay::set_animated` で個別に無効化可能。
    pub animated: bool,
    /// 表示中に空間全体を暗転させる（[`Overlay::set_veil`]）。
    pub veil: bool,
    /// レーザーが素通りする（[`Overlay::set_pass_through`]）。ヒット判定をスキップする。
    pub pass_through: bool,
    /// イン・アウトを不透明度のみで行う（[`Overlay::set_fade_only`]）。
    pub fade_only: bool,
    /// OS へのタッチをリリースまで同一パネルへ配送する。
    capture: bool,
    /// ポインティング中にカーソルに代えて表示する OS カーソル（[`Overlay::set_os_cursor`]）。
    pub os_cursor: Option<(Hand, Arc<Cursor>)>,
    /// 親（[`Overlay::set_parent`]）。取り付けより優先し「親の姿勢 × `offset`」に追従する。
    pub parent: Option<Parent>,
    /// このパネルがナビバーである場合の持ち主（[`Overlay::set_nav_bar`]）。持ち主の下端に吸着し、
    /// 頭からの見かけサイズが一定になるよう距離に比例して拡大する。自身のアニメーションは持たず、
    /// 出入り（不透明度・75%〜100% サイズ）と暗さは毎 tick 持ち主から複製する。持ち主と同時に閉じ、持ち主暗転中は押下不可。
    pub nav_bar_of: Option<PanelId>,
    /// ナビバーの頭から 1m 先での基準サイズ（`size` は毎 tick これ × 距離で算出）。
    nav_bar_size: Vec2,
    /// ナビバーの大きさを固定している度合い（1 = 基準サイズ固定、0 = 距離に比例）。次に置くときに合わせ直すなら `None`。
    pinned: Option<Transition>,
    /// ナビバーの覚醒度（1 = 表示中、0 = フェードアウト完了。不透明度に乗算）と遷移先。
    /// 持ち主・バーともに未ポインティングのまま一定時間経過すると線が縮小しつつフェードし、
    /// 完全消失時にランタイムオーバーレイを解放する（パネル・ビュー・画像は保持）。
    /// ポインティング再開時に再生成する（[`Overlay::place_nav_bars`]）。他パネルは常に 1。
    awake: f32,
    wake: Transition,
    /// 持ち主またはバーが最後にポインティングされた時刻（未ポインティングなら生成時刻）。
    pointed_at: Instant,
    /// 親から見た相対姿勢。
    offset: Pose,
    /// 現在の暗転度（0 = 暗転なし）と遷移先。
    dim: f32,
    dim_to: f32,
    /// 表示の進行度。サイズは 75%〜100%、不透明度は 0%〜100%。
    opacity: f32,
    /// 角を掴んで大きさを変えている間の、代わりの板の出具合（[`Overlay::place_stand_ins`]）。中身の不透明度は 1 からこれを引いた割合。
    stand_in: f32,
    surface: Surface,
    /// 描画順を決めるのに使う頭からの距離（m。急には動かさない。[`Overlay::place_layers`]）。
    depth: Option<f32>,
    /// 重なる下のパネルより手前にするために、`pose` を要望の位置から面の手前へずらした量（m。[`Overlay::place_layers`]）。
    lift: f32,
    /// 今 tick で追従させるデバイスと相対姿勢。ランタイムに委譲可能ならそうする。
    follow: Option<(Device, Pose)>,
    /// 頭の向きに緩やかに追従する（[`Overlay::set_lazy_follow`]）。取り付けより優先する。
    pub lazy_follow: Option<LazyFollow>,
    /// ドックスロット（[`Overlay::set_dock_slot`]）からグラブで引き抜き・接近ではめ込み可能（[`Overlay::set_dockable`]）。
    pub dockable: bool,
}

impl Panel {
    /// グラブ可能か（ロックされておらず LazyFollow も無効）。LazyFollow は毎 tick 位置を決定するためグラブ移動は無意味。
    pub fn grabbable(&self) -> bool {
        !self.locked && self.lazy_follow.is_none()
    }
}

/// レーザー描画パネルの役割。見た目は上位層の画面が描画し、配置はオーバーレイ層が決定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// 手元からヒット点（グラブ中はグラブ対象パネル）まで伸びる線。ヒットなしなら非表示。幅 = 長さで、テクスチャ横方向が
    /// レーザー方向（左端が手元）。テクスチャ面はコントローラ上方を向く。
    Laser,
    /// ヒット点に貼り付くカーソル。パネルより手前に描画。ヒットなしなら非表示。
    Cursor,
    /// ドラッグ中の要素の表示。中心をヒット点に配置し、[`Overlay::set_drag`] で幅が設定されている間のみ表示する。
    /// パネル外にも描画でき、ウィンドウ間での移動に対応する。
    Drag,
    /// ツールチップ。カーソルの子で、同一点・同一縮尺（頭から 1m 先での基準サイズ）でカーソルより手前に描画する。
    /// 中心がカーソル位置で、上下どちらに描くかは [`Overlay::tooltip_anchor`] を参照して上位層が決定する。
    Tooltip,
    /// ドックスロットのヒント表示。ドッカブルパネルのグラブ中、スロット位置にパネルと同サイズで配置する
    /// （上端に [`DOCK_HINT_PX`] の帯を加えた解像度をオーバーレイ層が決定）。パネルより奥に描画。内容は [`Overlay::dock_hint`]。
    Dock,
    /// 角を囲む線。角で大きさを変えられるパネル（[`Overlay::set_corner_resize`]）の角を指している間と、掴んで大きさを
    /// 変えている間、その角に置く。窓の中心がパネル面の角に来て、窓 1px がパネル 1px になる（[`design::CORNER_HANDLE_PX`]）。
    /// 右下の角の線を描けば、ほかの角には回して置く。
    Corner,
}

/// ツールチップのカーソルに対する上下配置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Anchor {
    /// 上。レーザー入射側（手）がカーソルの上方に見える場合（下方から見上げている状態）。
    Above,
    /// 下。手が頭から見てカーソルの下方にある場合（通常の構え）。
    #[default]
    Below,
}

/// レーザーパネル生成時の初期長さ（m）。以降は毎 tick ヒット点までの距離に伸縮する。
pub const LASER_MAX: f32 = 1.5;
/// レーザーをヒット点手前で止めるギャップ（m）。深度ソートランタイムでレーザー先端の透明部分がカーソルに重なり
/// カーソルが欠けるのを防ぐ。先端は透明なため見た目の長さへの影響はほぼない。
const LASER_GAP: f32 = 0.03;
/// レーザーの可視長（m）の既定値と範囲。手元からこの距離でフェードする（カーソルが近ければその手前で終端）。
pub const LASER_LENGTH: f32 = 0.3;
pub const LASER_LENGTH_MIN: f32 = 0.05;
/// カーソルをパネル面からレーザー入射側へ浮かせる距離（m）。描画順でもパネルより手前に配置するが、
/// 深度ソートランタイムでの Z ファイティング防止にも必要。
/// パネル→ドラッグ表示→カーソル→ツールチップの順に手前へ、各間は [`LIFT_STEP`] ずつ均等に配置。
const CURSOR_LIFT: f32 = LIFT_STEP * 2.0;
/// 面上重畳要素間の間隔（m）。広すぎると浮いて見え視差で照準がずれ、狭すぎると Z ファイティングが発生する。
/// 1mm は 10m 先でも 24bit 深度バッファで約 8 段の差がある。
// ponytail: 固定値。実機でちらつくなら拡大する。
const LIFT_STEP: f32 = 0.001;
/// 描画順の帯。最奥が空間全体の暗転幕（[`Overlay::set_veil`]）、次にドックヒント（[`Pointer::Dock`]。グラブ中のパネルと
/// そのナビバーが常に上に重なる）、[`WORLD_SORT`] からパネル、最も手前がレーザーまわり。パネルどうしの順位は
/// [`Overlay::place_layers`] が毎 tick 決める。どのパネルも描画順を直に指定しない。
const VEIL_SORT: u32 = 0;
const GHOST_SORT: u32 = 1;
const WORLD_SORT: u32 = 2;
/// パネルの順位が届かない大きさにする。
const LASER_SORT: u32 = 10_000;
const DRAG_SORT: u32 = LASER_SORT + 1;
const CURSOR_SORT: u32 = LASER_SORT + 2;
const TOOLTIP_SORT: u32 = LASER_SORT + 3;
/// グラブ移動で目標へ 1/120 秒（[`TICK_MIN`]）あたり近づく割合。経過時間で換算するためフレーム落ちでも追従速度は一定。
/// 目標との差が [`SNAP`] 未満なら目標にスナップする。
const SMOOTH: f32 = 0.3;
const SNAP: f32 = 1e-5;
/// 子が開いている間の親の暗転度（黒との混合率）。子に注目させつつ読める程度に抑える。
/// 暗転の遷移時間は子のフェードイン（0.2 秒）・アウト（0.15 秒）と同一。
const DIM: f32 = 0.45;
/// 空間全体の暗転（[`Overlay::set_veil`]）の濃さ（黒の不透明度）の既定値。周囲がうっすら見える程度にする。
/// [`Overlay::set_veil_opacity`] で変更できる。
pub const VEIL: f32 = 0.5;
/// スロット子（[`Parent::slot`]）のナビバー下端と親の上端の間隔（m）。
pub const SLOT_GAP: f32 = 0.02;
/// スロット子が入れ替わりで退出するときの後退距離（m）。
const SLOT_RETREAT: f32 = 0.15;
/// 手ブレ抑制の既定値: レーザー方向が 1 tick ごとに前回方向を保持する割合（0 = 抑制なし）。
/// ヒット判定もこの方向で行うため、クリック位置の揺れも抑制される。
// ponytail: tick 単位のためリフレッシュレートで効きが変わる。必要なら経過時間ベースに変更する。
pub const POINTER_SMOOTHING: f32 = POINTER_SMOOTHING_MAX;
/// 手ブレ抑制の上限。1 だとレーザーが静止する。
pub const POINTER_SMOOTHING_MAX: f32 = 0.998;
/// 手ブレ抑制を緩和する角度（度）の既定値と範囲。抑制レーザーが手の方向からこの角度遅延すると抑制が半減する。
/// 静止時は強く抑制し、大きな移動時は遅延を防ぐ。小さすぎると手ブレの揺れだけで抑制が崩壊する
/// （強い抑制ほど遅延が蓄積するため、抑制を強めても効果がなくなる）。
pub const POINTER_RELEASE_DEG: f32 = 11.25;
pub const POINTER_RELEASE_DEG_MIN: f32 = 0.5;
pub const POINTER_RELEASE_DEG_MAX: f32 = 45.0;
/// プレス後、プレス点がレーザーからこの角度（rad、2°）ずれるまでプレス位置に固定する。
/// トリガー操作時の手ブレでクリックの逸脱やスワイプ誤認を防止する。
// ponytail: 固定値。感度が合わなければここを調整する。
const CLICK_SLOP: f32 = 0.035;
/// 今 tick のレーザー情報。
#[derive(Debug, Clone, Copy)]
struct Ray {
    /// コントローラの姿勢。
    hand: Pose,
    /// レーザー: 位置はコントローラ、向きは手ブレ抑制適用済み。ヒット判定もこれで行う。
    aim: Pose,
    len: f32,
    /// ヒットしたパネルとトラッキング空間でのヒット点。
    hit: Option<(PanelId, Vec3)>,
}

impl Panel {
    /// 重なりのためにずらす前の姿勢。姿勢から相対位置を作り直すときはこちらを使う（ずらした量を要望に混ぜない）。
    fn unlifted(&self) -> Pose {
        self.pose * Pose::new(Vec3::NEG_Z * self.lift, Quat::IDENTITY)
    }

    /// 確定した描画順（大きいほど手前。[`Overlay::place_layers`]）。
    pub fn draw_order(&self) -> u32 {
        self.surface.sort()
    }

    /// 重なる下のパネルより手前にするために、要望の位置から手前へずらした量（m）。
    pub fn lifted(&self) -> f32 {
        self.lift
    }

    /// アニメーション反映後の表示サイズ。中心の位置・姿勢は不変。
    pub fn displayed_size(&self) -> Vec2 {
        if self.fade_only {
            return self.size;
        }
        self.size * (0.75 + 0.25 * self.opacity)
    }

    /// パネル上のピクセル座標（左上原点）をトラッキング空間の点に変換する。ヒット判定の逆変換。
    pub fn pixel_to_world(&self, px: Vec2) -> Vec3 {
        let (w, h) = (self.resolution.0 as f32, self.resolution.1 as f32);
        let local = Vec3::new(
            (px.x / w - 0.5) * self.displayed_size().x,
            (0.5 - px.y / h) * self.displayed_size().y,
            0.0,
        );
        self.pose.transform_point(local)
    }

    /// ランタイム側オーバーレイ ID。実ランタイムからの描画読み戻し確認用。
    pub fn overlay_id(&self) -> OverlayId {
        self.surface.id()
    }
}

/// 座標はパネルのピクセル単位、原点は左上。
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
    /// 前 tick でトリガー押下中だったか（押下開始検出用）。
    pressed: bool,
    /// 外側プレスで子を閉じた押下。リリースまで下層パネルへイベントを送らない（親のボタン誤押下を防止）。
    swallow: bool,
    menu: bool,
    /// グラブ移動中のパネル・手基準の位置・レーザー沿い距離・基準から見たパネル姿勢・リサイズ可否。
    /// 基準（頭方向へ手で移動する対象）は通常パネル自体（`Pose::IDENTITY`）。[`grab_target`] が決定する。
    dragging: Option<(PanelId, Vec3, f32, Pose, bool)>,
    /// グラブ開始時のサイズとグラブ基準（グラブ取り消し時の復帰用。[`Overlay::set_locked`]）。
    before: (Vec2, Option<Pose>),
    /// ドラッグ表示の幅（m）。ドラッグ中のみ有効。
    drag: Option<f32>,
    /// グラブ中のトリガーリサイズ時における前 tick の手の位置。
    scaling: Option<Vec3>,
    /// 前 tick に描画したレーザーの向き。非表示時は `None`（次回表示時は補間せず即座に合わせる）。
    drawn: Option<Quat>,
    /// 今 tick のレーザー。コントローラ不可視またはヒットなしの場合は `None`。
    ray: Option<Ray>,
    /// プレス中のキャプチャを解除した状態（リリースまで）。
    free: bool,
    /// プレス位置（ピクセル）とトラッキング空間での点。[`CLICK_SLOP`] を超過するかリリースするまで保持。
    press: Option<(Vec2, Vec3)>,
    /// ツールチップの配置方向（毎 tick 更新）。
    anchor: Anchor,
    /// グラブ中画面のドック引き抜き・はめ込み磁石（ドッカブルパネル自体のグラブ中のみ有効）。
    magnet: Option<Magnet>,
    /// この手のドックヒント（[`Pointer::Dock`]）の表示進行度。
    ghost: Ghost,
    /// 指している角（パネルと角の向き。[`corner_at`]）。毎 tick 更新する。
    corner: Option<(PanelId, Vec2)>,
    /// 角を掴んで大きさを変えている途中。
    stretch: Option<Stretch>,
    /// この手の角の線（[`Pointer::Corner`]）の出具合。
    line: Line,
}

/// 位置トランジション: `from` から `to` へ移動する。`to` が `None` なら毎 tick 算出される配置先（ドックスロット）へ向かう。
#[derive(Debug)]
struct Glide {
    from: Pose,
    to: Option<Pose>,
    tween: Animation,
    last: Instant,
}

#[derive(Debug, Default)]
pub struct Overlay {
    // shortcut: 閉じた ID の記録は終了まで残す。メモリが問題になれば世代付き ID に移行する。
    panels: Vec<Panel>,
    /// 未解放パネルのみ。ID 履歴は `panels` に残すが、毎 tick の走査対象はこちらのみ。
    present: Vec<PanelId>,
    hands: HashMap<Hand, HandState>,
    /// 直前 tick で検出されたトラッカー ID（取り付け先候補）。
    trackers: Vec<String>,
    /// 直前 tick でスケルタル入力が得られた手。
    skeletons: Vec<HandSkeleton>,
    /// 直前 tick の各コントローラ入力状態。
    controllers: Vec<ControllerState>,
    quit: bool,
    /// 直前 tick の HMD 姿勢。
    hmd: Pose,
    /// プレス中のポインタキャプチャを使用するか（[`Overlay::set_capture`]）。
    capture: bool,
    /// 手ブレ抑制値（[`POINTER_SMOOTHING`]）。`None` で既定値。
    smoothing: Option<f32>,
    /// レーザーの可視長（[`LASER_LENGTH`]）。`None` で既定値。
    laser_length: Option<f32>,
    /// 手ブレ抑制緩和角度（度。[`POINTER_RELEASE_DEG`]）。`None` で既定値。
    release_deg: Option<f32>,
    /// カーソルサイズ倍率（[`Overlay::set_cursor_scale`]）。`None` で等倍。
    cursor_scale: Option<f32>,
    /// 暗転中もゲームへの入力を通す（[`Overlay::set_block_input`]）。既定（`false`）は入力を遮断する。
    pass_input: bool,
    /// 暗転の濃さ（[`Overlay::set_veil_opacity`]）。`None` で既定値 [`VEIL`]。
    veil_opacity: Option<f32>,
    /// UI スケール（[`Overlay::set_scale`]）。`None` で等倍。
    scale: Option<f32>,
    /// 今 tick で手ブレ抑制が有効（ポインタをデバイス追従ではなくワールド配置にする）。
    steadied: bool,
    animations: HashMap<PanelId, (Animation, Instant)>,
    /// 今 tick で外側プレスにより閉じる子パネル。
    dismissed: Vec<PanelId>,
    /// 親パネルの暗転アニメーション。
    dims: HashMap<PanelId, (Animation, Instant)>,
    /// 位置トランジション（ドック・アンドック時の移動）。
    glides: HashMap<PanelId, Glide>,
    /// パネル外でのトリガー押下回数（[`Overlay::outside_presses`]）。
    outside_presses: u32,
    /// 空間全体の暗転状態。
    veil: Veil,
    /// 手ごとの OS カーソルサーフェスと送信済み画像。初回表示時に生成する。
    os_cursors: HashMap<Hand, (Surface, Option<Arc<Cursor>>)>,
    /// 前 tick の時刻と経過秒数（[`TICK_MIN`]〜[`TICK_MAX`] にクランプ）。
    last_tick: Option<Instant>,
    dt: f32,
    /// ドックスロット: 持ち主と上端に立てる角度（[`Overlay::set_dock_slot`]）。
    dock_slot: Option<(PanelId, f32)>,
    /// アンドック（`false`）・ドック（`true`）されたパネル（[`Overlay::take_dock_changes`] で取得）。
    dock_changes: Vec<(PanelId, bool)>,
    /// 掴んだまま、はまる所から抜けた（`false`）・吸い付いた（`true`）パネル（[`Overlay::take_dock_snaps`] で取得）。
    dock_snaps: Vec<(PanelId, bool)>,
    /// ドックスロットに接触した状態でリリースされたパネル（今 tick 終了時にドック・再ドックする）。
    landed: Vec<PanelId>,
    /// 角を掴んで大きさを変えているパネルの、中身の代わりに出す板（[`Overlay::place_stand_ins`]）。
    stand_ins: HashMap<PanelId, StandIn>,
}

/// 空間全体の暗転幕（[`Overlay::set_veil`]）。
#[derive(Debug, Default)]
struct Veil {
    /// HMD に取り付けた黒色幕。初回暗転時に生成する。
    surface: Option<Surface>,
    /// 現在の不透明度と遷移先。
    opacity: f32,
    to: f32,
    fade: Option<(Animation, Instant)>,
    /// ゲームへの入力を現在遮断中か（バックエンドへ最後に送信した値）。
    blocked: bool,
}
