//! ランタイムサブシステム。OpenVR / OpenXR / novr（VR なし）の違いを1つのトレイト [`VrBackend`] の裏に隠す。
//! どれも対等な「手段」で、上位層は [`connect`] から得た `Box<dyn VrBackend>` だけを扱う。
//! novr もデスクトップのウィンドウで VR をエミュレートするエミュレーター（[`novr::Emulator`]）なので、上位層から見た違いは無い。
//!
//! 語彙は OpenXR に揃える: コントローラのプロファイルは `"vendor/name"`（例 `"valve/index_controller"`）、
//! 入力は `"component/feature"`（例 `"trigger/value"`）。OpenVR バックエンドはこれに翻訳する。

#[cfg(feature = "novr")]
mod compositor;
mod external;
// openvr と openxr で使用する部分が異なるため、片方のみのビルドでは未使用になる。
#[cfg(any(feature = "openvr", feature = "openxr"))]
#[cfg_attr(not(all(feature = "openvr", feature = "openxr")), allow(dead_code))]
mod gpu;
#[cfg(any(test, feature = "test-support"))]
pub mod mock;
#[cfg(feature = "novr")]
pub mod novr;
#[cfg(feature = "openvr")]
mod openvr;

/// testkit の実ランタイムテスト用。上位層からは使用しない。
#[cfg(feature = "openvr")]
#[doc(hidden)]
pub use openvr::{
    overlay_image as openvr_overlay_image, overlay_state as openvr_overlay_state,
    runtime_path as openvr_runtime_path,
};
#[cfg(feature = "openxr")]
mod openxr;

use std::fmt;

pub use external::{ExternalImage, ImportFormats, Memory, PixelFormat};
pub use glam::{Mat3, Quat, Vec2, Vec3};

/// 頭がこの角度（ラジアン）を超えて見上げる・横に傾くと、仰臥姿勢とみなし始める（[`Pose::panel_up`]）。
const LIE_START: f32 = 35.0 * std::f32::consts::PI / 180.0;
/// この角度で仰臥姿勢が完了したとみなす。間はスムーズに遷移し、境界でパネルが跳ねない。
const LIE_FULL: f32 = 55.0 * std::f32::consts::PI / 180.0;

/// トラッキング空間での剛体変換（メートル）。前方は -Z（OpenVR / OpenXR 共通）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
    };

    pub const fn new(position: Vec3, orientation: Quat) -> Self {
        Self {
            position,
            orientation,
        }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        self.orientation * p + self.position
    }

    pub fn forward(&self) -> Vec3 {
        self.orientation * Vec3::NEG_Z
    }

    /// 頭の姿勢から決定する、パネルを立てる上方向。起立・着席中は真上で、首の傾きを無視する。
    /// 仰臥姿勢（見上げ・横傾き）が深いほど頭の上へ寄せ、完全な仰臥で頭の上そのもの。頭の位置（高さ）は参照せず、回転のみで決定する。
    /// 俯きは仰臥姿勢に含めない（手首を見下ろしてもパネルは立てたまま）。
    /// ponytail: 回転のみのため、起立状態で真上を見上げる場合も仰臥と区別しない。
    pub fn panel_up(&self) -> Vec3 {
        let (up, right) = (self.orientation * Vec3::Y, self.orientation * Vec3::X);
        // 見上げ角度と左右傾斜角度の大きい方。
        let tilt = self.forward().y.max(right.y.abs()).clamp(0.0, 1.0).asin();
        let t = ((tilt - LIE_START) / (LIE_FULL - LIE_START)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        Vec3::Y.lerp(up, t).normalize_or(Vec3::Y)
    }

    /// 位置はそのまま、上を [`Pose::panel_up`] に立て、前方（-Z）を頭の前方に寄せた姿勢。
    /// 頭の前にパネルを表示する際に使用する。起立中は水平方向（Y 軸回り）のみで、俯き・首の傾きを無視する。
    /// 仰臥中は頭の向きそのもので、顔の前に表示される。
    pub fn leveled(&self) -> Pose {
        let up = self.panel_up();
        let f = self.forward();
        let Some(back) = (up * f.dot(up) - f).try_normalize() else {
            // 真上・真下を見ている（起立中の完全な俯き）。方向が不定のため前方 -Z。
            return Pose::new(self.position, Quat::IDENTITY);
        };
        let x = up.cross(back).normalize();
        Pose::new(
            self.position,
            Quat::from_mat3(&Mat3::from_cols(x, back.cross(x), back)),
        )
    }

    pub fn inverse(&self) -> Pose {
        let inv = self.orientation.inverse();
        Pose {
            position: inv * -self.position,
            orientation: inv,
        }
    }
}

