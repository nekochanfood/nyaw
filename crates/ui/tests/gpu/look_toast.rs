//! 見た目の確認用: 通知のトースト（視界の端・手首・手首でホバーしたもの）を、テーマを替えながら実 GPU で描き、
//! `target/tmp/look-toast-<置き場所>-<状態>.bmp` に書き出す（透明の上に描くので、中身だけが見える）。

use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};

use design::Mode;
use gpui::{AppContext, Modifiers, MouseMoveEvent, PlatformInput, point, px};
use kernel::NotificationAnchor;
use kernel::subsystems::notifications::{Action, App, Notification};
use ui::{PanelControl, capture, open_panel_window};

fn note(id: u32, app: &str, title: &str, body: &str, actions: &[&str]) -> Arc<Notification> {
    Arc::new(Notification {
        id,
        app: App {
            id: app.into(),
            name: app.into(),
            icon: None,
        },
        title: title.into(),
        body: body.into(),
        arrived: SystemTime::now(),
        url: None,
        actions: actions
            .iter()
            .map(|a| Action {
                label: (*a).into(),
                url: None,
            })
            .collect(),
    })
}

pub fn main() {
    if !crate::common::should_run("look_toast") {
        return;
    }
    let shots = Rc::new(RefCell::new(Vec::new()));
    let out = shots.clone();
    ui::application().run(move |cx| {
        design::font::install(cx);
        design::corner::install();
        // 手首の時計: 1 つ目のトーストのものは見ておらず（HUD に出る）、ほかは見ている（手首に出る）。
        let ctl = cx.default_global::<PanelControl>();
        ctl.settings.notification_anchor = NotificationAnchor::TopRight;
        for wrist in [101, 102] {
            ctl.panels.entry(wrist).or_default().status.gazed = true;
        }
        let open = |panel, wrist, cx: &mut gpui::App| {
            open_panel_window(cx, toast::RES_PX, move |_, cx| {
                cx.new(|cx| toast::Toast::new(panel, wrist, cx))
            })
            .expect("cannot open panel window")
        };
        let windows = [
            ("hud", open(10, 100, cx)),
            ("wrist", open(11, 101, cx)),
            ("wrist-hover", open(12, 102, cx)),
        ];
        cx.default_global::<PanelControl>().notifications = Arc::from([
            note(
                1,
                "Calendar",
                "Standup in 10 minutes",
                "Room 3 · with the VR team",
                &[],
            ),
            note(
                2,
                "Discord",
                "kissa: are you joining tonight?",
                "We are starting at nine. Bring the new controller profile, and the long \
                 notes about the dashboard placement so we can go over them together.",
                &["Reply", "Mute"],
            ),
        ]);
        cx.spawn(async move |cx| {
            for (state, mode) in [("dark", Mode::Dark), ("light", Mode::Light)] {
                cx.update(|cx| {
                    cx.set_global(mode);
                    cx.set_global(if mode == Mode::Light {
                        i18n::Lang::Ja
                    } else {
                        i18n::Lang::En
                    });
                });
                for (name, window) in windows {
                    let mut last = None;
                    for _ in 0..6 {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        // 一番新しい（一番上の）カードの上にポインタを置く。
                        if name == "wrist-hover" {
                            let at = point(px(210.0), px(toast::RES_PX.1 as f32 - 200.0));
                            let event = PlatformInput::MouseMove(MouseMoveEvent {
                                position: at,
                                pressed_button: None,
                                modifiers: Modifiers::default(),
                            });
                            let _ = cx.update_window(window.into(), |_, window, cx| {
                                window.dispatch_event(event, cx)
                            });
                        }
                        let frame = cx
                            .update_window(window.into(), |_, window, cx| capture(window, cx))
                            .expect("window is closed");
                        last = frame.ok().filter(|f| f.width == toast::RES_PX.0);
                    }
                    out.borrow_mut()
                        .push((format!("{name}-{state}"), last.expect("captured")));
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let shots = shots.take();
    assert_eq!(shots.len(), 6);
    for (name, frame) in shots {
        crate::look::save(&frame, &format!("target/tmp/look-toast-{name}.bmp"));
        // 中身（不透明な画素）が描けていること。
        let drawn = frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .count();
        assert!(drawn > 1000, "{name}: only {drawn} opaque px");
        println!("look_toast: {name} ok ({drawn} opaque px)");
    }
}
