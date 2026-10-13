//! Layer 4: GPUI フロントエンド。各オーバーレイパネルは GPUI ウィンドウで実体化され、
//! オーバーレイ層のレーザーイベントは通常の GPUI マウス入力に変換される。
//! そのためビューはデスクトップの GPUI ビューと全く同じように書け、VR 上にあることを意識しない。
//! 逆向きは [`capture`] でウィンドウをオフスクリーン描画して読み戻し、`Overlay::submit` で VR パネルへ送る。

use std::time::Duration;

use gpui::{
    App, Bounds, Context, Entity, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};
use overlay::{Overlay, PanelId, PointerEvent};
use vr::{Pose, Quat, Surface, Vec2, Vec3, VrBackend};

/// 1パネル分のレーザーイベントを GPUI のプラットフォーム入力に変換する。GPUI のイベントは
/// 位置とボタン状態を持つがオーバーレイのイベントは持たないので、直前の値を保持する。
#[derive(Debug, Default)]
pub struct PointerBridge {
    pos: Point<Pixels>,
    pressed: bool,
}

impl PointerBridge {
    pub fn translate(&mut self, ev: PointerEvent) -> PlatformInput {
        let at = |p: Vec2| Point::new(px(p.x), px(p.y));
        let held = |pressed: bool| pressed.then_some(MouseButton::Left);
        match ev {
            PointerEvent::Move { pos } => {
                self.pos = at(pos);
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: self.pos,
                    pressed_button: held(self.pressed),
                    modifiers: Default::default(),
                })
            }
            PointerEvent::Down { pos } => {
                (self.pos, self.pressed) = (at(pos), true);
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            }
            PointerEvent::Up { pos } => {
                (self.pos, self.pressed) = (at(pos), false);
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                })
            }
            PointerEvent::Scroll { pos, delta } => {
                self.pos = at(pos);
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: self.pos,
                    delta: ScrollDelta::Lines(Point::new(delta.x, delta.y)),
                    modifiers: Default::default(),
                    touch_phase: Default::default(),
                })
            }
            PointerEvent::Leave => {
                let pressed_button = held(std::mem::take(&mut self.pressed));
                PlatformInput::MouseExited(MouseExitEvent {
                    position: self.pos,
                    pressed_button,
                    modifiers: Default::default(),
                })
            }
        }
    }
}

pub const PANEL_BG: u32 = 0x202020;
pub const BUTTON_BG: u32 = 0x3060c0;
/// レーザーが当たっている間のボタン色。VR 上でどこを指しているかの視覚的フィードバック。
pub const BUTTON_HOVER_BG: u32 = 0x4f86f7;

/// 仮のホームパネル。
#[derive(Debug, Default)]
pub struct Home {
    pub clicks: usize,
}

impl Render for Home {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(PANEL_BG))
            .child(
                div()
                    .id("click-me")
                    .debug_selector(|| "click-me".into())
                    .p_4()
                    .bg(rgb(BUTTON_BG))
                    .hover(|s| s.bg(rgb(BUTTON_HOVER_BG)))
                    .text_color(rgb(0xffffff))
                    .child(format!("Clicked {}", self.clicks))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
            )
    }
}

/// 1フレーム分: 下位パイプラインを回し、`panel` 宛てのイベントを GPUI 入力に変換して返す。
/// 本番の [`run`] とテスト用の testkit が共通で通る経路。
pub fn pump(
    overlay: &mut Overlay,
    vr: &mut dyn VrBackend,
    panel: PanelId,
    bridge: &mut PointerBridge,
) -> Vec<PlatformInput> {
    // ponytail: パネル1枚・ブリッジ1つ。両手で同じパネルを触るとマウス状態が混ざる。複数ポインタが要るならブリッジを手ごとにする。
    overlay
        .tick(vr)
        .into_iter()
        .filter(|e| e.panel == panel)
        .map(|e| bridge.translate(e.event))
        .collect()
}

/// CPU に読み戻した UI の1フレーム（RGBA8、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ウィンドウを描き直し、画面に出さずにオフスクリーンへ描画して読み戻す。
/// 大きさはデバイスピクセル（論理サイズ × 拡大率）。
// ponytail: gpui の `render_to_image` は test-support フィーチャー限定かつ Windows のみ実装。
// 上流で一般公開されたらフィーチャーを外す。Linux は gpui_wgpu のヘッドレスレンダラ経由が次の一手。
pub fn capture(window: &mut Window, cx: &mut App) -> gpui::Result<Frame> {
    window.draw(cx).clear(cx);
    let img = window.render_to_image()?;
    Ok(Frame {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// パネル用ウィンドウの設定。ランタイムの [`Surface`] によって見せ方だけが変わる。
/// - `Overlay`: 描画は VR に送るので、デスクトップには出さない（非表示）。
/// - `Desktop`: OS のウィンドウとして画面中央に表示し、OS の入力をそのまま受ける。
pub fn panel_window_options(surface: Surface, resolution: (u32, u32), cx: &App) -> WindowOptions {
    let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
    match surface {
        Surface::Overlay => WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size,
            })),
            show: false,
            focus: false,
            ..Default::default()
        },
        Surface::Desktop => WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
            ..Default::default()
        },
    }
}