impl std::ops::Mul for Pose {
    type Output = Pose;
    fn mul(self, rhs: Pose) -> Pose {
        Pose {
            position: self.transform_point(rhs.position),
            orientation: self.orientation * rhs.orientation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hand {
    Left,
    Right,
}

/// グラブ中パネルの拡縮（-1..1、+ で拡大）を直接伝達する入力名。実機のコントローラには存在せず、
/// novr のエミュレーターが PageUp / PageDown で生成する（テスト用。手の前後移動が不可能なため）。
pub const RESIZE_INPUT: &str = "nyaw/resize";

/// ショートカットのダブルプレスを、ランタイムがカウントして 1 フレームのみ 1 を立てる入力名。SteamVR ではバインドの「ダブルタップ」がカウントするため、
/// ボタンの押下そのもの（`x/click` 等）は出力しない。他のランタイムは出力せず、`kernel` が押下間隔からカウントする。
pub const DOUBLE_PRESS_INPUT: &str = "nyaw/double_press";

/// ランタイム非依存・バインド前のコントローラのスナップショット。解釈は入力層の仕事。
#[derive(Debug, Clone, PartialEq)]
pub struct RawController {
    pub hand: Hand,
    pub profile: String,
    pub aim: Pose,
    pub inputs: Vec<(String, f32)>,
}

impl RawController {
    pub fn input(&self, name: &str) -> Option<f32> {
        self.inputs.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

/// 手の関節の数。並びは OpenXR（XR_EXT_hand_tracking）の `XrHandJointEXT` に揃える:
/// 0 手のひら、1 手首、2〜5 親指（中手骨・基節・末節・先端）、6〜10 人差し指（中手骨・基節・中節・末節・先端）、
/// 11〜15 中指、16〜20 薬指、21〜25 小指。OpenVR のボーン（`eBone_*`）も 1〜25 は同じ並び。
pub const JOINT_COUNT: usize = 26;
pub const PALM: usize = 0;
pub const WRIST: usize = 1;
/// 指ごとの先端の関節（親指・人差し指・中指・薬指・小指）。
pub const TIPS: [usize; 5] = [5, 10, 15, 20, 25];

/// 関節 `joint` の親（骨でつながる手首側の関節）。手首と手のひらには無い。
pub const fn joint_parent(joint: usize) -> Option<usize> {
    match joint {
        PALM | WRIST => None,
        // 各指の付け根（中手骨）は手首につながる。
        2 => Some(WRIST),
        j if j % 5 == 1 => Some(WRIST),
        j => Some(j - 1),
    }
}

/// 片手の骨格（スケルタル入力）。関節の姿勢はトラッキング空間（[`Pose`] と同じ）。
/// 向きの軸はランタイムのまま（OpenXR は -Z が指先の向き・+Y が手の甲の外。OpenVR はボーンごとに違う）。
// ponytail: OpenVR の関節の向きは OpenXR の軸に直していない。向きで判定するものが要るなら eBone ごとの回転を掛けて揃える。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandSkeleton {
    pub hand: Hand,
    pub joints: [Pose; JOINT_COUNT],
    /// 指をどの程度実際に追跡しているか。
    pub tracking: FingerTracking,
}

/// 骨格の指をどの程度実際に追跡しているか（OpenVR の `EVRSkeletalTrackingLevel` に揃える）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerTracking {
    /// 指は追跡しておらず、ボタン等から推定した形状（Vive のコントローラ）。
    Estimated,
    /// コントローラを握ったまま、一部の指を追跡している（Index・Touch の静電容量）。
    Partial,
    /// 指を全部追跡している（光学式のハンドトラッキング・グローブ）。コントローラを握っていない手で、ランタイムがコントローラとして
    /// 提示していても（Steam Link・Virtual Desktop はピンチをトリガーに変換する）骨格で操作する。
    Full,
}

/// 手のひらと手の甲のどちらを顔に向けているか（[`HandSkeleton::facing`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Palm,
    Back,
}

/// 手首の関節から見た、握ったコントローラの tip（[`HandSkeleton::grip`] の元。grip の座標系。m）: 指の方へ 12cm、
/// 親指の側（コントローラの天面）へ 4cm。逆に tip から見た手首の関節は後ろへ 12cm・下へ 4cm で、手首の時計
/// （`wrist::watch_offset`）はここから決める。
// ponytail: Touch / Index を握った際の目分量。機種ごとの握りの差はこの値の調整で合わせる。
pub const GRIP_FROM_WRIST: Vec3 = Vec3::new(0.0, 0.04, -0.12);

/// 人差し指・小指の基節（手のひらの面を張る関節）。
const INDEX_PROXIMAL: usize = 7;
const LITTLE_PROXIMAL: usize = 22;

impl HandSkeleton {
    /// 手のひらから外へ出る向き（単位ベクトル）。関節の位置のみから算出するため、ランタイムの関節軸の違いに依存しない。
    pub fn palm_normal(&self) -> Vec3 {
        let wrist = self.joints[WRIST].position;
        let index = self.joints[INDEX_PROXIMAL].position - wrist;
        let little = self.joints[LITTLE_PROXIMAL].position - wrist;
        // 右手は人差し指 × 小指が手のひらの側。左手は鏡写しで逆。
        let n = match self.hand {
            Hand::Right => index.cross(little),
            Hand::Left => little.cross(index),
        };
        n.normalize_or_zero()
    }

    /// 手のひらを `eye` へどの程度向けているか（なす角の cos。1 で正面に手のひら、-1 で手の甲）。
    pub fn palm_toward(&self, eye: Vec3) -> f32 {
        let to_eye = (eye - self.joints[PALM].position).normalize_or_zero();
        self.palm_normal().dot(to_eye)
    }

    /// `eye` に向けているのは手のひらか手の甲か。
    pub fn facing(&self, eye: Vec3) -> Facing {
        if self.palm_toward(eye) >= 0.0 {
            Facing::Palm
        } else {
            Facing::Back
        }
    }

    /// この手でコントローラを握った際の `/pose/tip` に相当する姿勢: 前方（-Z）は手首から中指の付け根への向き、上（+Y）は
    /// 親指側（コントローラの天面）、+X は右手なら手の甲・左手なら手のひら側。原点は手首から [`GRIP_FROM_WRIST`]。
    /// コントローラに取り付けたパネル（手首の時計等）を、骨格のみの手にも同じ相対姿勢で付与するため。
    // ponytail: 前腕は追跡していないため、手首を曲げると手首の時計も手と一緒に傾く。
    pub fn grip(&self) -> Pose {
        let p = |i: usize| self.joints[i].position;
        let back = (p(WRIST) - p(12)).normalize_or(Vec3::Z);
        let palm = self.palm_normal();
        let x = match self.hand {
            Hand::Right => -palm,
            Hand::Left => palm,
        };
        let up = back.cross(x).normalize_or(Vec3::Y);
        let orientation = Quat::from_mat3(&Mat3::from_cols(up.cross(back), up, back));
        Pose::new(p(WRIST) + orientation * GRIP_FROM_WRIST, orientation)
    }

    /// 親指の先端と `tip`（[`TIPS`] のいずれか）の先端との距離（m）。ピンチ動作の判定に使用する。
    pub fn pinch_distance(&self, tip: usize) -> f32 {
        self.joints[TIPS[0]]
            .position
            .distance(self.joints[tip].position)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub hmd: Pose,
    pub controllers: Vec<RawController>,
    /// 骨格が取得できている手（スケルタル入力。ランタイムが未対応であれば空）。
    pub hands: Vec<HandSkeleton>,
    /// 手ごとの、ランタイムが追跡しているその手のデバイスの姿勢（[`Device::Controller`] への取り付け基準。コントローラなら
    /// [`RawController::aim`] と同一）。ハンドトラッキングでもランタイムが手をデバイスとして提示していれば存在する
    /// （Virtual Desktop の `vd_hand_controller`）。存在しなければ骨格の手は骨格から配置する。
    pub hand_devices: Vec<(Hand, Pose)>,
    /// コントローラ以外のトラッカー（腰・足など）。ID はランタイムが決める（OpenVR はシリアル番号）。
    pub trackers: Vec<(String, Pose)>,
    /// 電池の残量を報告しているデバイス（OpenVR のみ。OpenXR には標準的な手段が存在しない）。
    pub batteries: Vec<Battery>,
    /// ランタイムが終了を求めている（SteamVR の終了、OpenXR セッションの EXITING など）。
    pub quit: bool,
    /// デバイスは実在せずエミュレートされたもの（novr のエミュレーター）。取り付けたパネルは追従しない（[`Frame::pose`] が `None`）。
    pub emulated: bool,
}

impl Frame {
    /// このフレームでの `device` の姿勢。トラッキングされていなければ（エミュレートされたデバイスも）`None`。
    pub fn pose(&self, device: &Device) -> Option<Pose> {
        if self.emulated {
            return None;
        }
        match device {
            Device::Hmd => Some(self.hmd),
            Device::Controller(hand) => self
                .controllers
                .iter()
                .find(|c| c.hand == *hand)
                .map(|c| c.aim),
            Device::Tracker(id) => self.trackers.iter().find(|(i, _)| i == id).map(|(_, p)| *p),
        }
    }
}

/// デバイスの電池。
#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub device: Device,
    /// 残量（0〜1）。
    pub level: f32,
    pub charging: bool,
}

/// 視野: 頭の正面から左・右・上・下の端までの角度（ラジアン。どれも正）。[`VrBackend::fov`] は両目に映る範囲を返す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

impl Fov {
    /// ランタイムが提供しない場合に想定する視野（市販 HMD の中で狭いもの。両目の重なりが片側 40°）。
    pub const NARROW: Fov = Fov::even(40.0_f32.to_radians(), 40.0_f32.to_radians());

    /// 左右・上下が対称な視野（片側の角度）。
    pub const fn even(horizontal: f32, vertical: f32) -> Fov {
        Fov {
            left: horizontal,
            right: horizontal,
            up: vertical,
            down: vertical,
        }
    }

    /// 2 つの視野の重なり（両目に映る範囲）。
    pub fn overlap(self, other: Fov) -> Fov {
        Fov {
            left: self.left.min(other.left),
            right: self.right.min(other.right),
            up: self.up.min(other.up),
            down: self.down.min(other.down),
        }
    }

    /// HUD（頭に取り付けて視野の端に表示するもの）を配置可能な範囲。どの HMD でも確実に映り、余裕を持たせる: 視野の端から
    /// [`SAFE_SHARE`] の内側（レンズの端はにじみ・歪みで判読不能、目の位置のずれで欠ける）で、かつ [`SAFE_MAX`] を
    /// 超えない（広視野 HMD でも、目を少し動かすだけで判読可能な位置に配置し、HMD による差異を縮小する）。
    pub fn safe(self) -> Fov {
        let side = |a: f32, max: f32| (a * SAFE_SHARE).min(max);
        Fov {
            left: side(self.left, SAFE_MAX.left),
            right: side(self.right, SAFE_MAX.right),
            up: side(self.up, SAFE_MAX.up),
            down: side(self.down, SAFE_MAX.down),
        }
    }

    /// 角度がすべて 0 より大きく直角より小さい（ランタイムが返した値を信頼可能）。
    pub fn valid(self) -> bool {
        [self.left, self.right, self.up, self.down]
            .iter()
            .all(|a| *a > 0.0 && *a < std::f32::consts::FRAC_PI_2)
    }
}

/// HUD を置いてよい範囲（[`Fov::safe`]）に使う、視野の片側のうちの割合。
pub const SAFE_SHARE: f32 = 0.4;
/// HUD を配置可能な範囲の上限（左右 15°・上下 12°。目を少し動かすだけで判読可能な位置）。
pub const SAFE_MAX: Fov = Fov::even(15.0_f32.to_radians(), 12.0_f32.to_radians());

/// トラッキングされるデバイス。オーバーレイを取り付ける先として使う。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Device {
    Hmd,
    /// 姿勢は [`RawController::aim`]。
    Controller(Hand),
    /// [`Frame::trackers`] の ID。
    Tracker(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    OpenVr,
    OpenXr,
    /// VR を使用せず、デスクトップのウィンドウで VR をエミュレートする（[`novr::Emulator`]）。
    NoVr,
}

#[derive(Debug, PartialEq)]
pub enum VrError {
    NoRuntime,
    NotImplemented(Runtime),
    Runtime(String),
}

impl fmt::Display for VrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrError::NoRuntime => write!(f, "no OpenVR or OpenXR runtime found"),
            VrError::NotImplemented(r) => write!(f, "{r:?} backend is not implemented yet"),
            VrError::Runtime(m) => write!(f, "VR runtime error: {m}"),
        }
    }
}

