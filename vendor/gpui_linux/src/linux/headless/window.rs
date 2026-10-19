//! Windows for the headless platform client.
//!
//! A headless window has no compositor surface and no GPU: layout, text
//! shaping, and entity plumbing run normally, `draw` discards the scene, and
//! the sprite atlas hands out tiles without uploading pixels (the same
//! `HeadlessAtlas` GPUI's `TestWindow` uses). This lets command-line tools
//! drive real `Window`-based code paths without a display server.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Bounds, Capslock, DispatchEventResult, GpuSpecs, HeadlessAtlas, Modifiers, Pixels,
    PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point,
    PromptButton, PromptLevel, RequestFrameOptions, Scene, Size, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams, WindowVisibility,
};

thread_local! {
    /// NYAW: 次の `resize` で使う描画の拡大率（[`with_render_scale`]）。
    static RENDER_SCALE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// NYAW: `f`（`Window::resize` を呼ぶ）の間の resize で、headless のウィンドウの拡大率を `scale` にする。
/// 論理サイズはそのままで、描く解像度だけが変わる。headless は resize を gpui に知らせないので、
/// 呼んだ後に `Window::bounds_changed` で拡大率と論理サイズを gpui に取り込ませる。
pub fn with_render_scale<R>(scale: f32, f: impl FnOnce() -> R) -> R {
    RENDER_SCALE.with(|s| s.set(Some(scale)));
    let r = f();
    RENDER_SCALE.with(|s| s.set(None));
    r
}

struct HeadlessWindowState {
    material_window: u64,
    /// NYAW: 描画の拡大率（[`with_render_scale`]）。
    scale: f32,
    bounds: Bounds<Pixels>,
    /// Held while this window exists, so the platform can tell that headless windows are open.
    _lease: Rc<()>,
    input_handler: Option<PlatformInputHandler>,
    title: Option<String>,
    is_fullscreen: bool,
    /// NYAW: 描いて読み戻す先。作れなければ（GPU が無い）従来どおり何も描かない。
    #[cfg(feature = "test-support")]
    renderer: Option<gpui_wgpu::WgpuHeadlessRenderer>,
    /// NYAW: 透過の窓は透明で塗る（gpui が開いた直後に `set_background_appearance` で伝える）。
    background: WindowBackgroundAppearance,
}

pub(crate) struct HeadlessWindow(Rc<RefCell<HeadlessWindowState>>);

impl Drop for HeadlessWindow {
    fn drop(&mut self) {
        paint::remove(self.0.borrow().material_window);
    }
}

impl raw_window_handle::HasWindowHandle for HeadlessWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Headless windows are not backed by a native window.
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl raw_window_handle::HasDisplayHandle for HeadlessWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl HeadlessWindow {
    pub(crate) fn new(material_window: u64, params: WindowParams, lease: Rc<()>) -> Self {
        Self(Rc::new(RefCell::new(HeadlessWindowState {
            material_window,
            bounds: params.bounds,
            _lease: lease,
            input_handler: None,
            title: None,
            is_fullscreen: false,
            scale: 1.0,
            #[cfg(feature = "test-support")]
            renderer: gpui_wgpu::WgpuHeadlessRenderer::new()
                .inspect_err(|e| log::warn!("headless renderer unavailable: {e:#}"))
                .ok(),
            background: WindowBackgroundAppearance::Opaque,
        })))
    }
}

impl PlatformWindow for HeadlessWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        false
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let mut state = self.0.borrow_mut();
        state.bounds.size = size;
        if let Some(scale) = RENDER_SCALE.with(|s| s.get()) {
            state.scale = scale;
        }
    }

    fn scale_factor(&self) -> f32 {
        self.0.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        WindowAppearance::Dark
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }

    fn capslock(&self) -> Capslock {
        Capslock::default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        // Fall back to GPUI's rendered prompts.
        None
    }

    fn activate(&self) {}

    fn is_active(&self) -> bool {
        false
    }

    fn visibility(&self) -> WindowVisibility {
        // There is no display server: `draw` discards every scene.
        WindowVisibility::Hidden
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.0.borrow().background
    }

    fn set_title(&mut self, title: &str) {
        self.0.borrow_mut().title = Some(title.to_owned());
    }

    fn get_title(&self) -> String {
        self.0.borrow().title.clone().unwrap_or_default()
    }

    fn set_background_appearance(&self, background: WindowBackgroundAppearance) {
        self.0.borrow_mut().background = background;
    }

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {
        let mut state = self.0.borrow_mut();
        state.is_fullscreen = !state.is_fullscreen;
    }

    fn is_fullscreen(&self) -> bool {
        self.0.borrow().is_fullscreen
    }

    // No compositor drives a frame loop, so frame and status callbacks are
    // dropped: anything that awaits a frame will never resolve headlessly.
    fn on_request_frame(&self, _callback: Box<dyn FnMut(RequestFrameOptions)>) {}

    fn on_input(&self, _callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {}

    fn on_active_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(WindowVisibility)>) {}

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, _callback: Box<dyn FnMut(Size<Pixels>, f32)>) {}

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_display_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_close(&self, _callback: Box<dyn FnOnce()>) {}

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn draw(&self, _scene: &Scene) {}

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        // NYAW: 描くときは、レンダラが字形などを置くアトラスを使わせる。
        #[cfg(feature = "test-support")]
        if let Some(renderer) = &self.0.borrow().renderer {
            use gpui::PlatformHeadlessRenderer as _;
            return renderer.sprite_atlas();
        }
        Arc::new(HeadlessAtlas::default())
    }

    /// NYAW: 窓を出さずに描いて読み戻す（GPU の完了を待たず、前に積んだ絵を返す。`gpui_wgpu::collect_readback`）。
    #[cfg(feature = "test-support")]
    fn render_to_image(&self, scene: &Scene) -> anyhow::Result<image::RgbaImage> {
        let mut state = self.0.borrow_mut();
        let size = state.bounds.size.to_device_pixels(state.scale);
        let transparent = state.background != WindowBackgroundAppearance::Opaque;
        let material_window = state.material_window;
        let renderer = state
            .renderer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("headless renderer unavailable"))?;
        paint::with_window(material_window, || {
            renderer.render_to_image_deferred(scene, size, transparent)
        })
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }
}
