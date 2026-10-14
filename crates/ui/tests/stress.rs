//! Stress test (benchmark). Not run by default: `cargo test -p ui --test stress -- --ignored`。
//! Runs the novr emulator (no window, but composites every frame) on the production `ui::run_with` under heavy load and
//! aggregates the profiler's per-stage times per phase. As a lead for optimization, the cost is split into three:
//!
//! - renderer: the emulator's compositing (`compositor`)
//! - overlay: following, hit testing, pointer placement, animation (`overlay`)
//! - UI: input delivery (`input`), GPUI layout (`draw`), render and readback (`render`), transfer to the runtime (`upload`), web (`web`)
//!
//! Phases: idle -> sweep with the mouse (including pressed swipes) -> idle -> repeated panel summon and destroy -> panel in front of the eyes
//! -> volume (summon 10 stacked panels like Toggle Panel and sweep) -> idle again.
//! The difference between idles shows "getting heavier over time" (after sweeping = idle-mid, after summon and destroy = idle-after), and the max frame time of the summon phase shows summon hitches.
//! Writes a per-frame CSV (`stress.csv`) and a summary (`stress.txt`) to `CARGO_TARGET_TMPDIR`.
//! Phase length scales with `NYAW_STRESS_SCALE` (default 1.0). `NYAW_STRESS_WINDOW=1` shows windows as in production and
//! composites (on GPU if available; by default no window and CPU compositing). `NYAW_STRESS_NO_WEB=1` removes the web panels and `NYAW_STRESS_PROFILER=1` shows the profiler window (for isolating causes).

mod common;

use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Driven, Emu};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, PanelCommand, PanelId, Pose, Vec2, VrBackend};
use profiler::Profiler;
use testing::Testing;
use ui::{PanelControl, PanelSpec};

const SCREEN: (u32, u32) = (1920, 1080);
/// number of panels summoned stacked in the volume phase.
const VOLUME: usize = 10;

/// an output sink whose written CSV can be read back later.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn testing_at(x: f32, y: f32, z: f32) -> PanelSpec {
    PanelSpec::new(Pose::new(Vec3::new(x, y, z), Quat::IDENTITY), |id, _, _| {
        Testing::new(id)
    })
}

fn main() -> ExitCode {
    if !std::env::args().any(|a| a == "--ignored" || a == "--include-ignored") {
        println!("stress: skipped (benchmark, so only run with --ignored)");
        return ExitCode::SUCCESS;
    }
    if !common::should_run("stress") {
        return ExitCode::SUCCESS;
    }
    // with NYAW_STRESS_WINDOW=1, show windows and composite as in production (on GPU if available).
    let (backend, emu) = if std::env::var_os("NYAW_STRESS_WINDOW").is_some() {
        Driven::scripted_window(SCREEN.0, SCREEN.1)
    } else {
        Driven::headless(SCREEN.0, SCREEN.1)
    };
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    // 8 GPUI Testing panels (2 rows) and 2 web Testing panels, plus the beam as in production.
    let mut panels: Vec<PanelSpec> = (0..8)
        .map(|i| {
            let (col, row) = ((i % 4) as f32, (i / 4) as f32);
            testing_at(-1.5 + col, 0.7 - 0.7 * row, -2.2)
        })
        .collect();
    for x in [-0.5, 0.5].into_iter().filter(|_| web()) {
        panels.push(PanelSpec::web(
            Pose::new(Vec3::new(x, -0.75, -2.2), Quat::IDENTITY),
            webpages::TESTING,
        ));
    }
    panels.extend(beam::specs());
    let opened = panels.len();

    let csv = Shared::default();
    let profiler = Profiler::new(Some(Box::new(csv.clone()))).expect("can write to memory");
    let t0 = Instant::now();
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let phases = scenario(&emu, &remote, opened, t0);
        let report = report(&csv, &phases);
        println!("{report}");
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
        let _ = std::fs::write(dir.join("stress.csv"), csv.0.lock().unwrap().as_slice());
        let _ = std::fs::write(dir.join("stress.txt"), &report);
        println!("stress: ok（{}）", dir.join("stress.txt").display());
        *done.lock().unwrap() = Some(true);
        common::quit(&remote, true);
    });
    let hooks = ui::Hooks {
        on_web: None,
        remote: Some(rx),
        // the profiler window is redrawn every frame, so it is not shown to keep measurements clean (NYAW_STRESS_PROFILER=1 shows it to see its cost).
        hide_profiler: std::env::var_os("NYAW_STRESS_PROFILER").is_none(),
    };
    ui::run_with(kernel, panels, Some(profiler), webpages::FILES, hooks);
    common::exit_code("stress", &result)
}

