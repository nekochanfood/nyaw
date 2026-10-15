//! E2E for VR mode. Holds all layers (mock VR -> input -> overlay -> UI -> GPUI) as a whole,
//! so VR-specific features such as the laser, controller models, hands, panel operations and what is rendered in VR can also be verified.
//! The UI's rendering result is also sent to the VR mock every frame (rendering uses the CPU rasterizer in [`crate::raster`]).

use gpui::{Bounds, Context, Pixels, Render, TestAppContext, VisualTestContext, Window};
use kernel::{Kernel, PanelId};
use vr::{
    RawController,
    mock::{MockBackend, MockTexture},
};

use crate::panels::Panels;
use crate::watch::Watch;
use crate::{Action, Driver, Hand, Pose, Quat, Vec2, Vec3};

/// The virtual controller model used when none is specified.
pub const DEFAULT_PROFILE: &str = "valve/index_controller";
const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// Number of ticks for a grab-move to smoothly catch up (with some margin).
const DRAG_SETTLE: usize = 60;

/// A virtual controller. Converts the actions being pressed into raw input appropriate for the model and feeds them to the VR mock.
struct VirtualController {
    hand: Hand,
    profile: String,
    aim: Pose,
    held: Vec<Action>,
    /// How far the stick (trackpad) is tilted.
    stick: Vec2,
}

/// Resting position of the hands (slightly below the head, left and right). The laser is aimed from here at the target.
fn rest_position(hand: Hand) -> Vec3 {
    match hand {
        Hand::Left => Vec3::new(-0.2, -0.3, 0.0),
        Hand::Right => Vec3::new(0.2, -0.3, 0.0),
    }
}

/// Operates through the VR runtime (mock).
pub struct VrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    panels: Panels<V>,
    /// May be inspected directly (e.g. to check the panel pose sent to the VR side).
    pub kernel: Kernel<MockBackend>,
    panel: PanelId,
    controllers: Vec<VirtualController>,
    active: Hand,
}

impl<'a, V: Render> VrKit<'a, V> {
    /// Starts the `build` view as the first panel, placed 1m straight ahead. Panel resolution = window size.
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        // Turn off hand-jitter smoothing, since it would make pointing miss at the instant of aiming (the smoothing itself is checked in overlay unit tests).
        let config = kernel::config::Config {
            pointer_smoothing: Some(0.0),
            // Operation scenarios are written for the mouse (the default touch mode is checked in pointer_mode.rs / web_pointer.rs).
            pointer_mode: Some(kernel::PointerMode::Mouse),
            ..Default::default()
        };
        let mut kernel = Kernel::with_backend(MockBackend::default(), config);
        let mut panels = Panels::new(Watch::from_env());
        let pose = Pose::new(PANEL_POS, Quat::IDENTITY);
        let panel = panels.add(cx, &mut kernel, pose, Some(view));
        let mut kit = Self {
            cx,
            panels,
            kernel,
            panel,
            controllers: Vec::new(),
            active: Hand::Right,
        };
        kit.step(); // send the first frame to VR up front
        kit
    }

    /// Adds a panel with the same screen type. Pass the return value to [`Self::panel`] to select the target.
    pub fn add_panel(
        &mut self,
        pose: Pose,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> PanelId {
        let (view, cx) = self.cx.cx.add_window_view(build);
        let id = self.panels.add(cx, &mut self.kernel, pose, Some(view));
        self.step();
        id
    }

    /// Selects the panel used for subsequent operations and reads.
    pub fn panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.panels.windows.len(), "no panel {panel}");
        self.panel = panel;
        self
    }

    /// Switches the hand used for subsequent operations (right hand by default).
    pub fn hand(&mut self, hand: Hand) -> &mut Self {
        self.active = hand;
        self
    }

    /// Switches the controller model of the current hand.
    pub fn profile(&mut self, id: &str) -> &mut Self {
        self.ctrl().profile = id.to_owned();
        self
    }

    /// The bounds in the window of the element tagged with `debug_selector`.
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        VisualTestContext::from_window(self.panels.windows[self.panel], &self.cx.cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("element {selector:?} is not rendered"))
    }

    /// Aims the laser at a pixel on the panel and steps one frame.
    pub fn point_at(&mut self, px: Vec2) -> &mut Self {
        let target = self.kernel.panel(self.panel).pixel_to_world(px);
        let c = self.ctrl();
        let dir = (target - c.aim.position).normalize();
        c.aim.orientation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
        self.step()
    }

    /// Moves the current hand's controller to `aim` and steps one frame.
    pub fn move_hand(&mut self, aim: Pose) -> &mut Self {
        self.ctrl().aim = aim;
        self.step()
    }

    pub fn press(&mut self, action: Action) -> &mut Self {
        let c = self.ctrl();
        if !c.held.contains(&action) {
            c.held.push(action);
        }
        self.step()
    }

    pub fn release(&mut self, action: Action) -> &mut Self {
        self.ctrl().held.retain(|&a| a != action);
        self.step()
    }

    /// Grabs the panel center, translates the hand until the panel center reaches `to`, then releases.
    pub fn drag_panel(&mut self, to: Vec3) -> &mut Self {
        let p = self.kernel.panel(self.panel);
        let center = Vec2::new(p.resolution.0 as f32, p.resolution.1 as f32) / 2.0;
        let delta = to - p.pose.position;
        self.point_at(center).press(Action::Grab);
        self.ctrl().aim.position += delta;
        // Grab-move catches up smoothly, so keep holding until it catches up.
        for _ in 0..DRAG_SETTLE {
            self.step();
        }
        self.release(Action::Grab)
    }

    pub fn panel_pose(&self) -> Pose {
        self.kernel.panel(self.panel).pose
    }

    /// Feeds the virtual controller state to the VR mock, steps one frame with the same `ui::pump` as production, and dispatches to GPUI.
    pub fn step(&mut self) -> &mut Self {
        self.kernel.subsystems.backend.frame.controllers =
            self.controllers.iter().map(raw).collect();
        self.panels.step(self.cx, &mut self.kernel);
        self
    }

    /// The texture most recently sent to the VR panel.
    pub fn texture(&self) -> &MockTexture {
        self.kernel.subsystems.backend.overlays
            [self.kernel.panel(self.panel).overlay_id().0 as usize]
            .texture
            .as_ref()
            .expect("VrKit::new sends the first frame")
    }

    /// Color (0xRRGGBB) of logical pixel `px` on the panel as shown on the VR panel.
    pub fn vr_color_at_px(&self, px: Vec2) -> u32 {
        let tex = self.texture();
        let (rw, rh) = self.kernel.panel(self.panel).resolution;
        let x = ((px.x / rw as f32 * tex.width as f32) as u32).min(tex.width - 1);
        let y = ((px.y / rh as f32 * tex.height as f32) as u32).min(tex.height - 1);
        let [r, g, b, _] = tex.pixel(x, y);
        u32::from_be_bytes([0, r, g, b])
    }

    fn ctrl(&mut self) -> &mut VirtualController {
        let hand = self.active;
        let i = match self.controllers.iter().position(|c| c.hand == hand) {
            Some(i) => i,
            None => {
                self.controllers.push(VirtualController {
                    hand,
                    profile: DEFAULT_PROFILE.to_owned(),
                    aim: Pose::new(rest_position(hand), Quat::IDENTITY),
                    held: Vec::new(),
                    stick: Vec2::ZERO,
                });
                self.controllers.len() - 1
            }
        };
        &mut self.controllers[i]
    }
}