impl std::error::Error for VrError {}

/// 上位層が呼ぶ唯一のインターフェース。各メソッドは [`connect`] が返したバックエンドに応じて
/// 「OpenVR のやり方」「OpenXR のやり方」「novr のやり方」で実行される。
pub trait VrBackend {
    fn runtime(&self) -> Runtime;
    /// OpenVR / OpenXR はここでランタイムの vsync（次フレーム）まで待つ。novr は待たない（呼び出し側が周期を決める）。
    /// ランタイムのイベントを処理し、このフレームの姿勢と生のコントローラ入力を返す。
    fn poll(&mut self) -> Frame;
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError>;
    /// 同時に作成可能なオーバーレイ数の上限（OpenXR は 1 フレームに出力可能なレイヤー数、OpenVR はランタイム全体の数）。
    /// 存在しない・不明であれば `None`。超過分の [`create_overlay`](Self::create_overlay) はエラーになる。
    fn max_overlays(&self) -> Option<usize> {
        None
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose);
    /// `device` の姿勢 * `offset` に配置し、追従はランタイムに委任する（アプリが poll した姿勢を送信するより低遅延）。
    /// 委任不可なら `false` を返し、呼び出し側は毎 tick [`set_overlay_pose`](Self::set_overlay_pose) で送信する。
    /// 呼び出し側は毎 tick 再要求するため、前回と同一ならランタイムへ送信しないこと。
    fn set_overlay_pose_relative(
        &mut self,
        _id: OverlayId,
        _device: &Device,
        _offset: Pose,
    ) -> bool {
        false
    }
    /// 幅（m）。高さはテクスチャの縦横比で決まる。
    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32);
    /// 高さ（m）を幅・テクスチャの縦横比と無関係に固定する（伸縮するレーザーの太さを維持するため）。
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32);
    /// 作成直後は表示されている。
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool);
    /// 不透明度（0〜1）をランタイムで合成する。未対応であれば `false`（呼び出し側が画素を減衰させる）。
    fn set_overlay_alpha(&mut self, _id: OverlayId, _alpha: f32) -> bool {
        false
    }
    /// 色の明るさ（1 = そのまま 〜 0 = 黒）をランタイムで乗算する。透明度は変更しない。未対応であれば `false`（呼び出し側が画素を暗くする）。
    fn set_overlay_tint(&mut self, _id: OverlayId, _brightness: f32) -> bool {
        false
    }
    /// 重なった際の描画順。値が大きいほど手前に描画される（既定 0）。奥行きでは決定されない。
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32);
    /// RGBA8、行間にパディングなし。
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]);
    /// [`submit_overlay_external`](Self::submit_overlay_external) で取り込める画像。取り込めなければ `None`
    /// （呼び出し側は RGBA を [`submit_overlay_pixels`](Self::submit_overlay_pixels) で送る）。
    fn import_formats(&self) -> Option<ImportFormats> {
        None
    }
    /// 表示サーバーの画像を GPU 上で取り込み、RGBA に並べ替えて表示する。`image` は GPU が読み終えるまで保持する。
    fn submit_overlay_external(
        &mut self,
        _id: OverlayId,
        _image: &std::sync::Arc<ExternalImage>,
    ) -> Result<(), VrError> {
        Err(VrError::NotImplemented(self.runtime()))
    }
    /// オーバーレイを破棄してリソースを返却する。以降 `id` は使用しない。閉じたパネルが蓄積しないよう、既定は何もしない。
    fn destroy_overlay(&mut self, _id: OverlayId) {}
    /// 直前の [`poll`](Self::poll) でランタイムがアプリのスレッドで合成・表示に要した時間（待機は含まない）。
    /// 合成をランタイム（別プロセス）が実行する場合は 0（novr のエミュレーターのみが返す）。プロファイラ用。
    fn compositor_time(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    /// フレームの周期が確定していれば返す（novr のエミュレーター）。VR ランタイムは vsync で決定され不明のため `None`。
    /// プロファイラがドロップをカウントする基準に使用する。
    fn frame_interval(&self) -> Option<std::time::Duration> {
        None
    }
    /// 直前に合成したフレームで、オーバーレイの画面上の領域が 1 画素でも他のオーバーレイに遮蔽されずに存在したか
    /// （他のパネルに遮蔽された・視野外であれば `false`。自身の透明度は問わない）。
    /// ランタイムが提供しなければ `None`（呼び出し側が視野で推定する）。非可視パネルの再描画を省略するために使用する。
    fn overlay_seen(&self, _id: OverlayId) -> Option<bool> {
        None
    }
    /// シーンのアプリ（ゲーム）への入力をブロック・解除する（ダッシュボード表示中）。ブロック可能なのは OpenVR のみ
    /// （SteamVR Input のアクションセットをオーバーレイの全体優先度で更新し、使用する入力を占有する）。他は何もしない。
    fn block_app_input(&mut self, _block: bool) {}
    /// 両目に映る範囲（両目の視野の重なり）。ランタイムが提供しなければ `None`（呼び出し側は [`Fov::NARROW`] を想定する）。
    fn fov(&self) -> Option<Fov> {
        None
    }
    /// ランタイムの起動と同時にこのアプリを起動するか。切り替え不可のランタイムは `None`（OpenVR のみが保持する）。
    fn auto_launch(&self) -> Option<bool> {
        None
    }
    /// 自動起動を切り替える。有効にすると、ランタイム起動時にこの実行ファイルが起動する。切り替え不可であれば何もしない。
    fn set_auto_launch(&mut self, _on: bool) {}
    /// 疎通確認用: ランタイムへ送信した最後の画像（幅, 高さ, RGBA）を読み戻す。読み戻し不可の実装は `None`。
    /// OpenVR は [`openvr_overlay_image`] でランタイム側の画像を読み戻せるため、これは OpenXR 用。
    fn overlay_image(&mut self, _id: OverlayId) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
}

