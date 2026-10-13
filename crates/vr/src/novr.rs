//! novr バックエンド: VR ランタイムを使わない。パネルは OS のウィンドウとして表示され、
//! 入力も OS から直接 UI に届くので、ここでは何もしない（コントローラ無し・オーバーレイは記録だけ）。

use crate::{Frame, OverlayId, Pose, Runtime, Surface, VrBackend, VrError};

#[derive(Debug, Default)]
pub(crate) struct NoVrBackend {
    overlays: u64,
}

impl VrBackend for NoVrBackend {
    fn runtime(&self) -> Runtime {
        Runtime::NoVr
    }

    fn surface(&self) -> Surface {
        Surface::Desktop
    }

    fn poll(&mut self) -> Frame {
        Frame::default()
    }

    fn create_overlay(&mut self, _key: &str, _width_m: f32) -> Result<OverlayId, VrError> {
        self.overlays += 1;
        Ok(OverlayId(self.overlays - 1))
    }

    fn set_overlay_pose(&mut self, _id: OverlayId, _pose: Pose) {}

    fn set_overlay_width(&mut self, _id: OverlayId, _width_m: f32) {}

    fn set_overlay_visible(&mut self, _id: OverlayId, _visible: bool) {}

    fn set_overlay_sort(&mut self, _id: OverlayId, _order: u32) {}

    fn submit_overlay_pixels(&mut self, _id: OverlayId, _w: u32, _h: u32, _rgba: &[u8]) {}
}
