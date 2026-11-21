//! Linux の描画ホスト: パネルのウィンドウを表示サーバーに作らず、wgpu のオフスクリーン描画先（vendor/gpui_wgpu の
//! `WgpuHeadlessRenderer`）に描く。Linux の GPUI は Wayland / X11 のウィンドウで `render_to_image` を持たず、非表示の
//! ウィンドウも作れないため、ヘッドレスの Linux プラットフォームを包み、ウィンドウの生成だけを差し替える。
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    Action, AnyWindowHandle, BackgroundExecutor, Bounds, Capslock, ClipboardItem, CursorStyle,
    DevicePixels, DispatchEventResult, ForegroundExecutor, FrameRequestSource, GpuSpecs, Keymap,
    Menu, MenuItem, Modifiers, PathPromptOptions, Pixels, Platform, PlatformAtlas, PlatformDisplay,
    PlatformHeadlessRenderer, PlatformInput, PlatformInputHandler, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point, PromptButton, PromptLevel,
    RequestFrameOptions, Scene, Size, Task, ThermalState, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams, WindowVisibility,
    px,
};
use gpui_wgpu::{CosmicTextSystem, WgpuHeadlessRenderer};

thread_local! {
    /// 次回の `resize` で適用する描画スケール（[`with_render_scale`]）。
    static RENDER_SCALE: Cell<Option<f32>> = const { Cell::new(None) };
    /// 次の `render_to_image` で描画せず、発行済みの読み戻しを回収する（[`collect_readback`]）。
    static COLLECT: Cell<bool> = const { Cell::new(false) };
}

/// `f`（`Window::resize` を呼ぶ）の間の resize で、ウィンドウの描画スケールを `scale` にする（Windows の
/// `gpui_windows::with_render_scale` と同じ）。論理サイズは変えず、描画解像度だけが変わる。反映は次のタスク実行時。
pub fn with_render_scale<R>(scale: f32, f: impl FnOnce() -> R) -> R {
    RENDER_SCALE.set(Some(scale));
    let r = f();
    RENDER_SCALE.set(None);
    r
}

/// `f`（`Window::render_to_image` を呼ぶ）の間、再描画せず前回発行した読み戻しを回収する（Windows の
/// `gpui_windows::collect_readback` と同じ）。発行済みの読み戻しが無ければエラー。
pub fn collect_readback<R>(f: impl FnOnce() -> R) -> R {
    COLLECT.set(true);
    let r = f();
    COLLECT.set(false);
    r
}

/// ヘッドレスの Linux プラットフォーム（イベントループ・実行器）に、文字描画とオフスクリーンのウィンドウを組み合わせる。
pub struct OffscreenPlatform {
    inner: Rc<dyn Platform>,
    /// Zed のワークスペースは gpui_linux を表示サーバー（wayland / x11）なしで取り込むため、そのヘッドレスの文字描画は
    /// 何も描かない（`NoopTextSystem`）。表示サーバーありの gpui_linux と同じ文字描画（cosmic-text とシステムのフォント）を使う。
    text_system: Arc<CosmicTextSystem>,
}

impl OffscreenPlatform {
    pub fn new() -> Self {
        // 表示サーバーに接続しない（SteamVR から起動されたときなど、表示サーバーが無くても動く）。
        Self {
            inner: gpui_platform::current_platform(true),
            text_system: Arc::new(CosmicTextSystem::new(&system_ui_font())),
        }
    }
}

