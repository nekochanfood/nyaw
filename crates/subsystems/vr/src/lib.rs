//! ランタイムサブシステム。OpenVR / OpenXR / novr（VR なし）の違いを1つのトレイト [`VrBackend`] の裏に隠す。
//! どれも対等な「手段」で、上位層は [`connect`] から得た `Box<dyn VrBackend>` だけを扱う。
//! novr もデスクトップのウィンドウで VR を模すエミュレーター（[`novr::Emulator`]）なので、上位層から見た違いは無い。
//!
//! 語彙は OpenXR に揃える: コントローラのプロファイルは `"vendor/name"`（例 `"valve/index_controller"`）、
//! 入力は `"component/feature"`（例 `"trigger/value"`）。OpenVR バックエンドはこれに翻訳する。

#[cfg(feature = "novr")]
mod compositor;
// openvr と openxr で使う部分が違うので、片方だけのビルドでは未使用になる。
#[cfg(any(feature = "openvr", feature = "openxr"))]
#[cfg_attr(not(all(feature = "openvr", feature = "openxr")), allow(dead_code))]
mod gpu;
#[cfg(any(test, feature = "test-support"))]
pub mod mock;
#[cfg(feature = "novr")]
pub mod novr;
#[cfg(feature = "openvr")]
mod openvr;

/// testkit の実ランタイムテスト用。上位層からは使わない。
#[cfg(feature = "openvr")]
#[doc(hidden)]
pub use openvr::{
    overlay_image as openvr_overlay_image, overlay_state as openvr_overlay_state,
    runtime_path as openvr_runtime_path,
};
#[cfg(feature = "openxr")]
mod openxr;

use std::fmt;

pub use glam::{Mat3, Quat, Vec2, Vec3};

/// 頭がこの角度（ラジアン）より見上げる・横に倒れると、寝転がりとみなし始める（[`Pose::panel_up`]）。
const LIE_START: f32 = 35.0 * std::f32::consts::PI / 180.0;
/// この角度で寝転がりきったとみなす。間はなめらかに移り、境目でパネルが跳ねない。
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

    /// 頭の姿勢から決める、パネルを立てる上方向。立っている（座っている）間は真上で、首の傾きを無視する。
    /// 寝転がる（見上げる・横に倒す）ほど頭の上へ寄せ、寝転がりきれば頭の上そのもの。頭の位置（高さ）は見ず、回転だけで決める。
    /// うつむきは寝転がりに数えない（手首を見下ろしてもパネルは立てたまま）。
    /// ponytail: 回転だけなので、立って真上を見上げるのも仰向けと区別しない。
    pub fn panel_up(&self) -> Vec3 {
        let (up, right) = (self.orientation * Vec3::Y, self.orientation * Vec3::X);
        // 見上げる角度と左右に倒す角度の大きい方。
        let tilt = self.forward().y.max(right.y.abs()).clamp(0.0, 1.0).asin();
        let t = ((tilt - LIE_START) / (LIE_FULL - LIE_START)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        Vec3::Y.lerp(up, t).normalize_or(Vec3::Y)
    }

    /// 位置はそのまま、上を [`Pose::panel_up`] に立て、前方（-Z）を頭の前方に寄せた姿勢。
    /// 頭の前にパネルを出すのに使う。立っている間は水平方向（Y 軸回り）だけで、うつむき・首の傾きを無視する。
    /// 寝転がっている間は頭の向きそのもので、顔の前に出る。
    pub fn leveled(&self) -> Pose {
        let up = self.panel_up();
        let f = self.forward();
        let Some(back) = (up * f.dot(up) - f).try_normalize() else {
            // 真上・真下を見ている（立っている間のうつむききり）。向きは決まらないので前方 -Z。
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

/// 掴んでいるパネルの拡大縮小（-1..1、+ で大きく）を直接伝える入力名。実機のコントローラには無く、
/// novr のエミュレーターが PageUp / PageDown で出す（テスト用。手を前後に動かせないため）。
pub const RESIZE_INPUT: &str = "nyaw/resize";

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
    /// 指をどれだけ実際に追っているか。
    pub tracking: FingerTracking,
}

/// 骨格の指をどれだけ実際に追っているか（OpenVR の `EVRSkeletalTrackingLevel` に揃える）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerTracking {
    /// 指は追っておらず、ボタンなどから推し量った形（Vive のコントローラ）。
    Estimated,
    /// コントローラを握ったまま、一部の指を追っている（Index・Touch の静電容量）。
    Partial,
    /// 指を全部追っている（光学式のハンドトラッキング・グローブ）。コントローラを握っていない手で、ランタイムがコントローラとして
    /// 見せていても（Steam Link・Virtual Desktop はつまみをトリガーにする）骨格で操作する。
    Full,
}

/// 手のひらと手の甲のどちらを（顔に）向けているか（[`HandSkeleton::facing`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Palm,
    Back,
}

