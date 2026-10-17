//! Panel GPUI windows and frame stepping shared by the kits that go through a runtime ([`crate::VrKit`] / [`crate::NoVrKit`]).

use std::collections::HashMap;

use gpui::{AnyWindowHandle, AppContext, Entity, PlatformInput, Render, VisualTestContext};
use kernel::{Hand, Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec, PointerBridge};
use kernel::vr::VrBackend;

use crate::watch::Watch;
use crate::{Pose, Vec2, raster};

/// Panel width (m). The height is determined by the window aspect ratio.
pub(crate) const PANEL_WIDTH_M: f32 = 1.0;

pub(crate) struct Panels<V: 'static> {
    /// Indexed by PanelId. Panels with a different screen type (summoned ones) are `None`.
    pub(crate) views: Vec<Option<Entity<V>>>,
    pub(crate) windows: Vec<AnyWindowHandle>,
    bridges: HashMap<(PanelId, Hand), PointerBridge>,
    /// If `Some`, shows each panel's rendering in a window (`NYAW_E2E_WATCH`).
    pub(crate) watch: Option<Watch>,
}

impl<V: Render> Panels<V> {
    pub(crate) fn new(watch: Option<Watch>) -> Self {
        Self {
            views: Vec::new(),
            windows: Vec::new(),
            bridges: HashMap::new(),
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
        assert_eq!(
            id,
            self.windows.len(),
            "PanelId is the index in opening order"
        );
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
        let mut inputs: Vec<Vec<(kernel::vr::Hand, PlatformInput)>> =
            (0..self.windows.len()).map(|_| Vec::new()).collect();
        for (panel, hand, input) in ui::pump(kernel, &mut self.bridges) {
            inputs[panel].push((hand, input));
        }
        for (panel, (window, inputs)) in self.windows.iter().copied().zip(inputs).enumerate() {
            cx.cx
                .update_window(window, |_, window, cx| {
                    for (hand, input) in inputs {
                        ui::dispatch_input(panel, hand, input, window, cx);
                    }
                    ui::sync_panel(kernel, panel, cx);
                    // As in production, a view that changes every frame is redrawn every frame.
                    if cx
                        .try_global::<PanelControl>()
                        .and_then(|c| c.panels.get(&panel))
                        .is_some_and(|s| s.live)
                    {
                        window.refresh();
                    }
                })
                .unwrap();
        }
        cx.run_until_parked();
        // As in production, route the hovered element's tooltip to the hand pointing at it.
        let ids: Vec<_> = self
            .windows
            .iter()
            .map(|w| w.window_id())
            .enumerate()
            .collect();
        cx.cx
            .update(|cx| ui::sync_tooltips(kernel, &self.bridges, ids, cx));
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
        let id = self.windows.len();
        let pose = spec.pose_for(kernel, id);
        let parent = spec.parent();
        let (bar, owner) = (spec.nav_bar_spec(id), spec.nav_bar_of());
        let (root, cx) = cx
            .cx
            .add_window_view(|window, cx| PanelRoot::new(spec.build(id, window, cx)));
        let view = root.read_with(cx, |root, _| root.view().downcast::<V>().ok());
        let panel = self.add(cx, kernel, pose, view);
        if let Some(parent) = parent {
            kernel.apply(panel, kernel::PanelCommand::Parent(Some(parent)));
        }
        if let Some(owner) = owner {
            kernel.apply(panel, kernel::PanelCommand::NavBar(owner));
        }
        cx.update(|_, cx| ui::sync_panel(kernel, panel, cx));
        // As in production, the navigation bar opens right after its owner (PanelId = owner + 1).
        if let Some(bar) = bar {
            self.add_spec(cx, kernel, bar);
        }
    }
}