/// GPUI のシステム UI 書体（`.SystemUIFont`。書体を指定しない文字）に使う、デスクトップの sans-serif（fontconfig）。
/// Windows の Segoe UI に相当する。gpui_linux は Zed が同梱する IBM Plex Sans を使うが、nyaw は同梱しない。
fn system_ui_font() -> String {
    std::process::Command::new("fc-match")
        .args(["--format=%{family[0]}", "sans-serif"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|family| !family.is_empty())
        // ponytail: fontconfig が無い環境では、よく入っている書体を名指しする。
        .unwrap_or_else(|| "DejaVu Sans".into())
}

impl Default for OffscreenPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for OffscreenPlatform {
    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        params: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        Ok(Box::new(OffscreenWindow::new(
            params,
            self.inner.foreground_executor(),
            self.inner.background_executor(),
            self.inner.window_appearance(),
        )?))
    }

    /// ヘッドレスのプラットフォームは URL を開かない。デスクトップの既定のアプリ（xdg-open）に渡す。
    fn open_url(&self, url: &str) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
            logger::warn!("cannot open URL with xdg-open", url = url, error = e);
        }
    }

    fn background_executor(&self) -> BackgroundExecutor {
        self.inner.background_executor()
    }
    fn foreground_executor(&self) -> ForegroundExecutor {
        self.inner.foreground_executor()
    }
    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }
    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        self.inner.run(on_finish_launching)
    }
    fn quit(&self) {
        self.inner.quit()
    }
    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) {
        self.inner.restart(binary_path, arguments)
    }
    fn activate(&self, ignoring_other_apps: bool) {
        self.inner.activate(ignoring_other_apps)
    }
    fn hide(&self) {
        self.inner.hide()
    }
    fn hide_other_apps(&self) {
        self.inner.hide_other_apps()
    }
    fn unhide_other_apps(&self) {
        self.inner.unhide_other_apps()
    }
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.inner.displays()
    }
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.inner.primary_display()
    }
    fn on_displays_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_displays_changed(callback)
    }
    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.inner.active_window()
    }
    fn window_appearance(&self) -> WindowAppearance {
        self.inner.window_appearance()
    }
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.inner.on_open_urls(callback)
    }
    fn register_url_scheme(&self, url: &str) -> Task<anyhow::Result<()>> {
        self.inner.register_url_scheme(url)
    }
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> futures::channel::oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>> {
        self.inner.prompt_for_paths(options)
    }
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> futures::channel::oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
        self.inner.prompt_for_new_path(directory, suggested_name)
    }
    fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.inner.can_select_mixed_files_and_dirs()
    }
    fn reveal_path(&self, path: &Path) {
        self.inner.reveal_path(path)
    }
    fn open_with_system(&self, path: &Path) {
        self.inner.open_with_system(path)
    }
    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.on_quit(callback)
    }
    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_reopen(callback)
    }
    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_sleep(callback)
    }
    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_wake(callback)
    }
    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        self.inner.set_menus(menus, keymap)
    }
    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        self.inner.set_dock_menu(menu, keymap)
    }
    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.inner.on_app_menu_action(callback)
    }
    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_will_open_app_menu(callback)
    }
    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.inner.on_validate_app_menu_command(callback)
    }
    fn thermal_state(&self) -> ThermalState {
        self.inner.thermal_state()
    }
    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_thermal_state_change(callback)
    }
    fn prevent_idle_sleep(&self, reason: &str) -> Task<anyhow::Result<gpui::ActivityGuard>> {
        self.inner.prevent_idle_sleep(reason)
    }
    fn compositor_name(&self) -> &'static str {
        "offscreen"
    }
    fn app_path(&self) -> anyhow::Result<PathBuf> {
        self.inner.app_path()
    }
    fn path_for_auxiliary_executable(&self, name: &str) -> anyhow::Result<PathBuf> {
        self.inner.path_for_auxiliary_executable(name)
    }
    fn set_cursor_style(&self, style: CursorStyle) {
        self.inner.set_cursor_style(style)
    }
    fn hide_cursor_until_mouse_moves(&self) {
        self.inner.hide_cursor_until_mouse_moves()
    }
    fn is_cursor_visible(&self) -> bool {
        self.inner.is_cursor_visible()
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        self.inner.should_auto_hide_scrollbars()
    }
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.inner.read_from_clipboard()
    }
    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.inner.write_to_clipboard(item)
    }
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.inner.read_from_primary()
    }
    fn write_to_primary(&self, item: ClipboardItem) {
        self.inner.write_to_primary(item)
    }
    fn write_credentials(
        &self,
        url: &str,
        username: &str,
        password: &[u8],
    ) -> Task<anyhow::Result<()>> {
        self.inner.write_credentials(url, username, password)
    }
    fn read_credentials(&self, url: &str) -> Task<anyhow::Result<Option<(String, Vec<u8>)>>> {
        self.inner.read_credentials(url)
    }
    fn delete_credentials(&self, url: &str) -> Task<anyhow::Result<()>> {
        self.inner.delete_credentials(url)
    }
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.inner.keyboard_layout()
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.inner.keyboard_mapper()
    }
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_keyboard_layout_change(callback)
    }
}

