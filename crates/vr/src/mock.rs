//! この層より上のすべての層のテスト用、メモリ上のバックエンド。

use crate::{Frame, OverlayId, Pose, Runtime, VrBackend, VrError};

/// モックが記録するオーバーレイ1枚分の状態。
#[derive(Debug)]
pub struct MockOverlay {
    pub key: String,
    /// 最後に設定された姿勢。
    pub pose: Pose,
    pub width_m: f32,
    /// 固定された高さ（m）。
    pub height_m: Option<f32>,
    pub visible: bool,
    pub alpha: f32,
    pub pixel_submissions: usize,
    pub sort: u32,
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
    /// 不透明度 API のないランタイムを再現する。
    pub unsupported_alpha: bool,
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

    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        self.overlays.push(MockOverlay {
            key: key.to_owned(),
            pose: Pose::IDENTITY,
            width_m,
            height_m: None,
            visible: true,
            alpha: 1.0,
            pixel_submissions: 0,
            sort: 0,
            texture: None,
        });
        Ok(OverlayId(self.overlays.len() as u64 - 1))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.overlays[id.0 as usize].pose = pose;
    }

    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        self.overlays[id.0 as usize].width_m = width_m;
    }

    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        self.overlays[id.0 as usize].height_m = Some(height_m);
    }

    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        self.overlays[id.0 as usize].visible = visible;
    }

    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        if self.unsupported_alpha {
            return false;
        }
        self.overlays[id.0 as usize].alpha = alpha.clamp(0.0, 1.0);
        true
    }

    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.overlays[id.0 as usize].sort = order;
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.overlays[id.0 as usize].pixel_submissions += 1;
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
