//! 本番の描画と入力配送を通し、透過・照明・角丸・左右のポインタを GPU の画素で検証する。
mod common;

use design::{Effects, Mode, Theme, components};
use gpui::{AppContext, Context, IntoElement, Render, Window, div, prelude::*, px};
use kernel::{Hand, Vec2};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct View;

struct Foreground;
impl Render for Foreground {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .size_full()
            .relative()
            .children(
                design::Weight::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(i, weight)| {
                        div()
                            .absolute()
                            .left(px(i as f32 * 32.))
                            .top(px(20.))
                            .child(
                                design::icon(&design::icons::HEART)
                                    .weight(weight)
                                    .size(px(32.))
                                    .color(gpui::rgba(theme.neutral_text)),
                            )
                    }),
            )
            .child(
                div().absolute().top(px(90.)).child(
                    design::icon(&design::icons::HEART)
                        .size(px(64.))
                        .rotate(25.)
                        .opacity(0.6)
                        .color(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(gpui::rgba(theme.accent_solid), 0.),
                            gpui::linear_color_stop(gpui::rgba(theme.danger_solid), 1.),
                        )),
                ),
            )
            .child(div().absolute().left(px(80.)).top(px(110.)).child("Text"))
            .child(
                div()
                    .absolute()
                    .left(px(204.))
                    .top(px(8.))
                    .w(px(144.))
                    .h(px(156.))
                    .rounded(px(24.))
                    .border_1()
                    .border_color(gpui::rgba(theme.neutral_text_low)),
            )
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, _, w, _| {
                        let mut path = gpui::PathBuilder::fill();
                        path.move_to(gpui::point(px(250.), px(40.)));
                        path.line_to(gpui::point(px(330.), px(40.)));
                        path.line_to(gpui::point(px(330.), px(140.)));
                        path.line_to(gpui::point(px(250.), px(140.)));
                        path.close();
                        w.paint_path(path.build().unwrap(), gpui::rgba(theme.control));
                    },
                )
                .size_full(),
            )
            .child(
                div().absolute().left(px(265.)).top(px(60.)).child(
                    design::icon(&design::icons::HEART)
                        .weight(design::Weight::Fill)
                        .size(px(48.))
                        .color(gpui::rgba(theme.neutral_text)),
                ),
            )
    }
}