/// ポインタはオフスクリーンのウィンドウの上には無い（nyaw は入力を `Window::dispatch_event` で直接配送する）。
/// vendor/gpui が `MouseExited` で置く位置と同じ。
const NO_POINTER: Point<Pixels> = Point {
    x: px(-1.0e6),
    y: px(-1.0e6),
};

type ResizeCallback = Rc<RefCell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>>;

/// GPUI 自身のフレーム（`on_request_frame`）。GPUI はここで、汚れたウィンドウの描画と、次のフレームのコールバック
/// （`request_animation_frame`。部品のアニメーションが描画ごとに積む）の実行を行う。一度も呼ばないと、プログラムからの変更
/// （`cx.notify`）はフレームループが次に描くまで描かれず、次のフレームのコールバックは溜まり続ける。
/// 読み戻す描画はフレームループが担い、遠いパネルの間引き・隠れたパネルのカリングをしているので、それを打ち消さないよう、
/// 見えていてフレームループが今は描いていないウィンドウにだけ送る（AGENT.md「GPUI 自身のフレーム」）。
type FrameCallback = Box<dyn FnMut(RequestFrameOptions)>;

/// 要求から GPUI のフレームを送るまでの間隔。フレームループが描いていれば、その描画が要求を満たす。
const FRAME_DELAY: Duration = Duration::from_millis(100);
/// フレームループがこれより長く描いていないウィンドウは、見えていない（カリング中）とみなし、GPUI にも描かせない。
/// フレームループは見えているパネルを少なくとも 1 秒ごと（`IDLE_MAX`）に描く。
const CULLED: Duration = Duration::from_secs(2);
/// フレームループが描き続けているウィンドウにも、これだけ待ったら送る（溜まった次のフレームのコールバックを実行する）。
const MAX_WAIT: Duration = Duration::from_secs(1);

struct Frames {
    callback: RefCell<Option<FrameCallback>>,
    /// 要求を受けた時刻（送るまで）。
    pending: Cell<Option<Instant>>,
    /// フレームループが最後に描いた（読み戻した）時刻。
    rendered: Cell<Option<Instant>>,
    foreground: ForegroundExecutor,
    background: BackgroundExecutor,
}

/// 要求を受けてからの経過時間と、フレームループが最後に描いてからの経過時間で、今 GPUI のフレームを送るか決める。
fn frame_due(waited: Duration, since_render: Option<Duration>) -> bool {
    match since_render {
        None => false,
        Some(since) if since >= CULLED => false,
        Some(since) => since >= FRAME_DELAY || waited >= MAX_WAIT,
    }
}

impl Frames {
    fn request(self: &Rc<Self>) {
        if self.pending.get().is_some() {
            return;
        }
        self.pending.set(Some(Instant::now()));
        self.wait(FRAME_DELAY);
    }

    fn wait(self: &Rc<Self>, after: Duration) {
        let frames = Rc::downgrade(self);
        let timer = self.background.timer(after);
        self.foreground
            .spawn(async move {
                timer.await;
                // 閉じたウィンドウには送らない。
                let Some(frames) = frames.upgrade() else {
                    return;
                };
                let Some(requested) = frames.pending.get() else {
                    return;
                };
                let since_render = frames.rendered.get().map(|at| at.elapsed());
                if !frame_due(requested.elapsed(), since_render) {
                    // 描き続けている・見えていない間は待つ（見えれば描かれ、描くのをやめれば送る）。
                    frames.wait(FRAME_DELAY);
                    return;
                }
                frames.pending.set(None);
                let callback = frames.callback.borrow_mut().take();
                if let Some(mut callback) = callback {
                    callback(RequestFrameOptions {
                        signal_source: FrameRequestSource::LocalSchedule,
                        ..Default::default()
                    });
                    frames.callback.borrow_mut().get_or_insert(callback);
                }
            })
            .detach();
    }
}