/// `Box<dyn VrBackend>` もそのままバックエンドとして使用可能（カーネルが具象型・動的型の両方を保持できるように）。
impl<T: VrBackend + ?Sized> VrBackend for Box<T> {
    fn runtime(&self) -> Runtime {
        (**self).runtime()
    }
    fn poll(&mut self) -> Frame {
        (**self).poll()
    }
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        (**self).create_overlay(key, width_m)
    }
    fn max_overlays(&self) -> Option<usize> {
        (**self).max_overlays()
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        (**self).set_overlay_pose(id, pose)
    }
    fn set_overlay_pose_relative(&mut self, id: OverlayId, device: &Device, offset: Pose) -> bool {
        (**self).set_overlay_pose_relative(id, device, offset)
    }
    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        (**self).set_overlay_width(id, width_m)
    }
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        (**self).set_overlay_height(id, height_m)
    }
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        (**self).set_overlay_visible(id, visible)
    }
    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        (**self).set_overlay_alpha(id, alpha)
    }
    fn set_overlay_tint(&mut self, id: OverlayId, brightness: f32) -> bool {
        (**self).set_overlay_tint(id, brightness)
    }
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        (**self).set_overlay_sort(id, order)
    }
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        (**self).submit_overlay_pixels(id, width, height, rgba)
    }
    fn import_formats(&self) -> Option<ImportFormats> {
        (**self).import_formats()
    }
    fn submit_overlay_external(
        &mut self,
        id: OverlayId,
        image: &std::sync::Arc<ExternalImage>,
    ) -> Result<(), VrError> {
        (**self).submit_overlay_external(id, image)
    }
    fn destroy_overlay(&mut self, id: OverlayId) {
        (**self).destroy_overlay(id)
    }
    fn overlay_image(&mut self, id: OverlayId) -> Option<(u32, u32, Vec<u8>)> {
        (**self).overlay_image(id)
    }
    fn compositor_time(&self) -> std::time::Duration {
        (**self).compositor_time()
    }
    fn overlay_seen(&self, id: OverlayId) -> Option<bool> {
        (**self).overlay_seen(id)
    }
    fn frame_interval(&self) -> Option<std::time::Duration> {
        (**self).frame_interval()
    }
    fn block_app_input(&mut self, block: bool) {
        (**self).block_app_input(block)
    }
    fn fov(&self) -> Option<Fov> {
        (**self).fov()
    }
    fn auto_launch(&self) -> Option<bool> {
        (**self).auto_launch()
    }
    fn set_auto_launch(&mut self, on: bool) {
        (**self).set_auto_launch(on)
    }
}