struct Frost;
impl Render for Frost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::canvas(
            |_, _, _| (),
            |_, _, window, _| {
                for x in 0..160 {
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(px(x as f32 * 2.), px(0.)),
                            gpui::size(px(2.), px(200.)),
                        ),
                        gpui::rgba(if x % 2 == 0 { 0x000000ff } else { 0xffffffff }),
                    ));
                }
                window.paint_quad(
                    gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(px(30.), px(30.)),
                            gpui::size(px(260.), px(140.)),
                        ),
                        gpui::rgba(Theme::DARK.translucent().card),
                    )
                    .corner_radii(px(24.)),
                );
            },
        )
        .size_full()
    }
}
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        components::card_panel(&theme)
            .rounded(px(48.))
            .child(
                components::button(
                    &theme,
                    "material-button",
                    "Button",
                    components::Variant::Default,
                )
                .absolute()
                .left(px(280.))
                .top(px(80.))
                .w(px(130.)),
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

fn button_event(down: bool, pos: Vec2, window: &mut Window, cx: &mut gpui::App) {
    let mut bridge = ui::PointerBridge::default();
    let event = if down {
        kernel::PointerEvent::Down { pos }
    } else {
        kernel::PointerEvent::Up { pos }
    };
    for input in bridge.translate(event, kernel::PointerMode::Mouse) {
        ui::dispatch_input(0, Hand::Left, input, window, cx);
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
    let frost = Rc::new(RefCell::new(None));
    let frost_output = frost.clone();
    let pressed = Rc::new(RefCell::new(Vec::new()));
    let pressed_output = pressed.clone();
    let foregrounds = Rc::new(RefCell::new(Vec::new()));
    let foreground_output = foregrounds.clone();
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
        let frost_window = ui::open_panel_window(cx, (320, 200), |_, cx| {
            let view = cx.new(|_| Frost).into();
            cx.new(|_| ui::PanelRoot::new(view))
        })
        .unwrap();
        let foreground_window = ui::open_panel_window(cx, (360, 180), |_, cx| {
            let view = cx.new(|_| Foreground).into();
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
                let frames = cx
                    .update_window(foreground_window.into(), |_, window, cx| {
                        cx.set_global(mode);
                        move_pointer(Hand::Left, Some(Vec2::new(80., 40.)), window, cx);
                        move_pointer(Hand::Right, Some(Vec2::new(290., 90.)), window, cx);
                        let mut frames = Vec::new();
                        for effects in [
                            Effects::default(),
                            Effects {
                                translucent: true,
                                pointer_light: false,
                            },
                            Effects {
                                translucent: false,
                                pointer_light: true,
                            },
                            Effects::ENABLED,
                        ] {
                            effects.set(cx);
                            window.refresh();
                            frames.push(ui::capture(window, cx).unwrap());
                        }
                        Effects {
                            translucent: true,
                            pointer_light: false,
                        }
                        .set(cx);
                        for strength in [0., 3.] {
                            Effects::set_edge_strength(strength, cx);
                            window.refresh();
                            frames.push(ui::capture(window, cx).unwrap());
                        }
                        Effects::set_edge_strength(1., cx);
                        (mode, window.scale_factor(), frames)
                    })
                    .unwrap();
                foreground_output.borrow_mut().push(frames);
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
                            move_pointer(Hand::Left, Some(Vec2::new(330., 125.)), window, cx);
                            let foreground = take(window, cx);
                            Effects::set_edge_strength(0., cx);
                            let weak_edge = take(window, cx);
                            Effects::set_edge_strength(3., cx);
                            let strong_edge = take(window, cx);
                            Effects::set_edge_strength(1., cx);
                            (
                                mode,
                                translucent,
                                window.scale_factor(),
                                baseline,
                                lit,
                                right,
                                leave,
                                corner,
                                foreground,
                                weak_edge,
                                strong_edge,
                            )
                        })
                        .unwrap();
                    output.borrow_mut().push(frames);
                }
            }
            *frost_output.borrow_mut() = Some(
                cx.update_window(frost_window.into(), |_, window, cx| {
                    Effects::default().set(cx);
                    window.refresh();
                    let baseline = ui::capture(window, cx).unwrap();
                    Effects {
                        translucent: true,
                        pointer_light: false,
                    }
                    .set(cx);
                    window.refresh();
                    let blurred = ui::capture(window, cx).unwrap();
                    (window.scale_factor(), baseline, blurred)
                })
                .unwrap(),
            );
            let frame = cx
                .update_window(test_screen.into(), |_, window, cx| {
                    cx.set_global(Mode::Dark);
                    cx.set_global(i18n::Lang::Ja);
                    Effects::ENABLED.set(cx);
                    move_pointer(Hand::Right, Some(Vec2::new(400., 360.)), window, cx);
                    window.refresh();
                    let dark = ui::capture(window, cx).unwrap();
                    cx.set_global(Mode::Light);
                    window.refresh();
                    let light = ui::capture(window, cx).unwrap();
                    (dark, light)
                })
                .unwrap();
            *screen_output.borrow_mut() = Some(frame);
            cx.update(|cx| cx.set_global(design::Motion::default()));
            for mode in [Mode::Dark, Mode::Light] {
                for (name, y) in [("default", 400.), ("primary", 458.)] {
                    let pos = Vec2::new(150., y);
                    cx.update_window(test_screen.into(), |_, window, cx| {
                        cx.set_global(mode);
                        move_pointer(Hand::Right, None, window, cx);
                        move_pointer(Hand::Left, Some(pos), window, cx);
                        button_event(true, pos, window, cx);
                        window.refresh();
                        ui::capture(window, cx).unwrap();
                    })
                    .unwrap();
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    let (scale, down) = cx
                        .update_window(test_screen.into(), |_, window, cx| {
                            window.refresh();
                            let frame = ui::capture(window, cx).unwrap();
                            button_event(false, pos, window, cx);
                            window.refresh();
                            ui::capture(window, cx).unwrap();
                            (window.scale_factor(), frame)
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(Duration::from_millis(230))
                        .await;
                    let up = cx
                        .update_window(test_screen.into(), |_, window, cx| {
                            window.refresh();
                            ui::capture(window, cx).unwrap()
                        })
                        .unwrap();
                    pressed_output
                        .borrow_mut()
                        .push((mode, name, y, scale, down, up));
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let results = results.borrow();
    for (mode, scale, frames) in foregrounds.borrow().iter() {
        assert!(
            frames[0].rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 128),
            "前景を描画する"
        );
        for frame in &frames[1..] {
            assert_eq!(
                pixel(frame, *scale, 289., 84.),
                pixel(&frames[0], *scale, 289., 84.),
                "照らされた操作面上でもアイコンの色を保つ"
            );
            assert_eq!(
                pixel(frame, *scale, 220., 80.)[3],
                0,
                "背景のないコンテナ内に面を作らない"
            );
            assert_eq!(
                pixel(frame, *scale, 204., 80.),
                pixel(&frames[0], *scale, 204., 80.),
                "塗りのない枠線を材質から除外する"
            );
            for y in 0..frame.height {
                for x in 0..(200. * scale) as u32 {
                    let i = ((y * frame.width + x) * 4) as usize;
                    assert_eq!(
                        &frame.rgba[i..i + 4],
                        &frames[0].rgba[i..i + 4],
                        "{mode:?}: 文字と全ウェイト・半透明・グラデーションのアイコンを材質から除外 ({x}, {y})"
                    );
                }
            }
        }
        let base = pixel(&frames[0], *scale, 320., 110.);
        let lit = pixel(&frames[2], *scale, 320., 110.);
        assert!(
            lit[0] > base[0],
            "任意形状の操作面は照明を維持 {base:?} -> {lit:?}"
        );
        let weak = pixel(&frames[4], *scale, 290., 138.);
        let strong = pixel(&frames[5], *scale, 290., 138.);
        assert!(
            strong[0] < weak[0],
            "任意形状の縁の強さを調整できる {weak:?} -> {strong:?}"
        );
        assert_eq!(
            pixel(&frames[4], *scale, 320., 110.),
            pixel(&frames[5], *scale, 320., 110.),
            "エッジの調整は面の中央の色を変えない"
        );
        for (base, lit) in [(&frames[0], &frames[2]), (&frames[1], &frames[3])] {
            for (x, y) in [(251., 90.), (328., 90.), (290., 41.), (290., 138.)] {
                let a = pixel(base, *scale, x, y);
                let b = pixel(lit, *scale, x, y);
                assert!(
                    (0..3).all(|c| b[c] >= a[c]) && b[..3] != a[..3],
                    "{mode:?}: 内側のポインタは全周を照らし、凹面の暗い縁を作らない ({x}, {y}): {a:?} -> {b:?}"
                );
            }
        }
        snapshot(&format!("{mode:?}-foreground-exclusion"), &frames[3]);
    }
    let frost = frost.borrow();
    let (scale, baseline, blurred) = frost.as_ref().unwrap();
    let a = pixel(blurred, *scale, 100., 140.);
    let b = pixel(blurred, *scale, 102., 140.);
    assert!(
        (a[0] as i32 - b[0] as i32).abs() < 10,
        "ネイティブ GPU でも背面がぼける {a:?} {b:?}"
    );
    assert_eq!(
        pixel(baseline, *scale, 30., 30.),
        pixel(blurred, *scale, 30., 30.),
        "ぼかしを角丸の外へ漏らさない"
    );
    snapshot("native-frosted", blurred);
    let screen = screen.borrow();
    let (dark, light) = screen.as_ref().expect("確認画面を描画する");
    snapshot("test-screen", dark);
    snapshot("test-screen-light", light);
    for (mode, name, y, scale, down, up) in pressed.borrow().iter() {
        snapshot(&format!("{mode:?}-{name}-pressed"), down);
        snapshot(&format!("{mode:?}-{name}-released"), up);
        let surface = pixel(down, *scale, 80., *y);
        let card = pixel(down, *scale, 80., 350.);
        let released = pixel(up, *scale, 80., *y);
        let brightness =
            |p: [u8; 4]| p[0] as f32 * 0.2126 + p[1] as f32 * 0.7152 + p[2] as f32 * 0.0722;
        assert!(
            brightness(surface) < brightness(card),
            "{mode:?} {name}: 押下はカードより深い色 {surface:?} {card:?}"
        );
        assert!(
            brightness(released) > brightness(card),
            "解放するとホバーの明るい面へ戻る {released:?} {card:?}"
        );
        // 見本の中心はポインタより 1px 下。上下の縁の同じ深さで比較する。
        let top = pixel(down, *scale, 150., *y - 25.);
        let bottom = pixel(down, *scale, 150., *y + 26.);
        assert!(
            brightness(top) < brightness(bottom),
            "{mode:?} {name}: 押下は上が暗く下が明るい内側の影 {top:?} {bottom:?}"
        );
    }
    assert_eq!(results.len(), 4);
    for (
        mode,
        translucent,
        scale,
        baseline,
        lit,
        right,
        leave,
        corner,
        foreground,
        weak_edge,
        strong_edge,
    ) in results.iter()
    {
        let p = |frame, x, y| pixel(frame, *scale, x, y);
        assert_eq!(
            p(weak_edge, 120., 140.),
            p(strong_edge, 120., 140.),
            "エッジの調整は面の照明とアルファを変えない"
        );
        if *translucent {
            assert!(
                p(strong_edge, 240., 294.)[0] < p(weak_edge, 240., 294.)[0],
                "角丸の面の縁の強さを調整できる"
            );
        }
        let center = p(baseline, 120., 140.);
        let illuminated = p(lit, 120., 140.);
        assert_eq!(
            center[3] == 255,
            !translucent,
            "{mode:?}: 透過フラグがアルファに反映される {center:?}"
        );
        assert_eq!(
            illuminated[3], center[3],
            "照明だけでは材質の不透明度を変えない"
        );
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
        let edge = p(baseline, 305., 130.);
        let lit_edge = p(foreground, 305., 130.);
        assert!(
            (edge[0] as i32 - lit_edge[0] as i32).abs() > 8,
            "{mode:?}: ボタンの輪郭にも照明が当たる {edge:?} -> {lit_edge:?}"
        );
        if *mode == Mode::Light {
            assert_ne!(illuminated[..3], center[..3], "白い面でも照明を識別できる");
        }
        snapshot(&format!("{mode:?}-{translucent}-foreground"), foreground);
        println!(
            "surface_effects: {mode:?} translucent={translucent} center={center:?} lit={illuminated:?}"
        );
        snapshot(&format!("{mode:?}-{translucent}-baseline"), baseline);
        snapshot(&format!("{mode:?}-{translucent}-lit"), lit);
    }
}