struct OffscreenState {
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    input_handler: Option<PlatformInputHandler>,
    title: Option<String>,
    background: WindowBackgroundAppearance,
    renderer: WgpuHeadlessRenderer,
}

/// 表示サーバーに作らないウィンドウ。レイアウト・入力・描画は通常どおり行い、描画結果は `render_to_image` で読み戻す。
struct OffscreenWindow {
    state: Rc<RefCell<OffscreenState>>,
    appearance: WindowAppearance,
    atlas: Arc<dyn PlatformAtlas>,
    /// GPUI のウィンドウへのサイズ変更の通知。GPUI がこのウィンドウを更新している最中に呼ぶと失敗するため、タスクで遅らせて呼ぶ。
    on_resize: ResizeCallback,
    frames: Rc<Frames>,
    executor: ForegroundExecutor,
}

impl OffscreenWindow {
    fn new(
        params: WindowParams,
        executor: ForegroundExecutor,
        background: BackgroundExecutor,
        appearance: WindowAppearance,
    ) -> anyhow::Result<Self> {
        // ponytail: 背景は生成後に `set_background_appearance` で届くため、描画先は常に透明（事前乗算）で作る。
        // 不透明のウィンドウは、覆われていない部分が黒でなく透明になる（nyaw のパネルはすべて透明）。
        let renderer = WgpuHeadlessRenderer::new_transparent()?;
        let atlas = renderer.sprite_atlas();
        Ok(Self {
            state: Rc::new(RefCell::new(OffscreenState {
                bounds: params.bounds,
                scale_factor: 1.0,
                input_handler: None,
                title: None,
                background: WindowBackgroundAppearance::Transparent,
                renderer,
            })),
            appearance,
            atlas,
            on_resize: Rc::new(RefCell::new(None)),
            frames: Rc::new(Frames {
                callback: RefCell::new(None),
                pending: Cell::new(None),
                rendered: Cell::new(None),
                foreground: executor.clone(),
                background,
            }),
            executor,
        })
    }
}

impl Drop for OffscreenWindow {
    fn drop(&mut self) {
        // 遅らせた通知が、閉じたウィンドウに届かないように。
        self.on_resize.borrow_mut().take();
        self.frames.callback.borrow_mut().take();
    }
}

impl raw_window_handle::HasWindowHandle for OffscreenWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl raw_window_handle::HasDisplayHandle for OffscreenWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

/// 論理サイズ × スケールの描画解像度（切り捨て、最小 1px）。フレームループは LOD の描画を `解像度 >> lod` の大きさと
/// 照合するので、それと一致させる（スケールは 1/2^lod なので整数の解像度では割り切れる分だけ切り捨てになる）。
fn device_size(size: Size<Pixels>, scale: f32) -> Size<DevicePixels> {
    let px = |v: Pixels| DevicePixels(((f32::from(v) * scale).floor() as i32).max(1));
    Size {
        width: px(size.width),
        height: px(size.height),
    }
}