/// 手首の関節から見た [`HandSkeleton::grip`] の元（grip の座標系。m）: 指の方へ 5cm、手のひらの側へ 1.5cm。
/// コントローラ用の手首の時計の相対姿勢（`wrist::WATCH_OFFSET`。tip から手首寄りへ 8cm・上へ 4cm）が、
/// 手首の関節から腕側へ 3cm・手の甲の側へ 2.5cm（腕時計の位置）に来る。
pub const GRIP_FROM_WRIST: Vec3 = Vec3::new(0.0, -0.015, -0.05);

/// 人差し指・小指の基節（手のひらの面を張る関節）。
const INDEX_PROXIMAL: usize = 7;
const LITTLE_PROXIMAL: usize = 22;

impl HandSkeleton {
    /// 手のひらから外へ出る向き（単位ベクトル）。関節の位置だけから求めるので、ランタイムの関節の軸の違いによらない。
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

    /// 手のひらを `eye` へどれだけ向けているか（なす角の cos。1 で正面に手のひら、-1 で手の甲）。
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

    /// コントローラの座標系に合わせた手の姿勢: 前方（-Z）は手首から中指の付け根への向き、上（+Y）は手の甲の側。元は、手で
    /// コントローラを握ったときのその先（`/pose/tip`）に当たる所（手首から [`GRIP_FROM_WRIST`]）。コントローラに取り付けた
    /// パネル（手首の時計など）を、骨格だけの手にも同じ相対姿勢で付けるため。
    // ponytail: 前腕は追っていないので、手首を曲げると手首の時計も手と一緒に傾く。
    pub fn grip(&self) -> Pose {
        let p = |i: usize| self.joints[i].position;
        let back = (p(WRIST) - p(12)).normalize_or(Vec3::Z);
        let up = -self.palm_normal();
        let x = up.cross(back).normalize_or(Vec3::X);
        let orientation = Quat::from_mat3(&Mat3::from_cols(x, back.cross(x), back));
        Pose::new(p(WRIST) + orientation * GRIP_FROM_WRIST, orientation)
    }

    /// 親指の先と `tip`（[`TIPS`] のどれか）の先との距離（m）。つまむ動作の判定に使う。
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
    /// 骨格が取れている手（スケルタル入力。ランタイムが対応していなければ空）。
    pub hands: Vec<HandSkeleton>,
    /// コントローラ以外のトラッカー（腰・足など）。ID はランタイムが決める（OpenVR はシリアル番号）。
    pub trackers: Vec<(String, Pose)>,
    /// ランタイムが終了を求めている（SteamVR の終了、OpenXR セッションの EXITING など）。
    pub quit: bool,
    /// デバイスは実在せず模したもの（novr のエミュレーター）。取り付けたパネルは追従しない（[`Frame::pose`] が `None`）。
    pub emulated: bool,
}

