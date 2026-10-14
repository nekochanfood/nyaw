//! A Playwright-style E2E test foundation. Tests only state which element to operate and how,
//! and never write profile names or coordinate math.
//!
//! Runtimes are equal "means", so a kit is provided equally for each runtime:
//!
//! - [`NoVrKit`]: novr (emulator). As in production novr, points and clicks with the mouse (right-hand controller),
//!   and the image the user sees is the emulator screen ([`NoVrKit::screenshot`]). No window is shown.
//! - [`VrKit`]: VR (mock). Operates via the laser, and rendering is sent to the VR panel.
//!
//! [`DesktopKit`] sends the mouse directly to the panel window without going through a runtime (for real runtimes with no controllers).
//!
//! Basic operations ([`Driver`]) must be doable natively on every runtime, so a scenario of
//! basic operations is written once and run on all runtimes with [`on_every_runtime!`]. VR-specific features
//! (hands, controller models, panel movement, etc.) are [`VrKit`]'s own methods, and tests that only look at UI logic
//! are verified with [`NoVrKit`] alone.
//!
//! ```ignore
//! fn click_counts<D: Driver<Testing>>(kit: &mut D) {
//!     kit.click("click-me");
//!     assert_eq!(kit.read(|h| h.clicks), 1);
//! }
//! on_every_runtime!(click_counts, |_, _| Testing::default());
//! ```
//!
//! Elements are specified with GPUI's `.debug_selector(|| "name".into())`.
//!
//! Setting the environment variable `NYAW_E2E_WATCH` (display time per frame, in ms) lets you watch the panels in a window during tests.
//!
//! For tests on real runtimes (Monado / SteamVR), use [`runtimes`] (install wizard, launch, and cleanup).

pub mod desktop;
pub mod novr;
mod panels;
pub mod raster;
pub mod runtimes;
pub mod vr_kit;
mod watch;

pub use desktop::DesktopKit;
pub use input::Action;
pub use novr::NoVrKit;
pub use vr::novr::Button;
pub use vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
pub use vr_kit::{DEFAULT_PROFILE, VrKit};

/// Basic operations that can be done natively on every runtime.
pub trait Driver<V> {
    /// Points at the center of an element (novr: emulator mouse; VR: laser).
    fn hover(&mut self, selector: &'static str) -> &mut Self;
    /// Clicks the center of an element (novr: left click = trigger; VR: select button).
    fn click(&mut self, selector: &'static str) -> &mut Self;
    /// Holds down at the center of an element (novr: left button; VR: select button). Moving with `hover` while held makes a drag.
    fn down(&mut self, selector: &'static str) -> &mut Self;
    /// Releases the button pressed by `down` at the place currently pointed at.
    fn up(&mut self) -> &mut Self;
    /// Grabs `from`, carries it to `to`, and releases (GPUI drag and drop).
    fn drag(&mut self, from: &'static str, to: &'static str) -> &mut Self {
        self.down(from).hover(to).up()
    }
    /// Scrolls vertically by `lines` lines at the center of an element (novr: arrow keys = right-hand stick; VR: stick).
    fn scroll(&mut self, selector: &'static str, lines: f32) -> &mut Self;
    /// Reads the view's state.
    fn read<R>(&mut self, f: impl FnOnce(&V) -> R) -> R;
    /// Color (0xRRGGBB) at the center of an element as visible to the user. novr: emulator screen; VR: panel texture.
    fn color_at(&mut self, selector: &'static str) -> u32;
    /// Web pages the screen asked to open (not opened on the test platform; only the name is kept).
    fn web_spawns(&self) -> &[String];
}

/// Creates tests that run a basic-operation scenario `scenario(&mut impl Driver<V>)` with the kit of every runtime.
/// The tests are named `scenario::novr` / `scenario::vr`.
#[macro_export]
macro_rules! on_every_runtime {
    ($scenario:ident, $build:expr) => {
        mod $scenario {
            // `$build` uses names from the caller's scope (such as the view type), so pull them in
            #[allow(unused_imports)]
            use super::*;

            #[gpui::test]
            fn novr(cx: &mut gpui::TestAppContext) {
                super::$scenario(&mut $crate::NoVrKit::new(cx, $build));
            }

            #[gpui::test]
            fn vr(cx: &mut gpui::TestAppContext) {
                super::$scenario(&mut $crate::VrKit::new(cx, $build));
            }
        }
    };
}
