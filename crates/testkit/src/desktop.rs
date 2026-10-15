//! A kit that drives the screen without controllers. It sends the mouse straight to the panel's GPUI window (bypassing VR, input, and the overlay layer),
//! and sends rendering to the overlay of the runtime passed in. For checking, on a real runtime with no controllers, whether the result of operating the screen reaches VR
//! (`tests/runtimes.rs`). Panel operations from the view (attach, lock) are applied to the kernel as in production.

use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Modifiers, MouseButton, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, Window,
};
use kernel::{Kernel, PanelId};
use ui::{PanelControl, PanelRoot, PanelSpec};
use vr::VrBackend;

use crate::watch::Watch;
use crate::{Driver, Pose, Quat, Vec2, Vec3, raster};

/// Where the panel is placed (1m straight ahead at standing eye height, so it is visible in a real runtime).
const PANEL_POSE: Pose = Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY);

pub struct DesktopKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    views: Vec<Option<Entity<V>>>,
    windows: Vec<AnyWindowHandle>,
    /// Where rendering is sent.
    pub kernel: Kernel,
    pub panel: PanelId,
    watch: Option<Watch>,
    /// Web pages the screen asked to open ([`ui::PanelSpec::web`]). Not opened on the test platform.
    pub web_spawns: Vec<String>,
    /// Mouse position, and whether the left button is held down.
    mouse: Vec<(Point<Pixels>, bool)>,
}

impl<'a, V: Render> DesktopKit<'a, V> {
    /// Sends rendering to the `vr` overlay (1m ahead) every frame. Input stays the mouse.
    pub fn with_backend(
        cx: &'a mut TestAppContext,
        vr: Box<dyn VrBackend>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let mut kernel = Kernel::with_backend(vr, Default::default());
        let panel = kernel
            .add_panel("nyaw.testkit", PANEL_POSE, Vec2::ONE, res)
            .expect("failed to create panel");
        let mut kit = Self {
            cx,
            views: vec![Some(view)],
            windows: vec![window],
            kernel,
            panel,
            watch: Watch::from_env(),
            web_spawns: Vec::new(),
            mouse: vec![(Point::default(), false)],
        };
        kit.step();
        kit
    }

    /// One iteration of the production frame loop: apply panel operations from the view and run until things settle.
    pub fn step(&mut self) -> &mut Self {
        self.cx.run_until_parked();
        for (panel, window) in self.windows.iter().copied().enumerate() {
            self.cx
                .cx
                .update_window(window, |_, _, cx| {
                    ui::sync_panel(&mut self.kernel, panel, cx)
                })
                .unwrap();
        }
        let spawns = self
            .cx
            .cx
            .update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
        for spec in spawns {
            self.add_spec(spec);
        }
        self.cx.run_until_parked();
        for (panel, window) in self.windows.iter().copied().enumerate() {
            let frame = self
                .cx
                .cx
                .update_window(window, |_, window, cx| raster::capture(window, cx))
                .unwrap();
            if let Some(w) = &mut self.watch {
                w.show(panel, &frame);
            }
            self.kernel
                .submit(panel, frame.width, frame.height, &frame.rgba);
        }
        if let Some(w) = &self.watch {
            w.wait();
        }
        self
    }

    fn add_spec(&mut self, spec: PanelSpec) {
        // Web pages (Tauri WebView) cannot be opened on the test platform, so only record what was requested.
        if let Some(page) = spec.web_page() {
            self.web_spawns.push(page.to_owned());
            return;
        }
        let id = self.windows.len();
        let pose = spec.pose_for(&self.kernel, id);
        let parent = spec.parent();
        let (root, cx) = self
            .cx
            .cx
            .add_window_view(|window, cx| PanelRoot::new(spec.build(id, window, cx)));
        let window = cx.update(|window, _| window.window_handle());
        let vp = cx.update(|window, _| window.viewport_size());
        let res = (f32::from(vp.width) as u32, f32::from(vp.height) as u32);
        let view = root.read_with(cx, |root, _| root.view().downcast::<V>().ok());
        let panel = self
            .kernel
            .add_panel(&format!("nyaw.testkit.{id}"), pose, Vec2::ONE, res)
            .unwrap();
        assert_eq!(panel, id);
        if let Some(parent) = parent {
            self.kernel
                .apply(panel, kernel::PanelCommand::Parent(Some(parent)));
        }
        self.views.push(view);
        self.windows.push(window);
        self.mouse.push((Point::default(), false));
        cx.update(|_, cx| ui::sync_panel(&mut self.kernel, panel, cx));
    }

    /// Selects the panel used for subsequent operations and reads.
    pub fn select_panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.windows.len(), "no panel {panel}");
        self.panel = panel;
        self
    }

    fn visual(&self) -> VisualTestContext {
        VisualTestContext::from_window(self.windows[self.panel], &self.cx.cx)
    }

    /// The bounds in the window of the element tagged with `debug_selector`.
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.visual()
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("element {selector:?} is not rendered"))
    }

    /// Moves the mouse to a point in the window.
    pub fn move_to(&mut self, pos: Point<Pixels>) -> &mut Self {
        self.mouse[self.panel].0 = pos;
        let button = self.mouse[self.panel].1.then_some(MouseButton::Left);
        self.visual()
            .simulate_mouse_move(pos, button, Modifiers::none());
        self.step()
    }
}

impl<V: Render> Driver<V> for DesktopKit<'_, V> {
    fn web_spawns(&self) -> &[String] {
        &self.web_spawns
    }

    /// Moves the mouse to the center of an element.
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c)
    }

    /// Left-clicks the center of an element.
    fn click(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual().simulate_click(c, Modifiers::none());
        self.step()
    }

    /// Holds the left button down at the center of an element.
    fn down(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual()
            .simulate_mouse_down(c, MouseButton::Left, Modifiers::none());
        self.mouse[self.panel].1 = true;
        self.step()
    }

    fn up(&mut self) -> &mut Self {
        self.visual().simulate_mouse_up(
            self.mouse[self.panel].0,
            MouseButton::Left,
            Modifiers::none(),
        );
        self.mouse[self.panel].1 = false;
        self.step()
    }

    /// Turns the wheel at the center of an element.
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        let c = self.bounds(selector).center();
        self.move_to(c);
        self.visual().simulate_event(ScrollWheelEvent {
            position: c,
            delta: ScrollDelta::Lines(Point::new(0.0, lines)),
            ..Default::default()
        });
        self.step()
    }

    /// Reads the view's state.
    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.views[self.panel]
            .as_ref()
            .expect("screen type mismatch")
            .read_with(self.cx, |v, _| f(v))
    }

    /// Color (0xRRGGBB) at the center of an element drawn on the screen.
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        let (frame, scale) = self
            .visual()
            .update(|window, cx| (raster::capture(window, cx), window.scale_factor()));
        let (x, y) = (
            (f32::from(c.x) * scale) as u32,
            (f32::from(c.y) * scale) as u32,
        );
        let i = ((y * frame.width + x) * 4) as usize;
        u32::from_be_bytes([0, frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]])
    }
}
