//! E2E for the novr runtime. Goes through the emulator ([`Emulator`]) as production novr does: it holds all layers (emulator -> input ->
//! overlay -> UI -> GPUI) as a whole, panel rendering is sent to the emulator as a texture,
//! and input comes from the emulator's mouse (right-hand controller), buttons and sticks. The image the user sees is
//! the emulator screen ([`NoVrKit::screenshot`]). No window is shown (the screen can be viewed with `NYAW_E2E_WATCH`).

use gpui::{Bounds, Context, Pixels, Render, TestAppContext, VisualTestContext, Window};
use kernel::vr::novr::{Button, Emulator};
use kernel::{Kernel, PanelId};
use ui::Frame;

use crate::panels::Panels;
use crate::watch::Watch;
use crate::{Driver, Hand, Pose, Quat, Vec2, Vec3};

/// Position of the first panel.
const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.5);
/// Size of the emulator screen (same as the production window's initial size).
const SCREEN: (u32, u32) = (1280, 720);

pub struct NoVrKit<'a, V: 'static> {
    cx: &'a mut VisualTestContext,
    panels: Panels<V>,
    /// May be inspected directly (e.g. to check panel state). The emulator is `kernel.subsystems.backend`.
    pub kernel: Kernel<Emulator>,
    pub panel: PanelId,
    /// Shows the emulator screen (`NYAW_E2E_WATCH`).
    watch: Option<Watch>,
}

impl<'a, V: Render> NoVrKit<'a, V> {
    /// Starts the `build` view as the first panel, placed straight ahead. Panel resolution = window size.
    pub fn new(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self {
        let (view, cx) = cx.add_window_view(build);
        cx.run_until_parked();
        // Turn off hand-jitter smoothing, since it would make pointing miss at the instant of aiming (same as VrKit).
        let config = kernel::config::Config {
            pointer_smoothing: Some(0.0),
            // Operation scenarios are written for the mouse (the default touch mode is checked in pointer_mode.rs).
            pointer_mode: Some(kernel::PointerMode::Mouse),
            ..Default::default()
        };
        let mut kernel = Kernel::with_backend(Emulator::headless(SCREEN.0, SCREEN.1), config);
        let mut panels = Panels::new(None);
        let pose = Pose::new(PANEL_POS, Quat::IDENTITY);
        let panel = panels.add(cx, &mut kernel, pose, Some(view));
        let mut kit = Self {
            cx,
            panels,
            kernel,
            panel,
            watch: Watch::from_env(),
        };
        kit.step(); // send the first frame up front
        kit
    }

    /// Steps one frame (emulator input -> `ui::pump` -> GPUI -> rendering back to the emulator).
    pub fn step(&mut self) -> &mut Self {
        self.panels.step(self.cx, &mut self.kernel);
        if self.watch.is_some() {
            let shot = self.screenshot();
            let w = self.watch.as_mut().unwrap();
            w.show(0, &shot);
            w.wait();
        }
        self
    }

    /// Selects the panel used for subsequent operations and reads.
    pub fn select_panel(&mut self, panel: PanelId) -> &mut Self {
        assert!(panel < self.panels.windows.len(), "no panel {panel}");
        self.panel = panel;
        self
    }

    fn visual(&self) -> VisualTestContext {
        VisualTestContext::from_window(self.panels.windows[self.panel], &self.cx.cx)
    }

    /// The bounds on the panel (logical pixels) of the element tagged with `debug_selector`.
    pub fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.visual()
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("element {selector:?} is not rendered"))
    }

    /// The point on screen where the center of an element appears.
    pub fn on_screen(&mut self, selector: &'static str) -> Vec2 {
        let c = self.bounds(selector).center();
        let world = self
            .kernel
            .panel(self.panel)
            .pixel_to_world(Vec2::new(c.x.into(), c.y.into()));
        self.kernel
            .subsystems
            .backend
            .project(world)
            .unwrap_or_else(|| panic!("element {selector:?} is behind the eye"))
    }

    /// The emulator screen (the image visible to the user).
    pub fn screenshot(&mut self) -> Frame {
        let (width, height, rgba) = self.kernel.subsystems.backend.screenshot();
        Frame {
            width,
            height,
            rgba,
        }
    }

    /// Color (0xRRGGBB) of a point on screen.
    pub fn color_on_screen(&mut self, at: Vec2) -> u32 {
        let shot = self.screenshot();
        let i = ((at.y as u32 * shot.width + at.x as u32) * 4) as usize;
        u32::from_be_bytes([0, shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]])
    }

    /// Moves the mouse to a point on screen (`None` moves it off screen) and steps one frame.
    pub fn move_mouse(&mut self, at: Option<Vec2>) -> &mut Self {
        self.kernel.subsystems.backend.point(at);
        self.step()
    }

    /// Presses a mouse button (left = trigger, right = grip) and steps one frame.
    pub fn press(&mut self, button: Button) -> &mut Self {
        self.kernel.subsystems.backend.set_button(button, true);
        self.step()
    }

    pub fn release(&mut self, button: Button) -> &mut Self {
        self.kernel.subsystems.backend.set_button(button, false);
        self.step()
    }

    /// Tilts a stick (left hand = WASD, right hand = arrow keys) and steps one frame. Use `Vec2::ZERO` to release.
    pub fn stick(&mut self, hand: Hand, value: Vec2) -> &mut Self {
        self.kernel.subsystems.backend.set_stick(hand, value);
        self.step()
    }

    /// Types keys into the focused element (space-separated, like `"h i backspace"`). The emulator's keyboard is
    /// a stick, so it does not reach the panel. To check UI logic, types directly into the GPUI window.
    pub fn type_keys(&mut self, keys: &str) -> &mut Self {
        self.visual().simulate_keystrokes(keys);
        self.step()
    }
}

impl<V: Render> Driver<V> for NoVrKit<'_, V> {
    /// Moves the mouse to the center of an element.
    fn hover(&mut self, selector: &'static str) -> &mut Self {
        let at = self.on_screen(selector);
        self.move_mouse(Some(at))
    }

    /// Left-clicks (trigger) the center of an element.
    fn click(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector)
            .press(Button::Trigger)
            .release(Button::Trigger)
    }

    /// Holds the left button (trigger) down at the center of an element.
    fn down(&mut self, selector: &'static str) -> &mut Self {
        self.hover(selector).press(Button::Trigger)
    }

    fn up(&mut self) -> &mut Self {
        self.release(Button::Trigger)
    }

    /// Points at the center of an element and tilts the arrow keys (right-hand stick) vertically for one frame only.
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self {
        self.hover(selector)
            .stick(Hand::Right, Vec2::new(0.0, lines))
            .stick(Hand::Right, Vec2::ZERO)
    }

    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R {
        self.panels.views[self.panel]
            .as_ref()
            .expect("screen type mismatch")
            .read_with(self.cx, |v, _| f(v))
    }

    /// The color shown on the emulator screen.
    fn color_at(&mut self, selector: &'static str) -> u32 {
        let at = self.on_screen(selector);
        self.color_on_screen(at)
    }
}
