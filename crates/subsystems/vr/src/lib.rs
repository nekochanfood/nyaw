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

    /// 位置はそのまま、向きを水平方向（Y 軸回り）だけにした姿勢。前方は -Z。
    /// 頭の前にパネルを出すとき、うつむき・首の傾きを無視して立てて置くのに使う。
    pub fn leveled(&self) -> Pose {
        let f = self.forward();
        Pose::new(self.position, Quat::from_rotation_y((-f.x).atan2(-f.z)))
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub hmd: Pose,
    pub controllers: Vec<RawController>,
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
    fn forward_is_neg_z() {
        assert_eq!(Pose::IDENTITY.forward(), Vec3::NEG_Z);
    }
}
