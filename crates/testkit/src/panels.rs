//! Panel GPUI windows and frame stepping shared by the kits that go through a runtime ([`crate::VrKit`] / [`crate::NoVrKit`]).

use std::collections::HashMap;

use gpui::{AnyWindowHandle, AppContext, Entity, PlatformInput, Render, VisualTestContext};
use kernel::{Hand, Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec, PointerBridge};
use vr::VrBackend;

use crate::watch::Watch;
use crate::{Pose, Vec2, raster};

/// Panel width (m). The height is determined by the window aspect ratio.
pub(crate) const PANEL_WIDTH_M: f32 = 1.0;

pub(crate) struct Panels<V: 'static> {
    /// Indexed by PanelId. Panels with a different screen type (summoned ones) are `None`.
    pub(crate) views: Vec<Option<Entity<V>>>,
    pub(crate) windows: Vec<AnyWindowHandle>,
    bridges: HashMap<(PanelId, Hand), PointerBridge>,
    /// Web pages the screen asked to open ([`ui::PanelSpec::web`]). Not opened on the test platform.
    pub(crate) web_spawns: Vec<String>,
    /// If `Some`, shows each panel's rendering in a window (`NYAW_E2E_WATCH`).
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

    /// Adds view `view` (window `window`) as a panel at `pose`. Panel resolution = window size.
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
            .expect("failed to create panel");
        assert_eq!(id, self.windows.len(), "PanelId is the index in opening order");
        // Ordinary operation tests must not depend on time. Animations are verified in overlay / kernel.
        kernel.apply(id, kernel::PanelCommand::Animated(false));
        self.views.push(view);
        self.windows.push(window);
        id
    }

    /// Steps one frame with the same `ui::pump` as production and dispatches to GPUI, applies the view's panel operations and summons, and sends rendering to the runtime.
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

    /// Opens a panel summoned from a screen (placed with `pose_for`, as in production `ui::run`).
    fn add_spec<B: VrBackend>(
        &mut self,
        cx: &mut VisualTestContext,
        kernel: &mut Kernel<B>,
        spec: PanelSpec,
    ) {
        // Web pages (Tauri WebView) cannot be opened on the test platform, so only record what was requested.
        // Actually opening and operating them is done in crates/ui/tests/web_*.rs.
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