/// パネル用ウィンドウを開く。`Overlay` では Windows の非表示ウィンドウのレンダーターゲットが
/// 1x1 のまま作られるので、明示的にリサイズして実寸にする（反映は非同期。最初の数フレームは 1x1）。
pub fn open_panel_window<V: Render>(
    cx: &mut App,
    surface: Surface,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    let options = panel_window_options(surface, resolution, cx);
    let window = cx.open_window(options, build)?;
    if surface == Surface::Overlay {
        let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
        cx.update_window(window.into(), |_, window, _| window.resize(size))?;
    }
    Ok(window)
}

const PANEL_PX: (u32, u32) = (600, 400);
// ponytail: 約 90Hz の固定タイマー。後でランタイムの vsync (WaitGetPoses / xrWaitFrame) 駆動にする。
const FRAME: Duration = Duration::from_millis(11);

/// メインスレッドを占有する。GPUI のイベントループがランタイムのフレームループも回す。
/// どのランタイム（OpenVR / OpenXR / novr）でも同じ経路を通り、違いは `vr.surface()` だけ。
pub fn run(mut vr: Box<dyn VrBackend>) {
    let surface = vr.surface();
    gpui_platform::application().run(move |cx: &mut App| {
        let mut overlay = Overlay::new();
        let pose = Pose::new(Vec3::new(0.0, 1.2, -1.0), Quat::IDENTITY);
        let panel = overlay
            .add_panel(
                vr.as_mut(),
                "nyaw.home",
                pose,
                Vec2::new(0.6, 0.4),
                PANEL_PX,
            )
            .expect("create home overlay");

        let window = open_panel_window(cx, surface, PANEL_PX, |_, cx| cx.new(|_| Home::default()))
            .expect("open home window");
        if surface == Surface::Desktop {
            cx.activate(true);
        }

        cx.spawn(async move |cx| {
            let mut bridge = PointerBridge::default();
            let mut warned = false;
            loop {
                cx.background_executor().timer(FRAME).await;
                let inputs = pump(&mut overlay, vr.as_mut(), panel, &mut bridge);
                // WindowHandle::update はルートビューを借用したままになり、draw やクリック処理で
                // ビューを更新すると二重借用で落ちる。AnyWindowHandle 経由で借用しない。
                let Ok(frame) = cx.update_window(window.into(), |_, window, cx| {
                    for input in inputs {
                        window.dispatch_event(input, cx);
                    }
                    // Desktop では OS が表示するので読み戻さない。
                    // ponytail: Overlay では変化がなくても毎フレーム描画して送る。重ければ dirty なときだけにする。
                    (surface == Surface::Overlay).then(|| capture(window, cx))
                }) else {
                    cx.update(|cx| cx.quit()); // ウィンドウが閉じられた
                    return;
                };
                match frame {
                    Some(Ok(f)) => overlay.submit(vr.as_mut(), panel, f.width, f.height, &f.rgba),
                    Some(Err(e)) if !warned => {
                        eprintln!("nyaw: パネルの描画を取得できない: {e}");
                        warned = true;
                    }
                    _ => {}
                }
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn laser_click_reaches_gpui_view(cx: &mut TestAppContext) {
        let (home, cx) = cx.add_window_view(|_, _| Home::default());
        cx.run_until_parked();
        let c = cx
            .debug_bounds("click-me")
            .expect("button rendered")
            .center();
        let pos = Vec2::new(c.x.into(), c.y.into());

        let mut bridge = PointerBridge::default();
        for ev in [
            PointerEvent::Move { pos },
            PointerEvent::Down { pos },
            PointerEvent::Up { pos },
            PointerEvent::Leave,
        ] {
            let input = bridge.translate(ev);
            cx.update(|window, cx| {
                window.dispatch_event(input, cx);
            });
        }
        cx.run_until_parked();
        assert_eq!(home.read_with(cx, |h, _| h.clicks), 1);
    }

    #[gpui::test]
    fn panel_window_shows_only_on_desktop_surface(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(!panel_window_options(Surface::Overlay, PANEL_PX, cx).show);
            assert!(panel_window_options(Surface::Desktop, PANEL_PX, cx).show);
        });
    }

    #[gpui::test]
    fn desktop_window_matches_panel_resolution(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            open_panel_window(cx, Surface::Desktop, PANEL_PX, |_, cx| {
                cx.new(|_| Home::default())
            })
            .unwrap()
        });
        let vp = cx
            .update(|cx| cx.update_window(window.into(), |_, w, _| w.viewport_size()))
            .unwrap();
        assert_eq!(vp, size(px(PANEL_PX.0 as f32), px(PANEL_PX.1 as f32)));
    }

    #[test]
    fn leave_releases_held_button() {
        let mut b = PointerBridge::default();
        b.translate(PointerEvent::Down {
            pos: Vec2::new(3.0, 4.0),
        });
        let PlatformInput::MouseExited(e) = b.translate(PointerEvent::Leave) else {
            panic!()
        };
        assert_eq!(
            (e.position, e.pressed_button),
            (Point::new(px(3.0), px(4.0)), Some(MouseButton::Left))
        );
        assert!(!b.pressed);
    }
}
