//! 本番と同じ経路（実プラットフォーム + GPU）で、非表示のパネルウィンドウの描画を読み戻せることを確かめる。
//! GUI のイベントループはメインスレッドで回す必要があるので、テストハーネスを使わず main で実行する。

#[cfg(windows)]
fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{AppContext, Context, IntoElement, Render, Styled, Window, div, rgb};
    use ui::{capture, open_panel_window};
    use vr::Surface;

    const PANEL_BG: u32 = 0x204060;
    /// 背景を塗るだけの画面（画面は UI 層の上にあるので、UI 層のテストでは自前で用意する）。
    struct Solid;
    impl Render for Solid {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(PANEL_BG))
        }
    }

    const RES: (u32, u32) = (200, 100);
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    gpui_platform::application().run(move |cx| {
        let window = open_panel_window(cx, Surface::Overlay, RES, |_, cx| cx.new(|_| Solid))
            .expect("パネルウィンドウを開けない");
        cx.spawn(async move |cx| {
            // リサイズの反映は非同期なので、実寸で読み戻せるまで待つ（最大 5 秒）
            for _ in 0..100 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let captured = cx
                    .update_window(window.into(), |_, window, cx| {
                        (capture(window, cx), window.scale_factor())
                    })
                    .expect("ウィンドウが閉じている");
                let done = matches!(&captured.0, Ok(f) if f.width > 1) || captured.0.is_err();
                *out.borrow_mut() = Some(captured);
                if done {
                    break;
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let (frame, scale) = result.take().expect("アプリが起動しなかった");
    let frame = frame.expect("GPU から読み戻せない");
    assert_eq!(
        (frame.width, frame.height),
        ((RES.0 as f32 * scale) as u32, (RES.1 as f32 * scale) as u32)
    );
    assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    let [r, g, b] = [PANEL_BG >> 16, PANEL_BG >> 8, PANEL_BG].map(|c| (c & 0xff) as u8);
    assert_eq!(&frame.rgba[..4], &[r, g, b, 0xff], "左上はパネル背景色");
    println!("gpu_capture: ok ({}x{})", frame.width, frame.height);
}

// ponytail: render_to_image は gpui 側で Windows のみ実装。Linux 対応時にこの分岐を消す。
#[cfg(not(windows))]
fn main() {
    println!("gpu_capture: skipped (Windows 以外は未対応)");
}
