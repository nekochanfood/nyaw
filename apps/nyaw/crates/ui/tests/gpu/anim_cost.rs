//! アニメーション中のパネルを本番のフレームループで描き、パネルごとの描画回数を測る。装飾のくり返し（スピナー・波紋・
//! 案内の輪）は上限の頻度で、遷移はフレームごとに、イントロは画像が変わるときに描き直す。表示されないウィンドウに GPUI の
//! 次フレームの通知を溜めないことも確かめる。`cargo test -p ui --test gpu -- --ignored anim_cost`

use crate::common;

use std::{
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::channel,
    },
    time::{Duration, Instant},
};

use design::{Theme, components};
use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px, rgba};
use kernel::{Config, Hand, Kernel, Pose, Vec2, VrBackend, vr::Quat, vr::Vec3};
use ui::PanelSpec;

const NAMES: [&str; 6] = ["spinner", "ripple", "ring", "every", "intro", "still"];
const SPINNER: usize = 0;
const RIPPLE: usize = 1;
const RING: usize = 2;
const EVERY: usize = 3;
const INTRO: usize = 4;
const STILL: usize = 5;

struct Probe {
    kind: usize,
    renders: Arc<[AtomicUsize; 6]>,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders[self.kind].fetch_add(1, Ordering::SeqCst);
        let theme = Theme::current(cx);
        let root = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme.background))
            .text_color(rgba(theme.neutral_text));
        match self.kind {
            SPINNER => root.child(components::spinner(&theme, 96.0)),
            RIPPLE => root.child(
                guide::zoom(guide::Model::Index, Hand::Left)
                    .size(px(400.0), px(300.0))
                    .parts(vec!["a"]),
            ),
            RING => root.child(
                div()
                    .relative()
                    .w(px(160.0))
                    .h(px(64.0))
                    .children(design::spot::mark("anim-cost-spot", px(8.0), cx)),
            ),
            EVERY => {
                design::drive::keep_drawing(window, cx);
                let x = self.renders[EVERY].load(Ordering::SeqCst) % 300;
                root.child(
                    div()
                        .absolute()
                        .left(px(x as f32))
                        .size(px(64.0))
                        .bg(rgba(theme.accent_solid)),
                )
            }
            INTRO => root.child(design::intro_logo("anim-cost-intro").height(px(120.0))),
            _ => root.child("still"),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Count {
    renders: usize,
    submits: usize,
}

pub fn main() -> ExitCode {
    if !std::env::args().any(|a| a == "--ignored" || a == "--include-ignored") {
        println!("anim_cost: skipped (benchmark, so only run with --ignored)");
        return ExitCode::SUCCESS;
    }
    if !common::should_run("anim_cost") {
        return ExitCode::SUCCESS;
    }
    let mock = common::SharedMock::default();
    let mut config = Config::default();
    config.kernel.lod_quality = kernel::LodQuality::Off;
    let kernel = Kernel::with_backend(Box::new(mock.clone()) as Box<dyn VrBackend>, config);
    let renders: Arc<[AtomicUsize; 6]> = Arc::default();
    let panels: Vec<PanelSpec> = (0..NAMES.len())
        .map(|kind| {
            let renders = renders.clone();
            let (col, row) = ((kind % 3) as f32, (kind / 3) as f32);
            let pose = Pose::new(
                Vec3::new((col - 1.0) * 0.34, 0.15 - row * 0.3, -1.0),
                Quat::IDENTITY,
            );
            PanelSpec::root(pose, move |_, _, _| Probe { kind, renders })
                .sized(Vec2::new(0.3, 0.225), (400, 300))
                .without_animation()
                .without_nav_bar()
        })
        .collect();

    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scenario(&mock, &renders, &remote)
        }))
        .is_ok();
        println!("anim_cost: {}", if ok { "ok" } else { "FAILED" });
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
    common::exit_code("anim_cost", &result)
}