impl PlatformWindow for OffscreenWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.state.borrow().bounds
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
        let scale = {
            let mut state = self.state.borrow_mut();
            state.bounds.size = size;
            if let Some(scale) = RENDER_SCALE.get() {
                state.scale_factor = scale;
            }
            state.scale_factor
        };
        let callback = self.on_resize.clone();
        self.executor
            .spawn(async move {
                if let Some(callback) = callback.borrow_mut().as_mut() {
                    callback(size, scale);
                }
            })
            .detach();
    }

    fn scale_factor(&self) -> f32 {
        self.state.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        self.appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }

    fn mouse_position(&self) -> Point<Pixels> {
        NO_POINTER
    }

    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }

    fn capslock(&self) -> Capslock {
        Capslock::default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        // GPUI 自身が描く確認ダイアログを使う。
        None
    }

    fn activate(&self) {}

    fn is_active(&self) -> bool {
        false
    }

    fn visibility(&self) -> WindowVisibility {
        WindowVisibility::Hidden
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.state.borrow().background
    }

    fn set_title(&mut self, title: &str) {
        self.state.borrow_mut().title = Some(title.to_owned());
    }

    fn get_title(&self) -> String {
        self.state.borrow().title.clone().unwrap_or_default()
    }

    fn set_background_appearance(&self, background: WindowBackgroundAppearance) {
        self.state.borrow_mut().background = background;
    }

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        false
    }

    // 読み戻す描画は nyaw のフレームループが `Window::draw` と `render_to_image` で行う。フレーム要求は GPUI 自身の
    // 描画とアニメーションのため（[`Frames`]）。入力は直接配送されるため、プラットフォームからの入力・状態変化の通知は無い。
    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        *self.frames.callback.borrow_mut() = Some(callback);
    }

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        let frames: Weak<Frames> = Rc::downgrade(&self.frames);
        Some(Rc::new(move || {
            if let Some(frames) = frames.upgrade() {
                frames.request();
            }
        }))
    }

    fn schedule_frame(&self) {
        self.frames.request();
    }

    fn on_input(&self, _callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {}

    fn on_active_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(WindowVisibility)>) {}

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        *self.on_resize.borrow_mut() = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_display_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_close(&self, _callback: Box<dyn FnOnce()>) {}

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn draw(&self, _scene: &Scene) {}

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        // 透明な描画先にサブピクセルの文字は合成できない。
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }

    /// GPU の完了を待たず、前回の描画の読み戻しを返す（Windows の vendor/gpui_windows と同じ 1 回遅れ）。
    /// [`collect_readback`] の間は描画せず、発行済みの読み戻しを受け取る。
    fn render_to_image(&self, scene: &Scene) -> anyhow::Result<image::RgbaImage> {
        let mut state = self.state.borrow_mut();
        if COLLECT.get() {
            return state.renderer.collect_readback();
        }
        let size = device_size(state.bounds.size, state.scale_factor);
        self.frames.rendered.set(Some(Instant::now()));
        state.renderer.render_and_read_back(scene, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GPUI のフレームは、見えていて（最近フレームループが描いた）今は描かれていないウィンドウにだけ送る。
    /// 描き続けているウィンドウにも、待ちが長くなれば送る。
    #[test]
    fn frames_go_to_shown_windows_the_loop_is_not_drawing() {
        let ms = Duration::from_millis;
        assert!(!frame_due(ms(100), None), "never drawn: not shown yet");
        assert!(frame_due(ms(100), Some(ms(500))), "shown and idle");
        assert!(!frame_due(ms(100), Some(ms(20))), "the loop is drawing it");
        assert!(
            frame_due(ms(1000), Some(ms(20))),
            "drawn all along: after a while anyway"
        );
        assert!(!frame_due(ms(5000), Some(ms(3000))), "culled");
    }

    #[test]
    fn device_size_matches_the_frame_loop_and_never_collapses() {
        let size = Size {
            width: px(1024.0),
            height: px(683.0),
        };
        assert_eq!(
            device_size(size, 0.5),
            Size {
                width: DevicePixels(512),
                height: DevicePixels(341)
            }
        );
        assert_eq!(
            device_size(size, 1.0 / 8.0),
            Size {
                width: DevicePixels(128),
                height: DevicePixels(85)
            }
        );
        let tiny = Size {
            width: px(1.0),
            height: px(1.0),
        };
        assert_eq!(
            device_size(tiny, 0.125),
            Size {
                width: DevicePixels(1),
                height: DevicePixels(1)
            }
        );
    }
}
