//! Move the camera to switch production GPUI / WebView rendering through LOD0–3–0.
mod common;
use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use kernel::{
    Config, Kernel, PointerMode, Pose, VrBackend,
    vr::{Quat, Vec3},
};
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
        PanelSpec::web(
            Pose::new(common::PANEL_POS, Quat::IDENTITY),
            webpages::TESTING,
        )
        .without_animation(),
        PanelSpec::new(
            Pose::new(Vec3::new(0.8, 0.0, -1.0), Quat::IDENTITY),
            move |_, _, _| Probe(clicked),
        )
        .without_animation(),
    ];
    common::run(
        "panel_lod",
        kernel,
        panels,
        "panel-0",
        None,
        move |page, _| {
            let mut vr = common::Vr::new(mock.clone());
            for (index, (z, lod)) in [(0.0, 0), (2.0, 1), (4.0, 2), (8.0, 3), (0.0, 0)]
                .into_iter()
                .enumerate()
            {
                let lod = if off { 0 } else { lod };
                mock.0.lock().unwrap().frame.hmd.position.z = z;
                let expected = (800 >> lod, 600 >> lod);
                wait(
                    &format!("LOD{lod}: both textures must be {expected:?}"),
                    || {
                        vr.texture_size_of(0) == Some(expected)
                            && vr.texture_size_of(1) == Some(expected)
                    },
                );
                page.until("Web logical viewport is unchanged", |s| {
                    s["viewport"] == serde_json::json!([800, 600])
                });
                {
                    let m = mock.0.lock().unwrap();
                    for p in &m.overlays[..2] {
                        assert!((p.width_m - 0.8).abs() < 1e-5);
                    }
                    let tex = m.overlays[1].texture.as_ref().unwrap();
                    assert_eq!(
                        tex.pixel(632 >> lod, 432 >> lod),
                        [0, 255, 0, 255],
                        "GPUI layout is unchanged"
                    );
                }
                vr.panel = common::PANEL_POS;
                vr.point_at(page.center("click-me"));
                let before = common::num(&page.probe(), &["state", "clicks"]);
                vr.set("trigger/value", 1.0);
                std::thread::sleep(Duration::from_millis(100));
                vr.set("trigger/value", 0.0);
                page.until("Web remains clickable after LOD changes", |s| {
                    common::num(s, &["state", "clicks"]) == before + 1.0
                });
                vr.panel = Vec3::new(0.8, 0.0, -1.0);
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
        },
    )
}