/// phase name and its range from the start of measurement.
struct Phase {
    name: &'static str,
    range: (Duration, Duration),
}

/// whether to include web panels (`NYAW_STRESS_NO_WEB=1` removes them to see the GPUI-only cost).
fn web() -> bool {
    std::env::var_os("NYAW_STRESS_NO_WEB").is_none()
}

fn scale() -> f32 {
    std::env::var("NYAW_STRESS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0)
}

/// mouse position sweeping the screen widely (Lissajous curve).
fn sweep(frame: u32) -> Vec2 {
    let t = frame as f32 / 90.0;
    let (w, h) = (SCREEN.0 as f32, SCREEN.1 as f32);
    Vec2::new(
        w / 2.0 + w * 0.45 * (1.3 * TAU * t / 4.0).sin(),
        h / 2.0 + h * 0.45 * (1.7 * TAU * t / 4.0 + 0.5).sin(),
    )
}

/// one frame of the sweep. 30 of every 60 frames are pressed (swipe for touch, drag for mouse).
/// It also moves a lot right after pressing and before releasing, so it never becomes a tap (click). Scrolls with the stick every 90 frames.
fn sweep_frame(emu: &Emu, frame: u32) {
    emu.point(Some(sweep(frame)));
    match frame % 60 {
        10 => emu.button(Button::Trigger, true),
        40 => emu.button(Button::Trigger, false),
        _ => {}
    }
    match frame % 90 {
        70 => emu.stick(Vec2::new(0.0, -1.0)),
        76 => emu.stick(Vec2::ZERO),
        _ => {}
    }
}

fn send(remote: &Sender<ui::Remote>, f: impl FnOnce(&mut gpui::App) + Send + 'static) {
    let _ = remote.send(Box::new(f));
}

fn spawn(remote: &Sender<ui::Remote>, spec: impl FnOnce() -> PanelSpec + Send + 'static) {
    send(remote, move |cx| {
        cx.default_global::<PanelControl>().spawns.push(spec())
    });
}

fn close(remote: &Sender<ui::Remote>, panel: PanelId) {
    send(remote, move |cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .commands
            .push(PanelCommand::Close)
    });
}

/// run a phase that calls `step` once per frame for `len` and record it.
fn run_phase(
    phases: &mut Vec<Phase>,
    t0: Instant,
    name: &'static str,
    len: Duration,
    mut step: impl FnMut(u32),
) {
    let start = t0.elapsed();
    let mut frame = 0;
    // emu operations wait for the next frame, so one call is about one frame.
    while t0.elapsed() - start < len {
        step(frame);
        frame += 1;
    }
    phases.push(Phase {
        name,
        range: (start, t0.elapsed()),
    });
}

fn scenario(emu: &Emu, remote: &Sender<ui::Remote>, opened: usize, t0: Instant) -> Vec<Phase> {
    let s = scale();
    let secs = |x: f32| Duration::from_secs_f32(x * s);
    let mut phases = Vec::new();
    let mut next_id = opened;
    // wait for startup (including web loading) to settle. Not included in measurement.
    std::thread::sleep(Duration::from_secs(3));

    run_phase(&mut phases, t0, "idle", secs(4.0), |_| emu.point(None));
    run_phase(&mut phases, t0, "sweep", secs(8.0), |f| sweep_frame(emu, f));
    // check whether continued operation itself made things heavier (compare with the first idle).
    run_phase(&mut phases, t0, "idle-mid", secs(4.0), |_| emu.point(None));
    // every 0.5 s open a panel and close the previous one (GPUI Testing, with web Testing every 4th time).
    run_phase(&mut phases, t0, "spawn", secs(8.0), |f| {
        sweep_frame(emu, f);
        if f % 45 != 0 {
            return;
        }
        let n = f / 45;
        if n > 0 {
            close(remote, next_id - 1);
        }
        let (x, y) = ((n % 3) as f32 * 0.4 - 0.4, 0.1);
        if n % 4 == 3 && web() {
            spawn(remote, move || {
                PanelSpec::web(
                    Pose::new(Vec3::new(x, y, -1.4), Quat::IDENTITY),
                    webpages::TESTING,
                )
            });
        } else {
            spawn(remote, move || testing_at(x, y, -1.4));
        }
        next_id += 1;
    });
    if next_id > opened {
        close(remote, next_id - 1);
    }
    // sweep a panel 0.5m in front of the eyes (covers most of the screen).
    let near = next_id;
    spawn(remote, || testing_at(0.0, 0.0, -0.5));
    run_phase(&mut phases, t0, "near", secs(5.0), |f| sweep_frame(emu, f));
    close(remote, near);
    next_id += 1;
    // volume: like Toggle Panel, summon 10 stacked Testing panels 5cm in front of the parent and sweep over them.
    let first = next_id;
    spawn(remote, || testing_at(0.0, 0.0, -1.2));
    for k in 1..VOLUME {
        let from = first + k - 1;
        spawn(remote, move || {
            PanelSpec::spawn(from, |id, _, _| Testing::new(id))
        });
    }
    next_id += VOLUME;
    run_phase(&mut phases, t0, "volume", secs(6.0), |f| {
        sweep_frame(emu, f)
    });
    for id in first..next_id {
        close(remote, id);
    }
    run_phase(&mut phases, t0, "idle-after", secs(4.0), |_| {
        emu.point(None)
    });
    phases
}

