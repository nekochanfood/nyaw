//! ストレステスト（ベンチマーク）。常には流さない: `cargo test -p ui --test stress -- --ignored`。
//! 本番と同じ `ui::run_with` に novr のエミュレーター（ウィンドウは出さないが毎フレーム合成する）を載せ、厳しめの負荷を掛けて
//! プロファイラの段階ごとの時間を局面ごとに集計する。最適化の手掛かりにするため、コストを 3 つに分けて出す:
//!
//! - レンダラー: エミュレーターの合成（`compositor`）
//! - オーバーレイ: 追従・ヒット判定・ポインタの配置・アニメーション（`overlay`）
//! - UI: 入力の配送（`input`）・GPUI のレイアウト（`draw`）・描画と読み戻し（`render`）・ランタイムへの転送（`upload`）・Web（`web`）
//!
//! 局面: アイドル → マウスで掃く（押したままのスワイプを含む）→ パネルの召喚と破棄を繰り返す → 目の前のパネル → もう一度アイドル。
//! 最初と最後のアイドルの差で「時間とともに重くなる」を、召喚の局面の最大フレーム時間で召喚の引っ掛かりを見る。
//! 1 フレームごとの CSV（`stress.csv`）と集計（`stress.txt`）を `CARGO_TARGET_TMPDIR` に書く。
//! 局面の長さは `NYAW_STRESS_SCALE`（既定 1.0）で伸縮する。`NYAW_STRESS_NO_WEB=1` で Web のパネルを外す（切り分け用）。

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

const SCREEN: (u32, u32) = (1280, 720);

/// 書いた CSV を後から読める出力先。
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
        println!("stress: スキップ（ベンチマークなので --ignored のときだけ流す）");
        return ExitCode::SUCCESS;
    }
    if !common::should_run("stress") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    // GPUI の Testing 8 枚（2 段）と Web の Testing 2 枚、本番と同じくビームも載せる。
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
    let profiler = Profiler::new(Some(Box::new(csv.clone()))).expect("メモリへは書ける");
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
        // プロファイラの窓は毎フレーム描き直すので、計測を汚さないよう出さない。
        hide_profiler: true,
    };
    ui::run_with(kernel, panels, Some(profiler), webpages::FILES, hooks);
    common::exit_code("stress", &result)
}

/// 局面の名前と、計測開始からの範囲。
struct Phase {
    name: &'static str,
    range: (Duration, Duration),
}

/// Web のパネルを載せるか（`NYAW_STRESS_NO_WEB=1` で外して GPUI だけのコストを見る）。
fn web() -> bool {
    std::env::var_os("NYAW_STRESS_NO_WEB").is_none()
}

fn scale() -> f32 {
    std::env::var("NYAW_STRESS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0)
}

/// 画面を大きく掃くマウスの位置（リサジュー曲線）。
fn sweep(frame: u32) -> Vec2 {
    let t = frame as f32 / 90.0;
    let (w, h) = (SCREEN.0 as f32, SCREEN.1 as f32);
    Vec2::new(
        w / 2.0 + w * 0.45 * (1.3 * TAU * t / 4.0).sin(),
        h / 2.0 + h * 0.45 * (1.7 * TAU * t / 4.0 + 0.5).sin(),
    )
}

/// 1 フレーム分の掃く操作。60 フレームのうち 30 フレームは押したまま（タッチではスワイプ、マウスではドラッグ）。
/// 押し始めと離す前にも大きく動いているので、タップ（クリック）にはならない。90 フレームごとにスティックでスクロールする。
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

/// `len` の間、1 フレームに 1 回 `step` を行う局面を流して記録する。
fn run_phase(
    phases: &mut Vec<Phase>,
    t0: Instant,
    name: &'static str,
    len: Duration,
    mut step: impl FnMut(u32),
) {
    let start = t0.elapsed();
    let mut frame = 0;
    // emu の操作は次のフレームで行われるのを待つので、1 回 = 約 1 フレーム。
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
    // 起動（Web の読み込みを含む）が落ち着くまで待つ。計測には入れない。
    std::thread::sleep(Duration::from_secs(3));

    run_phase(&mut phases, t0, "idle", secs(4.0), |_| emu.point(None));
    run_phase(&mut phases, t0, "sweep", secs(8.0), |f| sweep_frame(emu, f));
    // 0.5 秒ごとにパネルを開いて前のを閉じる（GPUI の Testing、4 回に 1 回は Web の Testing）。
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
    // 目の前 0.5m（画面の大半を覆う）のパネルを掃く。
    let near = next_id;
    spawn(remote, || testing_at(0.0, 0.0, -0.5));
    run_phase(&mut phases, t0, "near", secs(5.0), |f| sweep_frame(emu, f));
    close(remote, near);
    run_phase(&mut phases, t0, "idle-after", secs(4.0), |_| {
        emu.point(None)
    });
    phases
}

/// 1 フレーム分の CSV の行（必要な列だけ）。
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

/// 平均と 95 パーセンタイル（ms）。
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
        "stress（{}x{}、GPUI 8 枚 + Web 2 枚 + ビーム。時間は ms、平均 / p95）",
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