/// 希望のランタイムに接続する。VR ランタイムを希望した（または指定なしの）場合は、
/// インストール済みのもの（OpenVR 優先）にフォールバックする。novr へのフォールバックは呼び出し側が決める。
/// `emulator_gpu` は novr のエミュレーターが GPU で合成するか（`false` なら CPU）。
pub fn connect(prefer: Option<Runtime>, emulator_gpu: bool) -> Result<Box<dyn VrBackend>, VrError> {
    if prefer == Some(Runtime::NoVr) {
        logger::debug!("connecting to runtime", runtime = ?Runtime::NoVr);
        return novr_emulator(emulator_gpu);
    }
    let order = match prefer {
        Some(Runtime::OpenXr) => [Runtime::OpenXr, Runtime::OpenVr],
        _ => [Runtime::OpenVr, Runtime::OpenXr],
    };
    let mut last = VrError::NoRuntime;
    for rt in order {
        match connect_exact(rt) {
            Ok(b) => {
                logger::debug!("connecting to runtime", runtime = ?rt);
                return Ok(b);
            }
            Err(VrError::NoRuntime) => logger::debug!("runtime not found", runtime = ?rt),
            Err(e) => {
                logger::debug!("failed to connect to runtime", runtime = ?rt, error = e);
                last = e;
            }
        }
    }
    Err(last)
}

