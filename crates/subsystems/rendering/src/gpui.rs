//! GPUI の非表示描画先・読み戻し・描画密度。
use ::gpui::{App, AppContext, Entity, Render, Window, WindowHandle, WindowOptions, px, size};
mod lod;
pub use lod::set_render_lod;

/// CPU に読み戻した UI の1フレーム（RGBA8、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ウィンドウを描き直し、画面に出さずにオフスクリーンへ描画して読み戻す。
/// 大きさはデバイスピクセル（論理サイズ × 拡大率）。
// ponytail: gpui の `render_to_image` は test-support フィーチャー限定。上流で一般公開されたらフィーチャーを外す。
pub fn capture(window: &mut Window, cx: &mut App) -> gpui::Result<Frame> {
    window.draw(cx).clear(cx);
    let frame = read_back(window)?;
    // 読み戻しは 1 回遅れなので、いま描いた絵を受け取る（積んでいなければ返った絵がいまの絵）。
    Ok(collect(window).unwrap_or(frame))
}

/// 描き直し済みのシーンを GPU で描画し、読み戻しを積む（[`capture`] の後半。プロファイラが別々に測るため分けてある）。
/// GPU の完了を待たず、前に積んだ読み戻しの絵（無ければ最後に返した絵）を返す。いま描いた絵は
/// 次のフレームの [`collect`] か、次の `read_back` で受け取る。
pub fn read_back(window: &mut Window) -> gpui::Result<Frame> {
    let img = window.render_to_image()?;
    Ok(Frame {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// 前の [`read_back`] で積んだ読み戻しを、描き直さずに受け取る。積んでいなければ `None`。
pub fn collect(window: &mut Window) -> Option<Frame> {
    #[cfg(windows)]
    let img = gpui_windows::collect_readback(|| window.render_to_image());
    img.ok().map(|img| Frame {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// 指定された表示方針でウィンドウを開く。Windows の非表示ウィンドウのレンダーターゲットは
/// 1x1 のまま作られるので、明示的にリサイズして実寸にする（反映は非同期。最初の数フレームは 1x1）。
pub fn open_window<V: Render>(
    cx: &mut App,
    mut options: WindowOptions,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    // OS の最小幅に、小さな描画先を広げられないよう枠を持たせない。
    options.kind = gpui::WindowKind::PopUp;
    options.window_min_size = Some(size(px(1.), px(1.)));
    let window = cx.open_window(options, build)?;
    let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
    cx.update_window(window.into(), |_, window, _| window.resize(size))?;
    Ok(window)
}
