//! カーネル最下部の描画サブシステム。描画先・読み戻し・ランタイムへの転送と能力差を処理する。
//! パネルの配置、入力、アニメーションの進行は overlay が決める。

#[cfg(feature = "gpui")]
pub mod gpui;

use vr::{Device, OverlayId, Pose, VrBackend, VrError};

/// ランタイム上の描画先。最後の画像と送信済み状態、解放状態を保持する。
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    id: OverlayId,
    /// ランタイムのオーバーレイのキー（[`Self::restore`] で作り直すときに使う）。
    key: String,
    texture: Option<(u32, u32, Vec<u8>)>,
    released: bool,
    /// ランタイム側のオーバーレイのみを破棄済み（[`Self::park`]）。画像・描画順は保持したまま。
    parked: bool,
    /// 描画順（再作成時に再設定する）。
    sort: u32,
    sent: Option<(Option<Device>, Pose)>,
    shown: Option<(bool, f32)>,
    /// 最後に送信した暗さ。
    dim: f32,
}

impl Surface {
    pub fn new(vr: &mut dyn VrBackend, key: &str, width: f32, pose: Pose) -> Result<Self, VrError> {
        let id = vr.create_overlay(key, width)?;
        vr.set_overlay_pose(id, pose);
        Ok(Self {
            id,
            key: key.to_owned(),
            texture: None,
            released: false,
            parked: false,
            sort: 0,
            sent: None,
            shown: None,
            dim: 0.0,
        })
    }

    pub fn id(&self) -> OverlayId {
        self.id
    }
    pub fn released(&self) -> bool {
        self.released
    }
    pub fn has_texture(&self) -> bool {
        self.texture.is_some()
    }

    /// ランタイム側のオーバーレイを解放済み（[`Self::park`]）。
    pub fn parked(&self) -> bool {
        self.parked
    }

    /// ランタイムへ送信可能（解放も park もしていない）。
    fn live(&self) -> bool {
        !self.released && !self.parked
    }

    /// [`Self::show`] で非表示にしている（一度も呼ばれていなければ表示中）か、オーバーレイを park 済み。
    pub fn hidden(&self) -> bool {
        self.parked || self.shown.is_some_and(|(visible, _)| !visible)
    }

    /// ランタイム側のオーバーレイのみを破棄する（ランタイムの枠を空ける）。画像・描画順は保持したままで、
    /// park 中に届いた画像も保持し、[`Self::restore`] で再作成して送信する。
    pub fn park(&mut self, vr: &mut dyn VrBackend) {
        if !self.live() {
            return;
        }
        vr.destroy_overlay(self.id);
        self.parked = true;
        self.sent = None;
    }

    /// [`Self::park`] で解放したオーバーレイを、同じキー・描画順で幅 `width`・姿勢 `pose` に再作成する。
    /// 画像は不透明度 `opacity` で再送信する（フェードインはここから開始する）。park 状態でなければ何もしない。
    pub fn restore(
        &mut self,
        vr: &mut dyn VrBackend,
        width: f32,
        pose: Pose,
        opacity: f32,
        dim: f32,
    ) -> Result<(), VrError> {
        if self.released || !self.parked {
            return Ok(());
        }
        self.id = vr.create_overlay(&self.key, width)?;
        vr.set_overlay_pose(self.id, pose);
        vr.set_overlay_sort(self.id, self.sort);
        // 非表示にしていた（[`Self::show`]）場合は非表示を維持する。
        if self.shown.is_some_and(|(visible, _)| !visible) {
            vr.set_overlay_visible(self.id, false);
        }
        self.shown = self.shown.map(|(visible, _)| (visible, width));
        (self.parked, self.sent, self.dim) = (false, Some((None, pose)), 0.0);
        self.present(vr, opacity, dim, true);
        Ok(())
    }