fn raw(c: &VirtualController) -> RawController {
    let mut inputs = c
        .held
        .iter()
        .map(|&a| {
            let name = input::source(&c.profile, a)
                .unwrap_or_else(|| panic!("{} has no input corresponding to {a:?}", c.profile));
            (name.to_owned(), 1.0)
        })
        .collect::<Vec<_>>();
    if c.stick != Vec2::ZERO {
        let (x, y) = input::scroll_source(&c.profile)
            .unwrap_or_else(|| panic!("{} has no input corresponding to scrolling", c.profile));
        inputs.extend([(x.to_owned(), c.stick.x), (y.to_owned(), c.stick.y)]);
    }
    RawController {
        hand: c.hand,
        profile: c.profile.clone(),
        aim: c.aim,
        inputs,
    }
}

impl<V: Render> Driver<V> for VrKit<'_, V> {
    fn web_spawns(&self) -> &[String] {
        &self.panels.web_spawns
    }

    /// Aims the laser at the center of an element.
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let c = self.bounds(selector).center();
        self.point_at(Vec2::new(c.x.into(), c.y.into()))
    }

    /// Aims at the center of an element and presses then releases the select button.
    fn click(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector)
            .press(Action::Select)
            .release(Action::Select)
    }

    /// Aims at the center of an element and holds the select button down.
    fn down(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector).press(Action::Select)
    }

    fn up(&mut self) -> &mut Self {
        self.release(Action::Select)
    }

    /// Aims at the center of an element and tilts the stick vertically for one frame only.
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        self.hover(selector);
        self.ctrl().stick = Vec2::new(0.0, lines);
        self.step();
        self.ctrl().stick = Vec2::ZERO;
        self.step()
    }

    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.panels.views[self.panel]
            .as_ref()
            .expect("screen type mismatch")
            .read_with(self.cx, |v, _| f(v))
    }

    /// The color shown on the VR panel.
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let c = self.bounds(selector).center();
        self.vr_color_at_px(Vec2::new(c.x.into(), c.y.into()))
    }
}
