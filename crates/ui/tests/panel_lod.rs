//! 本番の GPUI 描画で LOD0〜3〜0 の解像度・レイアウト・入力を検証する。
mod common;
use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use kernel::{Config, Kernel, PointerMode, Pose, VrBackend, vr::Quat};
use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use ui::PanelSpec;

struct Probe(Arc<AtomicUsize>);
impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x204060)).child(
            div()
                .id("lod-button")
                .absolute()
                .left(px(600.))
                .top(px(400.))
                .size(px(64.))
                .bg(rgb(0x00ff00))
                .on_click(cx.listener(|this, _, _, _| {
                    this.0.fetch_add(1, Ordering::SeqCst);
                })),
        )
    }
}

fn wait(what: &str, mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + common::TIMEOUT;
    while !ready() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn main() -> ExitCode {
    if !cfg!(windows) || !common::should_run("panel_lod") {
        return ExitCode::SUCCESS;
    }
    let mock = common::SharedMock::default();
    let off = std::env::args().any(|a| a == "--off");
    let clicks = Arc::new(AtomicUsize::new(0));
    let clicked = clicks.clone();
    let config = Config {
        lod_quality: if off {
            kernel::LodQuality::Off
        } else {
            kernel::LodQuality::Mid
        },
        pointer_smoothing: Some(0.0),
        pointer_mode: Some(PointerMode::Mouse),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(mock.clone()) as Box<dyn VrBackend>, config);
    let panels = vec![
        PanelSpec::new(
            Pose::new(common::PANEL_POS, Quat::IDENTITY),
            move |_, _, _| Probe(clicked),
        )
        .without_animation(),
    ];
    let (remote, rx) = std::sync::mpsc::channel::<ui::Remote>();
    let result: Arc<std::sync::Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut vr = common::Vr::new(mock.clone());
            for (index, (z, lod)) in [(0.0, 0), (2.0, 1), (4.0, 2), (8.0, 3), (0.0, 0)]
                .into_iter()
                .enumerate()
            {
                let lod = if off { 0 } else { lod };
                mock.0.lock().unwrap().frame.hmd.position.z = z;
                let expected = (800 >> lod, 600 >> lod);
                wait(&format!("LOD{lod}: texture must be {expected:?}"), || {
                    vr.texture_size_of(0) == Some(expected)
                });
                {
                    let m = mock.0.lock().unwrap();
                    assert!((m.overlays[0].width_m - 0.8).abs() < 1e-5);
                    let tex = m.overlays[0].texture.as_ref().unwrap();
                    assert_eq!(
                        tex.pixel(632 >> lod, 432 >> lod),
                        [0, 255, 0, 255],
                        "GPUI layout is unchanged"
                    );
                }
                vr.point_at((632., 432.));
                std::thread::sleep(Duration::from_millis(100));
                vr.set("trigger/value", 1.0);
                std::thread::sleep(Duration::from_millis(100));
                vr.set("trigger/value", 0.0);
                wait("GPUI remains clickable after LOD changes", || {
                    clicks.load(Ordering::SeqCst) == index + 1
                });
                println!("panel_lod: LOD{lod} {expected:?}, viewport/input OK");
            }
        }))
        .is_ok();
        println!("panel_lod: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    ui::run_with(
        kernel,
        panels,
        None,
        ui::Hooks {
            remote: Some(rx),
            ..Default::default()
        },
    );
    common::exit_code("panel_lod", &result)
}