/// 指定したランタイムだけに接続する（フォールバックしない）。見つからなければ [`VrError::NoRuntime`]。
/// testkit が起動したランタイムを、別のランタイムと取り違えずに掴むため。
pub fn connect_exact(rt: Runtime) -> Result<Box<dyn VrBackend>, VrError> {
    match rt {
        Runtime::NoVr => novr_emulator(true),
        #[cfg(feature = "openvr")]
        Runtime::OpenVr if openvr::available() => openvr::connect(),
        #[cfg(feature = "openxr")]
        Runtime::OpenXr if openxr::available() => openxr::connect(),
        _ => Err(VrError::NoRuntime),
    }
}

#[cfg(feature = "novr")]
fn novr_emulator(gpu: bool) -> Result<Box<dyn VrBackend>, VrError> {
    Ok(Box::new(novr::Emulator::windowed(gpu)))
}

/// `novr` フィーチャーなしのビルドにはエミュレーターが無い。
#[cfg(not(feature = "novr"))]
fn novr_emulator(_: bool) -> Result<Box<dyn VrBackend>, VrError> {
    Err(VrError::NoRuntime)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HUD を置いてよい範囲は、狭い HMD では視野の内側に縮み、広い HMD でも上限を超えない。どれも両目の重なりの内側。
    #[test]
    fn the_hud_safe_area_fits_every_headset() {
        let deg = f32::to_radians;
        // 狭いもの（両目の重なりが片側 35°）・Quest 2 くらい・Index くらい・広いもの（Pimax くらい）。
        let headsets = [
            Fov::even(deg(35.0), deg(35.0)),
            Fov {
                left: deg(42.0),
                right: deg(41.0),
                up: deg(44.0),
                down: deg(50.0),
            },
            Fov::even(deg(50.0), deg(52.0)),
            Fov::even(deg(75.0), deg(55.0)),
        ];
        for fov in headsets {
            let left_eye = Fov {
                right: fov.right + deg(10.0),
                ..fov
            };
            let both = left_eye.overlap(Fov {
                left: fov.left + deg(10.0),
                ..fov
            });
            assert_eq!(both, fov, "両目の重なり");
            let safe = fov.safe();
            for (s, f, max) in [
                (safe.left, fov.left, SAFE_MAX.left),
                (safe.right, fov.right, SAFE_MAX.right),
                (safe.up, fov.up, SAFE_MAX.up),
                (safe.down, fov.down, SAFE_MAX.down),
            ] {
                assert!(s <= f * SAFE_SHARE + 1e-6 && s <= max, "{fov:?} → {safe:?}");
                assert!(s > deg(10.0), "端に寄せられる余地が残る: {safe:?}");
            }
        }
        assert!(Fov::NARROW.valid() && !Fov::even(0.0, 1.0).valid());
    }

    #[test]
    fn pose_inverse_roundtrip() {
        let p = Pose::new(Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_y(0.7));
        let id = p * p.inverse();
        assert!(id.position.abs_diff_eq(Vec3::ZERO, 1e-5));
        assert!(id.orientation.abs_diff_eq(Quat::IDENTITY, 1e-5));
    }

    #[test]
    fn leveled_keeps_heading_and_drops_pitch() {
        // Head facing +X, looking down.
        let head = Pose::new(
            Vec3::new(1.0, 1.6, 0.0),
            Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-0.5),
        );
        let l = head.leveled();
        assert_eq!(l.position, head.position);
        assert!(l.forward().abs_diff_eq(Vec3::X, 1e-5));
        assert!((l.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
    }

    #[test]
    fn panel_up_stays_vertical_while_upright_and_follows_the_head_when_lying() {
        use std::f32::consts::FRAC_PI_2;
        let head = |q: Quat| Pose::new(Vec3::new(0.3, 1.6, 0.0), q);
        // 立っている間: 首を少し傾ける・見上げる・手首を見下ろす（うつむききり手前まで）。真上のまま。
        for q in [
            Quat::IDENTITY,
            Quat::from_rotation_z(0.4),
            Quat::from_rotation_x(0.4),
            Quat::from_rotation_y(2.0) * Quat::from_rotation_x(-1.3) * Quat::from_rotation_z(0.3),
        ] {
            assert!(head(q).panel_up().abs_diff_eq(Vec3::Y, 1e-5), "{q}");
        }
        // 寝転がる: 横向き（右を下に）・仰向け（真上を見る）・仰向けで首を傾ける。頭の上そのもの。
        for q in [
            Quat::from_rotation_z(-FRAC_PI_2),
            Quat::from_rotation_z(FRAC_PI_2),
            Quat::from_rotation_x(FRAC_PI_2),
            Quat::from_rotation_x(FRAC_PI_2) * Quat::from_rotation_z(0.5),
        ] {
            assert!(
                head(q).panel_up().abs_diff_eq(q * Vec3::Y, 1e-5),
                "{q} {}",
                head(q).panel_up()
            );
        }
        // 間はなめらか: 倒していくと少しずつ頭の上へ寄り、跳ばない。
        let mut prev = Vec3::Y;
        for i in 0..=90 {
            let up = head(Quat::from_rotation_z((i as f32).to_radians())).panel_up();
            assert!(up.angle_between(prev) < 0.1, "{i}° {up}");
            prev = up;
        }
    }

    #[test]
    fn leveled_puts_the_panel_in_front_of_the_face_when_lying() {
        use std::f32::consts::FRAC_PI_2;
        // 横向きに寝て、さらに少し見下ろす（足の方を見る）: 顔の正面・頭の上を上に。
        let q = Quat::from_rotation_y(0.7)
            * Quat::from_rotation_z(-FRAC_PI_2)
            * Quat::from_rotation_x(-0.3);
        let head = Pose::new(Vec3::new(0.0, 0.3, 0.0), q);
        let l = head.leveled();
        assert_eq!(l.position, head.position);
        assert!(l.orientation.abs_diff_eq(q, 1e-5), "{} {q}", l.orientation);
        // 仰向け: 真上に出て、上は頭の上（寝ている向き）。
        let q = Quat::from_rotation_y(-1.0) * Quat::from_rotation_x(FRAC_PI_2);
        let l = Pose::new(Vec3::ZERO, q).leveled();
        assert!(l.forward().abs_diff_eq(Vec3::Y, 1e-5), "{}", l.forward());
        assert!((l.orientation * Vec3::Y).abs_diff_eq(q * Vec3::Y, 1e-5));
        // 立っていて首を傾けている・見上げ気味: これまでどおり水平に立てる。
        let q =
            Quat::from_rotation_y(1.0) * Quat::from_rotation_x(0.3) * Quat::from_rotation_z(0.3);
        let l = Pose::new(Vec3::ZERO, q).leveled();
        assert!((l.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
        assert!(l.forward().y.abs() < 1e-5);
        assert!(l.forward().dot(q * Vec3::NEG_Z) > 0.9);
    }

    #[cfg(feature = "novr")]
    #[test]
    fn novr_is_always_available() {
        let b = connect(Some(Runtime::NoVr), true).unwrap();
        assert_eq!(b.runtime(), Runtime::NoVr);
    }

    #[test]
    fn frame_pose_finds_each_device() {
        let at = |x| Pose::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY);
        let frame = Frame {
            hmd: at(1.0),
            controllers: vec![RawController {
                hand: Hand::Left,
                profile: String::new(),
                aim: at(2.0),
                inputs: Vec::new(),
            }],
            trackers: vec![("LHR-1".into(), at(3.0))],
            ..Frame::default()
        };
        assert_eq!(frame.pose(&Device::Hmd), Some(at(1.0)));
        assert_eq!(frame.pose(&Device::Controller(Hand::Left)), Some(at(2.0)));
        assert_eq!(frame.pose(&Device::Controller(Hand::Right)), None);
        assert_eq!(frame.pose(&Device::Tracker("LHR-1".into())), Some(at(3.0)));
        assert_eq!(frame.pose(&Device::Tracker("LHR-2".into())), None);
        let emulated = Frame {
            emulated: true,
            ..frame
        };
        assert_eq!(emulated.pose(&Device::Hmd), None);
    }

    #[test]
    fn every_finger_chains_back_to_the_wrist() {
        for j in 2..JOINT_COUNT {
            let mut at = j;
            let mut steps = 0;
            while let Some(p) = joint_parent(at) {
                assert!(p < at, "{j}");
                at = p;
                steps += 1;
            }
            assert_eq!(at, WRIST, "{j}");
            // 親指は 4 本、ほかの指は 5 本の骨で手首まで。
            assert!(steps <= 5, "{j}");
        }
        assert_eq!(joint_parent(PALM), None);
        assert_eq!(joint_parent(6), Some(WRIST));
        assert_eq!(joint_parent(TIPS[0]), Some(4));
    }

    /// 手のひらを下へ向けた手（指は前方 -Z）。右手は親指・人差し指が左（-X）、左手は右。
    fn palm_down(hand: Hand) -> HandSkeleton {
        let side = match hand {
            Hand::Right => -1.0,
            Hand::Left => 1.0,
        };
        let mut joints = [Pose::IDENTITY; JOINT_COUNT];
        joints[INDEX_PROXIMAL].position = Vec3::new(side * 0.03, 0.0, -0.08);
        joints[LITTLE_PROXIMAL].position = Vec3::new(-side * 0.03, 0.0, -0.07);
        joints[PALM].position = Vec3::new(0.0, 0.0, -0.04);
        HandSkeleton {
            hand,
            joints,
            tracking: FingerTracking::Full,
        }
    }

    /// 手のひらを下にした手の姿勢は、握ったコントローラと同じく指の向きが前方（-Z）、親指の側が上（+Y）、
    /// +X は右手なら手の甲（上）・左手なら手のひら（下）。元は手首から指の方へ 12cm・親指の側へ 4cm。
    #[test]
    fn grip_is_held_like_a_controller() {
        for (hand, thumb, x) in [
            (Hand::Left, Vec3::X, Vec3::NEG_Y),
            (Hand::Right, Vec3::NEG_X, Vec3::Y),
        ] {
            let mut s = palm_down(hand);
            s.joints[WRIST].position = Vec3::new(0.0, 0.0, 0.0);
            s.joints[12].position = Vec3::new(0.0, 0.0, -0.08);
            let g = s.grip();
            assert!(
                g.forward().abs_diff_eq(Vec3::NEG_Z, 1e-5),
                "{hand:?} {}",
                g.forward()
            );
            assert!(
                (g.orientation * Vec3::Y).abs_diff_eq(thumb, 1e-5),
                "{hand:?}"
            );
            assert!((g.orientation * Vec3::X).abs_diff_eq(x, 1e-5), "{hand:?}");
            let tip = thumb * 0.04 + Vec3::new(0.0, 0.0, -0.12);
            assert!(g.position.abs_diff_eq(tip, 1e-5), "{}", g.position);
        }
    }

    #[test]
    fn facing_tells_palm_from_back_for_both_hands() {
        for hand in [Hand::Left, Hand::Right] {
            let s = palm_down(hand);
            assert!(s.palm_normal().abs_diff_eq(Vec3::NEG_Y, 1e-5), "{hand:?}");
            assert_eq!(
                s.facing(Vec3::new(0.0, -0.5, 0.0)),
                Facing::Palm,
                "{hand:?}"
            );
            assert_eq!(s.facing(Vec3::new(0.0, 0.5, 0.0)), Facing::Back, "{hand:?}");
            assert!(s.palm_toward(Vec3::new(0.0, -0.5, -0.04)) > 0.99);
        }
    }

    #[test]
    fn forward_is_neg_z() {
        assert_eq!(Pose::IDENTITY.forward(), Vec3::NEG_Z);
    }
}
