//! Linux の描画ホスト。パネルのウィンドウを表示サーバー上に生成せず、wgpu のオフスクリーン描画先（vendor/gpui_wgpu の
//! `WgpuHeadlessRenderer`）に描画する。Linux の GPUI は Wayland / X11 のウィンドウに `render_to_image` を実装しておらず、
//! 非表示のウィンドウも生成できない。そのため、ヘッドレスの Linux プラットフォームをラップし、ウィンドウの生成のみを差し替える。
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
    /// 次の `render_to_image` で描画せず、発行済みの読み戻しを回収するか（[`collect_readback`]）。
    static COLLECT: Cell<bool> = const { Cell::new(false) };
}

/// `f`（`Window::resize` を呼び出す）の実行中の resize で、ウィンドウの描画スケールを `scale` に設定する（Windows の
/// `gpui_windows::with_render_scale` に相当）。論理サイズは維持したまま描画解像度のみが変更され、次のタスクの実行時に反映される。
pub fn with_render_scale<R>(scale: f32, f: impl FnOnce() -> R) -> R {
    RENDER_SCALE.set(Some(scale));
    let r = f();
    RENDER_SCALE.set(None);
    r
}

/// `f`（`Window::render_to_image` を呼び出す）の実行中、再描画せず前回発行した読み戻しを回収する（Windows の
/// `gpui_windows::collect_readback` に相当）。発行済みの読み戻しが存在しない場合はエラーを返す。
pub fn collect_readback<R>(f: impl FnOnce() -> R) -> R {
    COLLECT.set(true);
    let r = f();
    COLLECT.set(false);
    r
}

/// ヘッドレスの Linux プラットフォーム（イベントループと実行器）に、テキストシステムとオフスクリーンのウィンドウを組み合わせたもの。
pub struct OffscreenPlatform {
    inner: Rc<dyn Platform>,
    /// Zed のワークスペースは gpui_linux を表示サーバーの機能（wayland / x11）なしで依存に含めるため、そのヘッドレスの
    /// テキストシステムは字形を描画しない（`NoopTextSystem`）。代わりに、表示サーバーに接続する gpui_linux と同じ
    /// テキストシステム（cosmic-text とシステムのフォント）を使用する。
    text_system: Arc<CosmicTextSystem>,
}

impl OffscreenPlatform {
    pub fn new() -> Self {
        // 表示サーバーには接続しない。SteamVR から起動された場合など、表示サーバーが存在しない環境でも動作する。
        Self {
            inner: gpui_platform::current_platform(true),
            text_system: Arc::new(CosmicTextSystem::new(&system_ui_font())),
        }
    }
}

