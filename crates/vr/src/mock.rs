//! この層より上のすべての層のテスト用、メモリ上のバックエンド。

use crate::{Frame, OverlayId, Pose, Runtime, VrBackend, VrError};

#[derive(Debug, Default)]
pub struct MockBackend {
    /// 毎回の `poll` で返す値。テストはフレーム間でこれを書き換える。
    pub frame: Frame,
    /// 作成されたオーバーレイごとの (キー, 最後の姿勢, 最後に送られたサイズ)。添字は `OverlayId.0`。
    pub overlays: Vec<(String, Pose, Option<(u32, u32)>)>,
}

impl VrBackend for MockBackend {
    fn runtime(&self) -> Runtime {
        Runtime::OpenXr
    }

    fn poll(&mut self) -> Frame {
        self.frame.clone()
    }

    fn create_overlay(&mut self, key: &str, _width_m: f32) -> Result<OverlayId, VrError> {
        self.overlays.push((key.to_owned(), Pose::IDENTITY, None));
        Ok(OverlayId(self.overlays.len() as u64 - 1))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.overlays[id.0 as usize].1 = pose;
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(rgba.len(), (width * height * 4) as usize, "bad RGBA buffer");
        self.overlays[id.0 as usize].2 = Some((width, height));
    }
}
