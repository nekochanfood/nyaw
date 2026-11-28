//! The full-logo intro (`design::intro_logo`) plays, ends on the logo, and reacts to a press after the intro: pressing the
//! left ear moves it, then it settles back. (That presses during the intro are ignored is tested by `design::lottie::Playback`;
//! the intro twitches the ears itself, so pictures alone cannot tell.)
//! Real platform + GPU, so it needs a display. The GUI event loop must run on the main thread, so it has its own main.

pub fn main() {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use design::Logo;
    use gpui::{
        AppContext, Context, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
        PlatformInput, Render, Window, div, point, prelude::*, px,
    };
    use ui::{capture, open_panel_window};

    const FRAME: (u32, u32) = (600, 300);
    const HEIGHT: f32 = 144.0;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(design::intro_logo("intro").height(px(HEIGHT)))
        }
    }

    // The left ear in composition units (assets/animations/logo-full: 401 × 199, the logo outline inset by 24),
    // mapped to the window: the logo box is centered and the composition sticks out of it by the margin.
    let width = HEIGHT * Logo::Full.aspect();
    let k = width / 352.49;
    let (stage_w, stage_h) = (width + 48.0 * k, HEIGHT + 48.0 * k);
    let origin = (
        (FRAME.0 as f32 - stage_w) / 2.0,
        (FRAME.1 as f32 - stage_h) / 2.0,
    );
    let ear = (
        origin.0 + 86.0 / 401.0 * stage_w,
        origin.1 + 50.0 / 199.0 * stage_h,
    );

    let shots: Rc<RefCell<Vec<(&str, ui::Frame)>>> = Rc::new(RefCell::new(Vec::new()));
    let out = shots.clone();
    ui::application().run(move |cx| {
        cx.set_global(design::Motion::default());
        let window = open_panel_window(cx, FRAME, |_, cx| cx.new(|_| View))
            .expect("cannot open panel window");
        cx.spawn(async move |cx| {
            let press = |cx: &mut gpui::AsyncApp| {
                let at = point(px(ear.0), px(ear.1));
                for event in [
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }),
                ] {
                    let _ = cx.update_window(window.into(), |_, window, cx| {
                        window.dispatch_event(event, cx)
                    });
                }
            };
            let start = std::time::Instant::now();
            // Capture at these times since the window opened (the intro is 3 s), pressing the ear at the marked ones.
            let plan: [(&str, u64, bool); 5] = [
                ("early", 250, false),
                ("mid", 1000, false),
                ("end", 3800, true),
                ("twitch", 3900, false),
                ("settled", 4800, false),
            ];
            for (name, at, then_press) in plan {
                // Draw every 16 ms until the planned time so the background renders keep up.
                loop {
                    let _ = cx.update_window(window.into(), |_, window, cx| capture(window, cx));
                    if start.elapsed() >= Duration::from_millis(at) {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
                let frame = cx
                    .update_window(window.into(), |_, window, cx| capture(window, cx))
                    .expect("window is closed")
                    .expect("captured");
                out.borrow_mut().push((name, frame));
                if then_press {
                    press(cx);
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let shots = shots.take();
    let get = |name: &str| &shots.iter().find(|(n, _)| *n == name).expect(name).1;
    let opaque = |f: &ui::Frame| {
        f.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 128)
            .count()
    };
    // Pixels that differ in a box around the ear.
    let diff_near_ear = |a: &ui::Frame, b: &ui::Frame| {
        let r = (stage_h * 0.3) as i32;
        let mut n = 0;
        for y in (ear.1 as i32 - r).max(0)..(ear.1 as i32 + r).min(FRAME.1 as i32) {
            for x in (ear.0 as i32 - r).max(0)..(ear.0 as i32 + r).min(FRAME.0 as i32) {
                let i = ((y as u32 * a.width + x as u32) * 4) as usize;
                if a.rgba[i..i + 4]
                    .iter()
                    .zip(&b.rgba[i..i + 4])
                    .any(|(p, q)| p.abs_diff(*q) > 40)
                {
                    n += 1;
                }
            }
        }
        n
    };
    for (name, f) in &shots {
        crate::look::save(f, &format!("target/tmp/intro-logo-{name}.bmp"));
        println!("intro_logo: {name} ({} opaque px)", opaque(f));
    }
    let (early, end) = (get("early"), get("end"));
    assert!(
        opaque(early) * 4 < opaque(end),
        "the logo builds up during the intro"
    );
    assert!(
        opaque(get("mid")) < opaque(end),
        "the intro is still building up at 1 s"
    );
    let i = ((ear.1 as u32 * end.width + ear.0 as u32) * 4 + 3) as usize;
    assert!(end.rgba[i] > 128, "the left ear is where the test presses");
    let twitched = diff_near_ear(get("end"), get("twitch"));
    assert!(
        twitched > 50,
        "pressing the ear after the intro moves it ({twitched} px)"
    );
    let back = diff_near_ear(get("end"), get("settled"));
    assert!(back < 20, "the ear settles back ({back} px)");
    println!("intro_logo: twitch {twitched} px, settled {back} px");
}