    pub fn store(&mut self, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "invalid RGBA buffer size"
        );
        // park 中も保持する（再作成時に送信する）。
        if !self.released {
            self.texture = Some((width, height, rgba.to_vec()));
        }
    }

    /// ネイティブの追従が無ければ、呼び出し側で計算した絶対姿勢へフォールバックする。
    /// 追従は毎回再要求する（同一であれば送信しないのはバックエンドの責務）。取り付け先のデバイスが消失・入れ替わった
    /// （ハンドトラッキングとコントローラの持ち替え）フレームは拒否され、当たり判定と同じ `pose` に配置される。
    pub fn place(&mut self, vr: &mut dyn VrBackend, pose: Pose, follow: Option<&(Device, Pose)>) {
        if !self.live() {
            return;
        }
        if let Some((device, offset)) = follow
            && vr.set_overlay_pose_relative(self.id, device, *offset)
        {
            self.sent = Some((Some(device.clone()), *offset));
            return;
        }
        let want = Some((None, pose));
        if self.sent != want {
            vr.set_overlay_pose(self.id, pose);
            self.sent = want;
        }
    }

    pub fn set_width(&self, vr: &mut dyn VrBackend, width: f32) {
        if self.live() {
            vr.set_overlay_width(self.id, width);
        }
    }

    pub fn set_sort(&mut self, vr: &mut dyn VrBackend, sort: u32) {
        self.sort = sort;
        if self.live() {
            vr.set_overlay_sort(self.id, sort);
        }
    }

    // ponytail: 高さは作り直しで付け直さない（使うのはレーザーだけで、手放さない）。手放すものに使うなら覚えておく。
    pub fn set_height(&self, vr: &mut dyn VrBackend, height: f32) {
        if self.live() {
            vr.set_overlay_height(self.id, height);
        }
    }

    /// 非表示にする際は最後に表示した幅を維持する。
    pub fn show(
        &mut self,
        vr: &mut dyn VrBackend,
        visible: bool,
        width: Option<f32>,
        initial: f32,
    ) {
        if !self.live() {
            return;
        }
        let width = width.unwrap_or_else(|| self.shown.map_or(initial, |s| s.1));
        if self.shown.map(|s| s.0) != Some(visible) {
            vr.set_overlay_visible(self.id, visible);
        }
        if self.shown.map(|s| s.1) != Some(width) {
            vr.set_overlay_width(self.id, width);
        }
        self.shown = Some((visible, width));
    }

    /// 描画先を解放する。解放タイミングは呼び出し側が決定する。park 中のオーバーレイは破棄済み。
    pub fn release(&mut self, vr: &mut dyn VrBackend) {
        if self.released {
            return;
        }
        if !self.parked {
            vr.set_overlay_alpha(self.id, 0.0);
            vr.set_overlay_visible(self.id, false);
            vr.submit_overlay_pixels(self.id, 1, 1, &[0; 4]);
            vr.destroy_overlay(self.id);
        }
        self.released = true;
        self.texture = None;
    }

    /// 不透明度 API のないランタイムでは、加工前の画像から画素を減衰させる。
    /// `dim`（0 = そのまま 〜 1 = 真っ黒）は色のみを黒へ寄せ、透明度は変更しない。ランタイムが明るさを乗算可能であれば画素には触れない
    /// （フェード中に毎フレーム画素を再生成して送信すると詰まる）。乗算不可の場合のみ、変更時に画素を暗くして再送信する。
    pub fn present(&mut self, vr: &mut dyn VrBackend, opacity: f32, dim: f32, upload: bool) {
        if !self.live() {
            return;
        }
        let native = vr.set_overlay_alpha(self.id, opacity);
        let tinted = vr.set_overlay_tint(self.id, 1.0 - dim);
        let upload = upload || !tinted && dim != self.dim;
        let dim = if tinted { 0.0 } else { dim };
        self.dim = dim;
        if native && !upload {
            return;
        }
        if let Some((width, height, rgba)) = &self.texture {
            let fade = if native { 1.0 } else { opacity };
            if fade == 1.0 && dim == 0.0 {
                vr.submit_overlay_pixels(self.id, *width, *height, rgba);
            } else {
                // 両 VR バックエンドの事前乗算アルファに合わせ、RGB も同じ割合で減らす。
                let alpha = (fade * 65536.0).round() as u32;
                let color = (fade * (1.0 - dim) * 65536.0).round() as u32;
                let faded: Vec<u8> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|px| {
                        let f = |v: u8, k: u32| ((v as u32 * k + 32768) >> 16) as u8;
                        [
                            f(px[0], color),
                            f(px[1], color),
                            f(px[2], color),
                            f(px[3], alpha),
                        ]
                    })
                    .collect();
                vr.submit_overlay_pixels(self.id, *width, *height, &faded);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::{Vec3, mock::MockBackend};

    #[test]
    fn native_alpha_keeps_pixels_and_skips_reupload() {
        let mut vr = MockBackend::default();
        let mut surface = Surface::new(&mut vr, "native", 1.0, Pose::IDENTITY).unwrap();
        surface.store(1, 1, &[200, 100, 50, 255]);
        surface.present(&mut vr, 0.0, 0.0, true);
        surface.present(&mut vr, 0.5, 0.0, false);
        assert_eq!(vr.overlays[0].alpha, 0.5);
        assert_eq!(vr.overlays[0].pixel_submissions, 1);
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().rgba,
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn fallback_fades_from_original_pixels_and_can_restore_them() {
        let mut vr = MockBackend {
            unsupported_alpha: true,
            ..Default::default()
        };
        let mut surface = Surface::new(&mut vr, "fallback", 1.0, Pose::IDENTITY).unwrap();
        surface.store(1, 1, &[200, 100, 50, 255]);
        for (opacity, expected) in [
            (0.5, [100, 50, 25, 128]),
            (0.25, [50, 25, 13, 64]),
            (1.0, [200, 100, 50, 255]),
        ] {
            surface.present(&mut vr, opacity, 0.0, false);
            assert_eq!(vr.overlays[0].texture.as_ref().unwrap().rgba, expected);
        }
    }

    #[test]
    fn dim_is_native_when_the_runtime_can_tint() {
        let mut vr = MockBackend::default();
        let mut surface = Surface::new(&mut vr, "tint", 1.0, Pose::IDENTITY).unwrap();
        surface.store(1, 1, &[200, 100, 50, 128]);
        surface.present(&mut vr, 1.0, 0.0, true);
        let sent = vr.overlays[0].pixel_submissions;
        for dim in [0.2, 0.45, 0.0] {
            surface.present(&mut vr, 1.0, dim, false);
            assert_eq!(vr.overlays[0].tint, 1.0 - dim);
        }
        assert_eq!(vr.overlays[0].pixel_submissions, sent, "no pixel work");
        assert_eq!(
            vr.overlays[0].texture.as_ref().unwrap().rgba,
            [200, 100, 50, 128]
        );
    }

    #[test]
    fn dim_darkens_color_only_and_resends_when_it_changes() {
        for unsupported_alpha in [false, true] {
            let mut vr = MockBackend {
                unsupported_alpha,
                unsupported_tint: true,
                ..Default::default()
            };
            let mut surface = Surface::new(&mut vr, "dim", 1.0, Pose::IDENTITY).unwrap();
            surface.store(1, 1, &[200, 100, 50, 128]);
            surface.present(&mut vr, 1.0, 0.5, false);
            assert_eq!(
                vr.overlays[0].texture.as_ref().unwrap().rgba,
                [100, 50, 25, 128]
            );
            let sent = vr.overlays[0].pixel_submissions;
            surface.present(&mut vr, 1.0, 0.0, false);
            assert_eq!(vr.overlays[0].pixel_submissions, sent + 1);
            assert_eq!(
                vr.overlays[0].texture.as_ref().unwrap().rgba,
                [200, 100, 50, 128]
            );
        }
    }

    #[test]
    fn absolute_pose_fallback_and_visibility_keep_the_last_width() {
        let mut vr = MockBackend::default();
        let mut surface = Surface::new(&mut vr, "pose", 1.0, Pose::IDENTITY).unwrap();
        let pose = Pose {
            position: Vec3::new(1.0, 2.0, -3.0),
            ..Pose::IDENTITY
        };
        surface.place(&mut vr, pose, Some(&(Device::Hmd, Pose::IDENTITY)));
        assert_eq!(vr.overlays[0].pose, pose);
        surface.show(&mut vr, true, Some(2.0), 1.0);
        surface.show(&mut vr, false, None, 1.0);
        assert!(!vr.overlays[0].visible);
        assert_eq!(vr.overlays[0].width_m, 2.0);
    }

    #[test]
    fn released_surface_discards_pixels_and_ignores_later_updates() {
        let mut vr = MockBackend::default();
        let mut surface = Surface::new(&mut vr, "close", 1.0, Pose::IDENTITY).unwrap();
        surface.store(1, 1, &[255; 4]);
        surface.present(&mut vr, 0.0, 0.0, true);
        assert!(!surface.released() && surface.has_texture());
        surface.release(&mut vr);
        assert!(surface.released());
        assert!(!surface.has_texture());
        let submissions = vr.overlays[0].pixel_submissions;
        surface.store(1, 1, &[255; 4]);
        surface.present(&mut vr, 1.0, 0.0, true);
        surface.release(&mut vr);
        surface.show(&mut vr, true, Some(5.0), 1.0);
        surface.set_width(&mut vr, 5.0);
        surface.set_height(&mut vr, 5.0);
        surface.set_sort(&mut vr, 5);
        surface.place(
            &mut vr,
            Pose {
                position: Vec3::X,
                ..Pose::IDENTITY
            },
            None,
        );
        assert!(!surface.has_texture());
        assert_eq!(vr.overlays[0].pixel_submissions, submissions);
        assert_eq!(vr.overlays[0].texture.as_ref().unwrap().rgba, [0; 4]);
        assert!(!vr.overlays[0].visible);
        assert_eq!(vr.overlays[0].pose, Pose::IDENTITY);
        assert_eq!(vr.overlays[0].width_m, 1.0);
        assert_eq!(vr.overlays[0].height_m, None);
        assert_eq!(vr.overlays[0].sort, 0);
    }

    #[test]
    fn parked_surface_keeps_its_image_and_comes_back_as_a_new_overlay() {
        let mut vr = MockBackend::default();
        let mut surface = Surface::new(&mut vr, "bar", 1.0, Pose::IDENTITY).unwrap();
        surface.set_sort(&mut vr, 5);
        surface.store(1, 1, &[255; 4]);
        surface.present(&mut vr, 1.0, 0.0, true);
        surface.park(&mut vr);
        assert!(surface.parked() && surface.hidden() && !surface.released());
        assert!(vr.overlays[0].destroyed);
        // 手放している間はランタイムに何も送らないが、届いた画像は覚えておく。
        surface.store(1, 1, &[9; 4]);
        surface.present(&mut vr, 1.0, 0.0, true);
        surface.set_width(&mut vr, 3.0);
        assert_eq!(vr.overlays[0].pixel_submissions, 1);
        assert_eq!(vr.overlays.len(), 1);
        let pose = Pose {
            position: Vec3::Y,
            ..Pose::IDENTITY
        };
        surface.restore(&mut vr, 2.0, pose, 0.5, 0.0).unwrap();
        assert!(!surface.parked() && !surface.hidden());
        assert_eq!(surface.id(), OverlayId(1));
        let o = &vr.overlays[1];
        assert_eq!((o.key.as_str(), o.width_m, o.sort), ("bar", 2.0, 5));
        assert_eq!((o.pose, o.alpha), (pose, 0.5));
        assert_eq!(o.texture.as_ref().unwrap().rgba, [9; 4]);
        // 手放したまま解放しても、捨てたオーバーレイには触らない。
        surface.park(&mut vr);
        surface.release(&mut vr);
        assert!(surface.released() && vr.overlays[1].destroyed);
        assert_eq!(vr.overlays[1].pixel_submissions, 1);
    }

    #[test]
    #[should_panic(expected = "invalid RGBA buffer size")]
    fn rejects_invalid_pixels() {
        let mut vr = MockBackend::default();
        Surface::new(&mut vr, "invalid", 1.0, Pose::IDENTITY)
            .unwrap()
            .store(2, 1, &[0; 4]);
    }
}
