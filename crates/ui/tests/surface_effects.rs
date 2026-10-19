//! 本番の描画と入力配送を通し、透過・照明・角丸・左右のポインタを GPU の画素で検証する。
mod common;

use design::{Effects, Mode, Theme, components};
use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px, rgba};
use kernel::{Hand, Vec2};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct View;
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        components::card_panel(&theme)
            .rounded(px(48.))
            .child(
                div()
                    .absolute()
                    .left(px(280.))
                    .top(px(80.))
                    .size(px(48.))
                    .bg(rgba(theme.accent_solid)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(50.))
                    .top(px(210.))
                    .child("Translucent / Pointer light"),
            )
    }
}

fn pixel(frame: &ui::Frame, scale: f32, x: f32, y: f32) -> [u8; 4] {
    let index = (((y * scale) as u32 * frame.width + (x * scale) as u32) * 4) as usize;
    frame.rgba[index..index + 4].try_into().unwrap()
}

fn move_pointer(hand: Hand, position: Option<Vec2>, window: &mut Window, cx: &mut gpui::App) {
    let mut bridge = ui::PointerBridge::default();
    let event = position.map_or(kernel::PointerEvent::Leave, |pos| {
        kernel::PointerEvent::Move { pos }
    });
    for input in bridge.translate(event, kernel::PointerMode::Mouse) {
        ui::dispatch_input(0, hand, input, window, cx);
    }
}

fn snapshot(name: &str, frame: &ui::Frame) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/surface-effects");
    std::fs::create_dir_all(&directory).unwrap();
    let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
    for (index, pixel) in frame.rgba.as_chunks::<4>().0.iter().enumerate() {
        let x = index as u32 % frame.width;
        let y = index as u32 / frame.width;
        let background = if (x / 32 + y / 32).is_multiple_of(2) {
            [75, 94, 126]
        } else {
            [112, 83, 119]
        };
        for channel in 0..3 {
            ppm.push(
                ((pixel[channel] as u32 * pixel[3] as u32
                    + background[channel] * (255 - pixel[3] as u32))
                    / 255) as u8,
            );
        }
    }
    std::fs::write(directory.join(format!("{name}.ppm")), ppm).unwrap();
}

fn main() {
    if !common::should_run("surface_effects") {
        return;
    }
    let results = Rc::new(RefCell::new(Vec::new()));
    let output = results.clone();
    let screen = Rc::new(RefCell::new(None));
    let screen_output = screen.clone();
    ui::application().run(move |cx| {
        design::font::install(cx);
        let window = ui::open_panel_window(cx, (480, 320), |_, cx| {
            let view = cx.new(|_| View).into();
            cx.new(|_| ui::PanelRoot::new(view))
        })
        .unwrap();
        let test_screen = ui::open_panel_window(cx, (800, 600), |_, cx| {
            let view = cx.new(|_| testing::EffectsTest).into();
            cx.new(|_| ui::PanelRoot::new(view))
        })
        .unwrap();
        cx.spawn(async move |cx| {
            for _ in 0..100 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let ready = cx
                    .update_window(window.into(), |_, window, cx| {
                        ui::capture(window, cx).is_ok_and(|frame| frame.width > 1)
                    })
                    .unwrap();
                if ready {
                    break;
                }
            }
            for mode in [Mode::Dark, Mode::Light] {
                for translucent in [false, true] {
                    let frames = cx
                        .update_window(window.into(), |_, window, cx| {
                            cx.set_global(mode);
                            let take = |window: &mut Window, cx: &mut gpui::App| {
                                window.refresh();
                                ui::capture(window, cx).unwrap()
                            };
                            Effects {
                                translucent,
                                pointer_light: false,
                            }
                            .set(cx);
                            move_pointer(Hand::Left, Some(Vec2::new(120., 140.)), window, cx);
                            let baseline = take(window, cx);
                            Effects {
                                translucent,
                                pointer_light: true,
                            }
                            .set(cx);
                            let lit = take(window, cx);
                            move_pointer(Hand::Right, Some(Vec2::new(360., 250.)), window, cx);
                            move_pointer(Hand::Left, None, window, cx);
                            let right = take(window, cx);
                            move_pointer(Hand::Right, None, window, cx);
                            let leave = take(window, cx);
                            move_pointer(Hand::Left, Some(Vec2::new(40., 40.)), window, cx);
                            let corner = take(window, cx);
                            (
                                mode,
                                translucent,
                                window.scale_factor(),
                                baseline,
                                lit,
                                right,
                                leave,
                                corner,
                            )
                        })
                        .unwrap();
                    output.borrow_mut().push(frames);
                }
            }
            let frame = cx
                .update_window(test_screen.into(), |_, window, cx| {
                    cx.set_global(Mode::Dark);
                    cx.set_global(i18n::Lang::Ja);
                    Effects::ENABLED.set(cx);
                    move_pointer(Hand::Right, Some(Vec2::new(400., 360.)), window, cx);
                    window.refresh();
                    ui::capture(window, cx).unwrap()
                })
                .unwrap();
            *screen_output.borrow_mut() = Some(frame);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let results = results.borrow();
    snapshot(
        "test-screen",
        screen.borrow().as_ref().expect("確認画面を描画する"),
    );
    assert_eq!(results.len(), 4);
    for (mode, translucent, scale, baseline, lit, right, leave, corner) in results.iter() {
        let p = |frame, x, y| pixel(frame, *scale, x, y);
        let center = p(baseline, 120., 140.);
        let illuminated = p(lit, 120., 140.);
        assert_eq!(
            center[3] == 255,
            !translucent,
            "{mode:?}: 透過フラグがアルファに反映される {center:?}"
        );
        if *translucent {
            assert!(
                illuminated[3] > center[3],
                "明るいテーマでも光が面のアルファに反映される"
            );
        }
        if *mode == Mode::Dark {
            assert!(
                illuminated[0] > center[0] + 15,
                "光源の近くが明るくなる: {center:?} -> {illuminated:?}"
            );
            assert!(
                p(right, 360., 250.)[0] > p(baseline, 360., 250.)[0] + 15,
                "右手だけでも照らす"
            );
        }
        assert_eq!(
            p(lit, 360., 250.),
            p(baseline, 360., 250.),
            "遠くは照らさない"
        );
        assert_eq!(p(leave, 120., 140.), center, "両手が離れると消える");
        assert_eq!(
            p(corner, 25., 25.),
            p(baseline, 25., 25.),
            "角丸の外には漏れない"
        );
        assert_eq!(
            p(lit, 320., 120.),
            p(baseline, 320., 120.),
            "操作部品の色は変えない"
        );
        println!(
            "surface_effects: {mode:?} translucent={translucent} center={center:?} lit={illuminated:?}"
        );
        snapshot(&format!("{mode:?}-{translucent}-baseline"), baseline);
        snapshot(&format!("{mode:?}-{translucent}-lit"), lit);
    }
}
