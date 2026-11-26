//! この層より上のすべての層のテスト用、メモリ上のバックエンド。

use std::sync::Arc;

use crate::{ExternalImage, Frame, ImportFormats, OverlayId, Pose, Runtime, VrBackend, VrError};

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
    /// 色の明るさ（[`VrBackend::set_overlay_tint`]）。
    pub tint: f32,
    pub pixel_submissions: usize,
    /// 外部の画像（[`VrBackend::submit_overlay_external`]）を受け取った回数。
    pub external_submissions: usize,
    pub sort: u32,
    /// 最後に送られたテクスチャ。
    pub texture: Option<MockTexture>,
    /// 捨てられた（[`VrBackend::destroy_overlay`]）。
    pub destroyed: bool,
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
    /// 明るさの合成に未対応（画素を暗くして送信させる）。
    pub unsupported_tint: bool,
    /// 毎回の `poll` で返す値。テストはフレーム間でこの値を書き換えて使用する。
    pub frame: Frame,
    /// 作成されたオーバーレイ。添字は `OverlayId.0`。
    pub overlays: Vec<MockOverlay>,
    /// シーンのアプリへの入力をブロック中（[`VrBackend::block_app_input`]）。
    pub app_input_blocked: bool,
    /// 自動起動（[`VrBackend::auto_launch`]）。`None` は切り替え不可のランタイム。
    pub auto_launch: Option<bool>,
    /// 両目に映る範囲（[`VrBackend::fov`]）。
    pub fov: Option<crate::Fov>,
    /// 取り込める外部の画像（[`VrBackend::import_formats`]）。`None` なら RGBA だけを受け付ける。
    pub import: Option<ImportFormats>,
    /// 最後に受け取った外部の画像（GPU の読み出しが終わるまで保持する実装を再現する）。
    pub held: Option<Arc<ExternalImage>>,
}

impl VrBackend for MockBackend {
    fn runtime(&self) -> Runtime {
        Runtime::OpenXr
    }

    fn poll(&mut self) -> Frame {
        self.frame.clone()
    }

    fn auto_launch(&self) -> Option<bool> {
        self.auto_launch
    }

    fn fov(&self) -> Option<crate::Fov> {
        self.fov
    }

    fn set_auto_launch(&mut self, on: bool) {
        if self.auto_launch.is_some() {
            self.auto_launch = Some(on);
        }
    }

    fn block_app_input(&mut self, block: bool) {
        self.app_input_blocked = block;
    }

    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        self.overlays.push(MockOverlay {
            key: key.to_owned(),
            pose: Pose::IDENTITY,
            width_m,
            height_m: None,
            visible: true,
            alpha: 1.0,
            tint: 1.0,
            pixel_submissions: 0,
            external_submissions: 0,
            sort: 0,
            texture: None,
            destroyed: false,
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

    fn set_overlay_tint(&mut self, id: OverlayId, brightness: f32) -> bool {
        if self.unsupported_tint {
            return false;
        }
        self.overlays[id.0 as usize].tint = brightness.clamp(0.0, 1.0);
        true
    }

    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.overlays[id.0 as usize].sort = order;
    }

    fn destroy_overlay(&mut self, id: OverlayId) {
        self.overlays[id.0 as usize].destroyed = true;
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.overlays[id.0 as usize].pixel_submissions += 1;
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "invalid RGBA buffer size"
        );
        self.overlays[id.0 as usize].texture = Some(MockTexture {
            width,
            height,
            rgba: rgba.to_vec(),
        });
    }

    fn import_formats(&self) -> Option<ImportFormats> {
        self.import.clone()
    }

    /// 共有メモリの画像は CPU で RGBA に並べ替えて記録する（GPU の取り込みと同じ結果）。DMA-BUF は大きさだけを記録する。
    fn submit_overlay_external(
        &mut self,
        id: OverlayId,
        image: &Arc<ExternalImage>,
    ) -> Result<(), VrError> {
        if self.import.is_none() {
            return Err(VrError::NotImplemented(self.runtime()));
        }
        let rgba = match image.host_bytes() {
            Some((bytes, stride)) => (0..image.height as usize)
                .flat_map(|y| {
                    bytes[y * stride..y * stride + image.width as usize * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| {
                            let [r, g, b] = if image.format.bgr() {
                                [p[2], p[1], p[0]]
                            } else {
                                [p[0], p[1], p[2]]
                            };
                            [r, g, b, if image.format.opaque() { 255 } else { p[3] }]
                        })
                        .collect::<Vec<_>>()
                })
                .flatten()
                .collect(),
            None => Vec::new(),
        };
        let o = &mut self.overlays[id.0 as usize];
        o.external_submissions += 1;
        o.texture = Some(MockTexture {
            width: image.width,
            height: image.height,
            rgba,
        });
        self.held = Some(image.clone());
        Ok(())
    }
}