fn scenario(
    mock: &common::SharedMock,
    renders: &Arc<[AtomicUsize; 6]>,
    remote: &std::sync::mpsc::Sender<ui::Remote>,
) {
    // 案内の輪を出すため、そのパネルだけを焦点にする。ほかはそのまま。
    let _ = remote.send(Box::new(|cx| {
        ui::tour::set(
            Some(ui::tour::Focus {
                free: vec![SPINNER, RIPPLE, EVERY, INTRO, STILL],
                lit: vec![RING],
                spots: vec!["anim-cost-spot".into()],
                ..Default::default()
            }),
            cx,
        )
    }));
    let snapshot = || -> [Count; 6] {
        let m = mock.0.lock().unwrap();
        std::array::from_fn(|i| Count {
            renders: renders[i].load(Ordering::SeqCst),
            submits: m.overlays.get(i).map_or(0, |o| o.pixel_submissions),
        })
    };
    let t0 = Instant::now();
    let sleep_until = |s: f32| {
        std::thread::sleep(Duration::from_secs_f32(s).saturating_sub(t0.elapsed()));
    };
    // イントロ（3 秒）の読み込みと再生を含む区間。
    sleep_until(4.5);
    let intro = snapshot();
    sleep_until(5.0);
    let from = snapshot();
    let started = Instant::now();
    sleep_until(12.2);
    let to = snapshot();
    let secs = started.elapsed().as_secs_f32();

    // 表示されないウィンドウに溜まった、次フレームの通知の数。
    let (tx, pending) = channel();
    let _ = remote.send(Box::new(move |cx| {
        let counts: Vec<usize> = cx
            .windows()
            .into_iter()
            .filter_map(|w| {
                cx.update_window(w, |_, window, cx| window.simulate_next_frame(cx))
                    .ok()
            })
            .collect();
        let _ = tx.send(counts.iter().sum::<usize>());
    }));
    let pending = pending
        .recv_timeout(common::TIMEOUT)
        .expect("the frame loop answers");

    println!(
        "intro (0..4.5s): renders {}  submits {}",
        intro[INTRO].renders, intro[INTRO].submits
    );
    println!("steady ({secs:.1}s)   renders/s  submits/s");
    let rate: [(f32, f32); 6] = std::array::from_fn(|i| {
        (
            (to[i].renders - from[i].renders) as f32 / secs,
            (to[i].submits - from[i].submits) as f32 / secs,
        )
    });
    for (name, (r, s)) in NAMES.iter().zip(rate) {
        println!("  {name:8} {r:9.1} {s:10.1}");
    }
    println!("pending next-frame callbacks: {pending}");

    // 通知は溜まらない。
    assert!(pending < 20, "next-frame callbacks pile up: {pending}");
    // 遷移はフレームごと（このモックは約 80〜90Hz）。
    assert!(rate[EVERY].0 > 60.0, "transitions run every frame");
    // 装飾は上限（45Hz）の付近まで。止まらない。
    for i in [SPINNER, RING] {
        assert!(
            (25.0..=47.0).contains(&rate[i].0),
            "{}: {}",
            NAMES[i],
            rate[i].0
        );
        assert!(rate[i].1 > 15.0, "{} keeps moving: {}", NAMES[i], rate[i].1);
    }
    // 波紋は 1.8 秒のうち 1.26 秒だけ出る。合間は描かない。
    assert!(
        (15.0..=35.0).contains(&rate[RIPPLE].0),
        "ripple: {}",
        rate[RIPPLE].0
    );
    assert!(
        rate[RIPPLE].1 > 10.0,
        "ripple keeps moving: {}",
        rate[RIPPLE].1
    );
    // イントロは動き、描き直しは画像の更新とほぼ同じ回数で、終わったら止まる。
    assert!(
        intro[INTRO].submits > 40,
        "the intro plays: {}",
        intro[INTRO].submits
    );
    assert!(
        intro[INTRO].renders <= intro[INTRO].submits * 3 / 2 + 10,
        "the intro redraws only for new images"
    );
    assert!(rate[INTRO].0 < 6.0, "the intro stops: {}", rate[INTRO].0);
    assert!(
        rate[STILL].0 < 6.0,
        "idle panels stay idle: {}",
        rate[STILL].0
    );
}