/// one frame's CSV row (only the needed columns).
struct Row {
    t: f64,
    interval: f64,
    wait: f64,
    input: f64,
    draw: f64,
    render: f64,
    upload: f64,
    overlay: f64,
    compositor: f64,
    web: f64,
    dropped: bool,
}

fn rows(csv: &Shared) -> Vec<Row> {
    let text = String::from_utf8_lossy(&csv.0.lock().unwrap()).into_owned();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |name: &str| header.iter().position(|h| *h == name).expect(name);
    let idx = [
        "t_ms",
        "interval_ms",
        "wait_ms",
        "input_ms",
        "draw_ms",
        "render_ms",
        "upload_ms",
        "overlay_ms",
        "compositor_ms",
        "web_ms",
        "dropped",
    ]
    .map(col);
    lines
        .filter_map(|l| {
            let v: Vec<f64> = l.split(',').map(|x| x.parse().unwrap_or(0.0)).collect();
            let g = |i: usize| v.get(idx[i]).copied();
            Some(Row {
                t: g(0)?,
                interval: g(1)?,
                wait: g(2)?,
                input: g(3)?,
                draw: g(4)?,
                render: g(5)?,
                upload: g(6)?,
                overlay: g(7)?,
                compositor: g(8)?,
                web: g(9)?,
                dropped: g(10)? > 0.0,
            })
        })
        .collect()
}

/// mean and 95th percentile (ms).
fn stat(mut v: Vec<f64>) -> (f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    let avg = v.iter().sum::<f64>() / v.len() as f64;
    let p95 = v[((v.len() as f64 * 0.95).ceil() as usize).clamp(1, v.len()) - 1];
    (avg, p95, *v.last().unwrap())
}

fn report(csv: &Shared, phases: &[Phase]) -> String {
    let rows = rows(csv);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "stress ({}x{}, 8 GPUI + 2 web panels + beam. times in ms, mean / p95)",
        SCREEN.0, SCREEN.1
    );
    let _ = writeln!(
        out,
        "{:<11} {:>6} {:>6} {:>6} {:>17} | {:>13} {:>13} {:>13} | {:>13} {:>13} {:>13} {:>13} {:>13} | {:>13}",
        "phase",
        "frames",
        "fps",
        "drop%",
        "frame(avg/p95/max)",
        "renderer",
        "overlay",
        "UI",
        "ui:input",
        "ui:draw",
        "ui:render",
        "ui:upload",
        "ui:web",
        "wait"
    );
    for p in phases {
        let (a, b) = (p.range.0.as_secs_f64() * 1e3, p.range.1.as_secs_f64() * 1e3);
        let r: Vec<&Row> = rows.iter().filter(|r| r.t >= a && r.t < b).collect();
        let col = |f: fn(&Row) -> f64| stat(r.iter().map(|x| f(x)).collect());
        let pair = |(avg, p95, _): (f64, f64, f64)| format!("{avg:6.2}/{p95:6.2}");
        let frame = col(|r| r.interval);
        let n = r.len().max(1) as f64;
        let drop = r.iter().filter(|r| r.dropped).count() as f64 / n * 100.0;
        let fps = if frame.0 > 0.0 { 1000.0 / frame.0 } else { 0.0 };
        let _ = writeln!(
            out,
            "{:<11} {:>6} {:>6.1} {:>6.1} {:>5.1}/{:>5.1}/{:>5.0} | {:>13} {:>13} {:>13} | {:>13} {:>13} {:>13} {:>13} {:>13} | {:>13}",
            p.name,
            r.len(),
            fps,
            drop,
            frame.0,
            frame.1,
            frame.2,
            pair(col(|r| r.compositor)),
            pair(col(|r| r.overlay)),
            pair(col(|r| r.input + r.draw + r.render + r.upload + r.web)),
            pair(col(|r| r.input)),
            pair(col(|r| r.draw)),
            pair(col(|r| r.render)),
            pair(col(|r| r.upload)),
            pair(col(|r| r.web)),
            pair(col(|r| r.wait)),
        );
    }
    out
}
