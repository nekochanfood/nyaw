//! Layer 1: ランタイム。OpenVR / OpenXR / novr（VR なし）の違いを1つのトレイト [`VrBackend`] の裏に隠す。
//! どれも対等な「手段」で、上位層は [`connect`] から得た `Box<dyn VrBackend>` だけを扱う。
//! 基本操作はどのランタイムでもネイティブに行える。違いは [`Surface`]（表示と入力の経路）だけで表す。
//!
//! 語彙は OpenXR に揃える: コントローラのプロファイルは `"vendor/name"`（例 `"valve/index_controller"`）、
//! 入力は `"component/feature"`（例 `"trigger/value"`）。OpenVR バックエンドはこれに翻訳する。

mod gpu;
#[cfg(any(test, feature = "test-support"))]
pub mod mock;
mod novr;
mod openvr;
mod openxr;

use std::fmt;

pub use glam::{Quat, Vec2, Vec3};

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

    pub fn new(position: Vec3, orientation: Quat) -> Self {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    OpenVr,
    OpenXr,
    /// VR を使わずデスクトップで動かす。
    NoVr,
}

/// パネルをどう見せ、入力をどこから受けるか。上位層がランタイムごとに変えてよいのはここだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// 描画を読み戻して VR のオーバーレイへ送る。入力はコントローラ（[`VrBackend::poll`]）から。
    Overlay,
    /// OS のウィンドウとしてそのまま表示する。入力も OS（マウス等）からネイティブに受ける。
    Desktop,
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
    fn surface(&self) -> Surface;
    /// ランタイムのイベントを処理し、このフレームの姿勢と生のコントローラ入力を返す。
    fn poll(&mut self) -> Frame;
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError>;
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose);
    /// RGBA8、行間に詰め物なし。
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]);
}

/// 希望のランタイムに接続する。VR ランタイムを希望した（または指定なしの）場合は、
/// インストール済みのもの（OpenVR 優先）にフォールバックする。novr へのフォールバックは呼び出し側が決める。
pub fn connect(prefer: Option<Runtime>) -> Result<Box<dyn VrBackend>, VrError> {
    if prefer == Some(Runtime::NoVr) {
        return Ok(Box::new(novr::NoVrBackend::default()));
    }
    let order = match prefer {
        Some(Runtime::OpenXr) => [Runtime::OpenXr, Runtime::OpenVr],
        _ => [Runtime::OpenVr, Runtime::OpenXr],
    };
    let mut last = VrError::NoRuntime;
    for rt in order {
        let attempt = match rt {
            Runtime::OpenVr if openvr::available() => openvr::connect(),
            Runtime::OpenXr if openxr::available() => openxr::connect(),
            _ => continue,
        };
        match attempt {
            Ok(b) => return Ok(b),
            Err(e) => last = e,
        }
    }
    Err(last)
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
    fn novr_is_always_available_and_desktop() {
        let b = connect(Some(Runtime::NoVr)).unwrap();
        assert_eq!(
            (b.runtime(), b.surface()),
            (Runtime::NoVr, Surface::Desktop)
        );
    }

    #[test]
    fn forward_is_neg_z() {
        assert_eq!(Pose::IDENTITY.forward(), Vec3::NEG_Z);
    }
}