impl Frame {
    /// このフレームでの `device` の姿勢。トラッキングされていなければ（模したデバイスも）`None`。
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
    /// VR を使わず、デスクトップのウィンドウで VR を模す（[`novr::Emulator`]）。
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
    /// 同時に作れるオーバーレイの数の上限（OpenXR は 1 フレームに出せるレイヤー数、OpenVR はランタイム全体の数）。
    /// 無い・分からなければ `None`。超えた分の [`create_overlay`](Self::create_overlay) はエラーになる。
    fn max_overlays(&self) -> Option<usize> {
        None
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose);
    /// `device` の姿勢 * `offset` に置き、追従はランタイムに任せる（アプリが poll した姿勢を送るより遅れない）。
    /// 任せられなければ `false` を返し、呼び出し側は毎 tick [`set_overlay_pose`](Self::set_overlay_pose) で送る。
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
    /// 高さ（m）を幅・テクスチャの縦横比と関係なく固定する（伸び縮みするレーザーの太さを保つため）。
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32);
    /// 作成直後は表示されている。
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool);
    /// 不透明度（0〜1）をランタイムで合成する。未対応なら `false`（呼び出し側が画素を減衰させる）。
    fn set_overlay_alpha(&mut self, _id: OverlayId, _alpha: f32) -> bool {
        false
    }
    /// 色の明るさ（1 = そのまま 〜 0 = 黒）をランタイムで掛ける。透明度は変えない。未対応なら `false`（呼び出し側が画素を暗くする）。
    fn set_overlay_tint(&mut self, _id: OverlayId, _brightness: f32) -> bool {
        false
    }
    /// 重なったときの描画順。大きいほど手前に描かれる（既定 0）。奥行きでは決まらない。
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32);
    /// RGBA8、行間に詰め物なし。
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]);
    /// オーバーレイを捨てて資源を返す。以降 `id` は使わない。閉じたパネルが溜まらないよう、既定は何もしない。
    fn destroy_overlay(&mut self, _id: OverlayId) {}
    /// 直前の [`poll`](Self::poll) でランタイムがアプリのスレッドで合成・表示にかけた時間（待ちは含まない）。
    /// 合成をランタイム（別プロセス）がするなら 0（novr のエミュレーターだけが返す）。プロファイラ用。
    fn compositor_time(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    /// フレームの周期が決まっていれば返す（novr のエミュレーター）。VR ランタイムは vsync で決まり分からないので `None`。
    /// プロファイラがドロップを数える基準に使う。
    fn frame_interval(&self) -> Option<std::time::Duration> {
        None
    }
    /// 直前に合成した絵で、オーバーレイの画面上の領域が 1 画素でもほかのオーバーレイに隠されずにあったか
    /// （ほかのパネルに隠れた・視野の外なら `false`。自分が透明かどうかは問わない）。
    /// ランタイムが教えてくれなければ `None`（呼び出し側が視野で見積もる）。見えないパネルの描き直しを省くのに使う。
    fn overlay_seen(&self, _id: OverlayId) -> Option<bool> {
        None
    }
    /// 疎通確認用: ランタイムへ渡した最後の画像（幅, 高さ, RGBA）を読み戻す。読めない実装は `None`。
    /// OpenVR は [`openvr_overlay_image`] でランタイム側の画像を読めるので、これは OpenXR 用。
    fn overlay_image(&mut self, _id: OverlayId) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
}

/// `Box<dyn VrBackend>` もそのままバックエンドとして使える（カーネルが具象型・動的型の両方を持てるように）。
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

    /// 手のひらを下にした手の姿勢は、指の向きが前方（-Z）、手の甲が上（+Y）。
    #[test]
    fn grip_faces_along_the_fingers_with_the_back_up() {
        for hand in [Hand::Left, Hand::Right] {
            let mut s = palm_down(hand);
            s.joints[WRIST].position = Vec3::new(0.0, 0.0, 0.0);
            s.joints[12].position = Vec3::new(0.0, 0.0, -0.08);
            let g = s.grip();
            assert!(g.forward().abs_diff_eq(Vec3::NEG_Z, 1e-5), "{hand:?} {}", g.forward());
            assert!((g.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5), "{hand:?}");
            // 元は手首から指の方へ 5cm・手のひらの側（下）へ 1.5cm。時計の相対姿勢は手首から腕側・手の甲の側に来る。
            assert!(g.position.abs_diff_eq(Vec3::new(0.0, -0.015, -0.05), 1e-5), "{}", g.position);
            let watch = g.transform_point(Vec3::new(0.0, 0.04, 0.08));
            assert!(watch.abs_diff_eq(Vec3::new(0.0, 0.025, 0.03), 1e-5), "{watch}");
        }
    }

    #[test]
    fn facing_tells_palm_from_back_for_both_hands() {
        for hand in [Hand::Left, Hand::Right] {
            let s = palm_down(hand);
            assert!(s.palm_normal().abs_diff_eq(Vec3::NEG_Y, 1e-5), "{hand:?}");
            assert_eq!(s.facing(Vec3::new(0.0, -0.5, 0.0)), Facing::Palm, "{hand:?}");
            assert_eq!(s.facing(Vec3::new(0.0, 0.5, 0.0)), Facing::Back, "{hand:?}");
            assert!(s.palm_toward(Vec3::new(0.0, -0.5, -0.04)) > 0.99);
        }
    }

    #[test]
    fn forward_is_neg_z() {
        assert_eq!(Pose::IDENTITY.forward(), Vec3::NEG_Z);
    }
}