/// GPUI のシステム UI 書体（`.SystemUIFont`。書体を指定しない文字に適用される）として、fontconfig が選ぶデスクトップの
/// sans-serif を返す。Windows の Segoe UI に相当する。gpui_linux は Zed が同梱する IBM Plex Sans を指定するが、nyaw はこれを同梱しない。
fn system_ui_font() -> String {
    std::process::Command::new("fc-match")
        .args(["--format=%{family[0]}", "sans-serif"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|family| !family.is_empty())
        // ponytail: fontconfig が存在しない環境では、多くのディストリビューションが収録する書体を指定する。
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

    /// ヘッドレスのプラットフォームは URL を開かないため、デスクトップの既定のアプリケーション（xdg-open）で開く。
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

/// OS のポインタがオフスクリーンのウィンドウ上に存在することはない（nyaw は入力を `Window::dispatch_event` で直接配送する）。
/// 座標は vendor/gpui が `MouseExited` の受信時に設定する位置と同じ。
const NO_POINTER: Point<Pixels> = Point {
    x: px(-1.0e6),
    y: px(-1.0e6),
};

type ResizeCallback = Rc<RefCell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>>;

/// GPUI 自身のフレーム（`on_request_frame` のコールバック）。GPUI はこのコールバックで、再描画が必要なウィンドウの描画と、
/// 次のフレームのコールバック（`request_animation_frame`。部品はアニメーション中に描画のたびに登録する）の実行を行う。
/// 呼び出さなければ、入力を伴わない変更（`cx.notify`）はフレームループが次に描画するまで反映されず、次のフレームの
/// コールバックは実行されないまま蓄積する。一方、読み戻す描画はフレームループが担い、遠いパネルの間引きと隠れたパネルの
/// カリングを行っている。これを無効にしないよう、表示中かつフレームループが描画していないウィンドウにのみ呼び出す
/// （AGENT.md「GPUI 自身のフレーム」）。
type FrameCallback = Box<dyn FnMut(RequestFrameOptions)>;

/// フレームの要求から GPUI のフレームを呼び出すまでの間隔。その間にフレームループが描画すれば、その描画が要求を満たす。
const FRAME_DELAY: Duration = Duration::from_millis(100);
/// フレームループがこの時間を超えて描画していないウィンドウは非表示（カリング中）とみなし、GPUI にも描画させない。
/// フレームループは表示中のパネルを少なくとも 1 秒（`IDLE_MAX`）ごとに描画する。
const CULLED: Duration = Duration::from_secs(2);
/// フレームループが描画を続けているウィンドウでも、要求からこの時間が経過したらフレームを呼び出し、蓄積した次のフレームの
/// コールバックを実行させる。
const MAX_WAIT: Duration = Duration::from_secs(1);

struct Frames {
    callback: RefCell<Option<FrameCallback>>,
    /// 未処理のフレーム要求を受けた時刻。
    pending: Cell<Option<Instant>>,
    /// フレームループが最後に描画した（読み戻しを発行した）時刻。
    rendered: Cell<Option<Instant>>,
    foreground: ForegroundExecutor,
    background: BackgroundExecutor,
}

/// 要求からの経過時間と、フレームループが最後に描画してからの経過時間から、GPUI のフレームを今呼び出すかを判定する。
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
                // 閉じたウィンドウのフレームは呼び出さない。
                let Some(frames) = frames.upgrade() else {
                    return;
                };
                let Some(requested) = frames.pending.get() else {
                    return;
                };
                let since_render = frames.rendered.get().map(|at| at.elapsed());
                if !frame_due(requested.elapsed(), since_render) {
                    // フレームループが描画中、または非表示の間は待機する（表示されればフレームループが描画し、
                    // フレームループが描画を止めればフレームを呼び出す）。
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

/// 表示サーバー上に生成しないウィンドウ。レイアウト・入力・描画は通常のウィンドウと同様に処理し、描画結果は `render_to_image` で読み戻す。
struct OffscreenWindow {
    state: Rc<RefCell<OffscreenState>>,
    appearance: WindowAppearance,
    atlas: Arc<dyn PlatformAtlas>,
    /// GPUI のウィンドウへのサイズ変更の通知。GPUI がこのウィンドウを更新している最中に呼び出すと失敗するため、タスクで遅延させて呼び出す。
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
        // ponytail: 背景の指定は生成後に `set_background_appearance` で渡されるため、描画先は常に透明（事前乗算アルファ）で生成する。
        // 不透明のウィンドウでは、要素に覆われていない部分が黒ではなく透明になる（nyaw のパネルはすべて透明）。
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
        // 遅延させた通知を、閉じたウィンドウに対して呼び出さないようにする。
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

/// 論理サイズ × スケールの描画解像度（端数は切り捨て、最小 1px）。フレームループは LOD の描画結果を `解像度 >> lod` の
/// 大きさと照合するため、これと一致させる（スケールは 1/2^lod であり、整数の解像度では右シフトと同じ値になる）。
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
        // GPUI が描画する確認ダイアログを使用する。
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

    // 読み戻す描画は nyaw のフレームループが `Window::draw` と `render_to_image` で行う。フレームの要求は GPUI 自身の
    // 描画とアニメーションに用いる（[`Frames`]）。入力は直接配送されるため、プラットフォームからの入力と状態変化の通知は発生しない。
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
        // サブピクセルレンダリングの文字は透明な描画先に合成できない。
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }

    /// GPU の完了を待たず、前回の描画の読み戻しを返す（vendor/gpui_windows と同じく 1 回遅れ）。
    /// [`collect_readback`] の実行中は描画せず、発行済みの読み戻しを回収する。
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

    /// GPUI のフレームは、表示中（フレームループが直近に描画した）かつフレームループが描画していないウィンドウにのみ呼び出す。
    /// フレームループが描画を続けているウィンドウでも、要求からの待機が長くなれば呼び出す。
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
