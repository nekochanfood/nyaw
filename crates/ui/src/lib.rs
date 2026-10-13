//! Layer 4: GPUI フロントエンド。各オーバーレイパネルは GPUI ウィンドウで実体化され、
//! オーバーレイ層のレーザーイベントは通常の GPUI マウス入力に変換される。
//! そのためビューはデスクトップの GPUI ビューと全く同じように書け、VR 上にあることを意識しない。

use std::time::Duration;

use gpui::{
    App, Bounds, Context, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use overlay::{Overlay, PanelId, PointerEvent};
use vr::{Pose, Quat, Vec2, Vec3, VrBackend};

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
            .bg(rgb(0x202020))
            .child(
                div()
                    .id("click-me")
                    .debug_selector(|| "click-me".into())
                    .p_4()
                    .bg(rgb(0x3060c0))
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

const PANEL_PX: (u32, u32) = (600, 400);
// ponytail: 約 90Hz の固定タイマー。後でランタイムの vsync (WaitGetPoses / xrWaitFrame) 駆動にする。
const FRAME: Duration = Duration::from_millis(11);

/// メインスレッドを占有する。GPUI のイベントループが VR のフレームループも回す。
pub fn run(mut vr: Box<dyn VrBackend>) {
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

        // ponytail: パネルは普通のデスクトップウィンドウで、ピクセルはまだ VR に送っていない。
        // GPUI にはオフスクリーン描画先がないので、次の段階でウィンドウのサーフェスを取得して
        // `overlay.submit(vr, panel, rgba)` に流す。
        let bounds = Bounds::centered(None, size(px(PANEL_PX.0 as f32), px(PANEL_PX.1 as f32)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Home::default()),
            )
            .expect("open home window");

        cx.spawn(async move |cx| {
            let mut bridge = PointerBridge::default();
            loop {
                cx.background_executor().timer(FRAME).await;
                for input in pump(&mut overlay, vr.as_mut(), panel, &mut bridge) {
                    let alive = window.update(cx, |_, window, cx| {
                        window.dispatch_event(input, cx);
                    });
                    if alive.is_err() {
                        return; // ウィンドウが閉じられた
                    }
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
