//! ランタイムを通すキット（[`crate::VrKit`] / [`crate::NoVrKit`]）に共通の、パネルの GPUI ウィンドウとフレームの進め方。

use std::collections::HashMap;

use gpui::{AnyWindowHandle, AppContext, Entity, PlatformInput, Render, VisualTestContext};
use kernel::{Hand, Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec, PointerBridge};
use vr::VrBackend;

use crate::watch::Watch;
use crate::{Pose, Vec2, raster};

/// パネルの幅（m）。高さはウィンドウの縦横比で決まる。
pub(crate) const PANEL_WIDTH_M: f32 = 1.0;

pub(crate) struct Panels<V: 'static> {
    /// 添字は PanelId。画面型が違うパネル（召喚されたもの）は `None`。
    pub(crate) views: Vec<Option<Entity<V>>>,
    pub(crate) windows: Vec<AnyWindowHandle>,
    bridges: HashMap<(PanelId, Hand), PointerBridge>,
    /// 画面が開くよう頼んだ Web のページ（[`ui::PanelSpec::web`]）。テストプラットフォームでは開かない。
    pub(crate) web_spawns: Vec<String>,
    /// `Some` なら各パネルの描画をウィンドウに出す（`NYAW_E2E_WATCH`）。
    pub(crate) watch: Option<Watch>,
}

impl<V: Render> Panels<V> {
    pub(crate) fn new(watch: Option<Watch>) -> Self {
        Self {
            views: Vec::new(),
            windows: Vec::new(),
            bridges: HashMap::new(),
            web_spawns: Vec::new(),
            watch,
        }
    }

    /// ビュー `view`（ウィンドウ `window`）を `pose` のパネルとして足す。パネル解像度 = ウィンドウサイズ。
    pub(crate) fn add<B: VrBackend>(
        &mut self,
        cx: &mut VisualTestContext,
        kernel: &mut Kernel<B>,
        pose: Pose,
        view: Option<Entity<V>>,
    ) -> PanelId {
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let size = Vec2::new(PANEL_WIDTH_M, PANEL_WIDTH_M * res.1 as f32 / res.0 as f32);
        let id = kernel
            .add_panel(&format!("testkit.{}", self.windows.len()), pose, size, res)
            .expect("パネルを作れない");
        assert_eq!(id, self.windows.len(), "PanelId は開いた順の添字");
        // 通常の操作テストは時間に依存させない。アニメーションは overlay / kernel で検証する。
        kernel.apply(id, kernel::PanelCommand::Animated(false));
        self.views.push(view);
        self.windows.push(window);
        id
    }

    /// 本番と同じ `ui::pump` で1フレーム進めて GPUI に配送し、ビューのパネル操作・召喚を反映して、描画をランタイムへ送る。
    pub(crate) fn step<B: VrBackend>(
        &mut self,
        cx: &mut VisualTestContext,
        kernel: &mut Kernel<B>,
    ) {
        let mut inputs: Vec<Vec<PlatformInput>> =
            (0..self.windows.len()).map(|_| Vec::new()).collect();
        for (panel, input) in ui::pump(kernel, &mut self.bridges) {
            inputs[panel].push(input);
        }
        for (panel, (window, inputs)) in self.windows.iter().copied().zip(inputs).enumerate() {
            cx.cx
                .update_window(window, |_, window, cx| {
                    for input in inputs {
                        window.dispatch_event(input, cx);
                    }
                    ui::sync_panel(kernel, panel, cx);
                })
                .unwrap();
        }
        cx.run_until_parked();
        let spawns = cx
            .cx
            .update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
        for spec in spawns {
            self.add_spec(cx, kernel, spec);
        }
        cx.run_until_parked();
        for (panel, window) in self.windows.iter().copied().enumerate() {
            let frame = cx
                .cx
                .update_window(window, |_, window, cx| raster::capture(window, cx))
                .unwrap();
            if let Some(w) = &mut self.watch {
                w.show(panel, &frame);
            }
            kernel.submit(panel, frame.width, frame.height, &frame.rgba);
        }
        if let Some(w) = &self.watch {
            w.wait();
        }
    }

    /// 画面から召喚されたパネルを開く（本番の `ui::run` と同じく `pose_for` で置く）。
    fn add_spec<B: VrBackend>(
        &mut self,
        cx: &mut VisualTestContext,
        kernel: &mut Kernel<B>,
        spec: PanelSpec,
    ) {
        // Web のページ（Tauri の WebView）はテストプラットフォームでは開けないので、頼まれたことだけ記録する。
        // 実際に開いて操作するのは crates/ui/tests/web_*.rs。
        if let Some(page) = spec.web_page() {
            self.web_spawns.push(page.to_owned());
            return;
        }
        let id = self.windows.len();
        let pose = spec.pose_for(kernel, id);
        let (root, cx) = cx
            .cx
            .add_window_view(|window, cx| PanelRoot::new(spec.build(id, window, cx)));
        let view = root.read_with(cx, |root, _| root.view().downcast::<V>().ok());
        let panel = self.add(cx, kernel, pose, view);
        cx.update(|_, cx| ui::sync_panel(kernel, panel, cx));
    }
}
