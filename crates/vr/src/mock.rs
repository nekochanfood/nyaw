//! この層より上のすべての層のテスト用、メモリ上のバックエンド。

use crate::{Frame, OverlayId, Pose, Runtime, VrBackend, VrError};

/// モックが記録するオーバーレイ1枚分の状態。
#[derive(Debug)]
pub struct MockOverlay {
    pub key: String,
    /// 最後に設定された姿勢。
    pub pose: Pose,
    /// 最後に送られたテクスチャ。
    pub texture: Option<MockTexture>,
}

/// モックが受け取ったテクスチャ（RGBA8）。
#[derive(Debug, Clone, PartialEq)]
pub struct MockTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl MockTexture {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().unwrap()
    }
}

#[derive(Debug, Default)]
pub struct MockBackend {
    /// 毎回の `poll` で返す値。テストはフレーム間でこれを書き換える。
    pub frame: Frame,
    /// 作成されたオーバーレイ。添字は `OverlayId.0`。
    pub overlays: Vec<MockOverlay>,
}

impl VrBackend for MockBackend {
    fn runtime(&self) -> Runtime {
        Runtime::OpenXr
    }

    fn poll(&mut self) -> Frame {
        self.frame.clone()
    }

    fn create_overlay(&mut self, key: &str, _width_m: f32) -> Result<OverlayId, VrError> {
        self.overlays.push(MockOverlay {
            key: key.to_owned(),
            pose: Pose::IDENTITY,
            texture: None,
        });
        Ok(OverlayId(self.overlays.len() as u64 - 1))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.overlays[id.0 as usize].pose = pose;
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "RGBA バッファのサイズが不正"
        );
        self.overlays[id.0 as usize].texture = Some(MockTexture {
            width,
            height,
            rgba: rgba.to_vec(),
        });
    }
}
