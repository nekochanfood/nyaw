//! Layer 5 横: アプリケーションのプロファイラ。`--profile` で起動したときだけ動き、普段は存在しない。
//! UI 層のフレームループが段階ごとの所要時間を [`Profiler::record`] に渡し、ここで
//! 1. 直近 [`CAPACITY`] フレームを保持して OS のウィンドウ（[`ProfilerView`]）にグラフと数値で出し、
//! 2. 1 フレーム 1 行の CSV（[`CSV_HEADER`]）にダンプする（pandas / Excel / gnuplot 等でそのまま読める）。
//!
//! CPU / GPU の分け方（GPUI は GPU タイマーを公開していないので、同期して待つ段階の実時間で測る）:
//! - CPU = `input`（入力の配送とパネル操作の反映）+ `draw`（GPUI のレイアウトとシーン構築）+ `overlay`（オーバーレイ層）
//!   + `web`（Web のパネル）
//! - GPU = `render`（GPU 描画と CPU への読み戻し）+ `upload`（VR ランタイムへのテクスチャ転送）
//!   + `compositor`（novr のエミュレーターの合成と表示）
//! - `wait` = 次のフレームまでの待ち（VR は vsync、novr はエミュレーターの周期）。アイドル時間。

use std::{
    collections::VecDeque,
    io::{self, Write},
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Context, Hsla, IntoElement, Pixels, Window, canvas, div, fill, point, prelude::*,
    px, rgb, size,
};

/// 保持してグラフに出すフレーム数。
pub const CAPACITY: usize = 900;
/// グラフの 1 列の幅（px）。この幅に入るフレームは平均してまとめて描く。
const COLUMN_PX: f32 = 2.0;
/// 数値（平均・p99 など）を出す直近のフレーム数。
pub const WINDOW: usize = 90;
/// 予算（1 フレームの目標時間）のこの倍を超えたフレームをドロップとみなす。
pub const DROP_FACTOR: f64 = 1.5;
/// CSV の列。時間はすべてミリ秒。`t_ms` は計測開始からの経過。
pub const CSV_HEADER: &str = "frame,t_ms,interval_ms,wait_ms,input_ms,draw_ms,render_ms,upload_ms,profiler_ms,overlay_ms,compositor_ms,web_ms,cpu_ms,gpu_ms,budget_ms,dropped";

/// 1 フレームの段階ごとの所要時間。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stages {
    pub wait: Duration,
    pub input: Duration,
    pub draw: Duration,
    pub render: Duration,
    pub upload: Duration,
    /// プロファイラ窓自身の描画（render + paint）。待ちの間に main スレッドで走るので wait からは差し引く。
    pub profiler: Duration,
    /// オーバーレイ層（追従・ヒット判定・ポインタの配置・アニメーション）。
    pub overlay: Duration,
    /// ランタイムがこのスレッドで合成・表示にかけた時間（novr のエミュレーター。VR ランタイムは 0）。
    pub compositor: Duration,
    /// Web のパネル（Tauri を回す・ポインタを送る・WebView を撮る）。
    pub web: Duration,
}

impl Stages {
    pub fn cpu(&self) -> Duration {
        self.input + self.draw + self.profiler + self.overlay + self.web
    }

    pub fn gpu(&self) -> Duration {
        self.render + self.upload + self.compositor
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub frame: u64,
    /// 計測開始からの経過。
    pub at: Duration,
    /// 前のフレームからの間隔（= フレーム時間）。
    pub interval: Duration,
    pub stages: Stages,
    pub budget: Duration,
    pub dropped: bool,
}

impl Sample {
    pub fn csv_row(&self) -> String {
        let ms = |d: Duration| format!("{:.3}", d.as_secs_f64() * 1e3);
        let s = &self.stages;
        [
            self.frame.to_string(),
            ms(self.at),
            ms(self.interval),
            ms(s.wait),
            ms(s.input),
            ms(s.draw),
            ms(s.render),
            ms(s.upload),
            ms(s.profiler),
            ms(s.overlay),
            ms(s.compositor),
            ms(s.web),
            ms(s.cpu()),
            ms(s.gpu()),
            ms(self.budget),
            (self.dropped as u8).to_string(),
        ]
        .join(",")
    }
}

/// 直近 [`WINDOW`] フレームの集計。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub fps: f64,
    pub frame_avg: Duration,
    pub frame_p99: Duration,
    pub frame_max: Duration,
    pub cpu_avg: Duration,
    pub gpu_avg: Duration,
    pub stages_avg: Stages,
    pub budget: Duration,
    /// 起動からの総フレーム数と総ドロップ数。
    pub frames: u64,
    pub dropped: u64,
}

pub struct Profiler {
    start: Instant,
    last: Option<Instant>,
    /// 既知の予算（novr のタイマー周期）。`None` なら間隔から推定する（VR は vsync で決まる）。
    budget: Option<Duration>,
    samples: VecDeque<Sample>,
    frames: u64,
    dropped: u64,
    /// 前回の `record` 以降にプロファイラ窓の描画にかかった時間。
    self_time: Duration,
    out: Option<Box<dyn Write>>,
}

impl gpui::Global for Profiler {}

impl Profiler {
    /// `out` があれば CSV の見出しを書き、以降 1 フレーム 1 行ずつ書く。予算は間隔から推定する。
    pub fn new(out: Option<Box<dyn Write>>) -> io::Result<Self> {
        let mut out = out;
        if let Some(w) = out.as_mut() {
            writeln!(w, "{CSV_HEADER}")?;
        }
        let now = Instant::now();
        Ok(Self {
            start: now,
            last: None,
            budget: None,
            samples: VecDeque::with_capacity(CAPACITY),
            frames: 0,
            dropped: 0,
            self_time: Duration::ZERO,
            out,
        })
    }

    /// 予算（1 フレームの目標時間）が分かっているとき（novr のタイマー周期）に指定する。
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = Some(budget);
        self
    }

    /// フレームの終わり（`now`）に呼ぶ。最初の 1 回は間隔が分からないので段階の合計を間隔とする。
    /// プロファイラ窓の描画時間を足す。次の `record` で `Stages::profiler` に入り、wait から差し引かれる。
    pub fn add_self_time(&mut self, d: Duration) {
        self.self_time += d;
    }

    pub fn record(&mut self, now: Instant, mut stages: Stages) {
        stages.profiler = std::mem::take(&mut self.self_time);
        stages.wait = stages.wait.saturating_sub(stages.profiler);
        let interval = match self.last.replace(now) {
            Some(prev) => now - prev,
            None => stages.wait + stages.cpu() + stages.gpu(),
        };
        let budget = self.budget();
        let dropped = interval.as_secs_f64() > budget.as_secs_f64() * DROP_FACTOR;
        let sample = Sample {
            frame: self.frames,
            at: now - self.start,
            interval,
            stages,
            budget,
            dropped,
        };
        self.frames += 1;
        self.dropped += dropped as u64;
        if self.samples.len() == CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        if let Some(w) = self.out.as_mut() {
            // 落ちても残るよう毎フレーム書き切る。書けなくなったらダンプだけ止める（計測と表示は続ける）。
            if writeln!(w, "{}", sample.csv_row())
                .and_then(|_| w.flush())
                .is_err()
            {
                self.out = None;
            }
        }
    }

    /// 1 フレームの目標時間。既知でなければ保持中の間隔の下位 10% 点（= vsync 周期）で推定する。
    // ponytail: ずっと vsync に間に合わないと推定が伸びてドロップを数え損ねる。困ったら VrBackend に表示周波数を足す。
    pub fn budget(&self) -> Duration {
        self.budget.unwrap_or_else(|| {
            let v: Vec<Duration> = self.samples.iter().map(|s| s.interval).collect();
            percentile(v, 0.10).unwrap_or(Duration::from_millis(11))
        })
    }

    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }

    pub fn summary(&self) -> Summary {
        let recent: Vec<&Sample> = self.samples.iter().rev().take(WINDOW).collect();
        let n = recent.len().max(1) as u32;
        let sum = |f: fn(&Sample) -> Duration| recent.iter().map(|s| f(s)).sum::<Duration>() / n;
        let frame_avg = sum(|s| s.interval);
        let intervals: Vec<Duration> = recent.iter().map(|s| s.interval).collect();
        Summary {
            fps: if frame_avg.is_zero() {
                0.0
            } else {
                1.0 / frame_avg.as_secs_f64()
            },
            frame_avg,
            frame_p99: percentile(intervals.clone(), 0.99).unwrap_or_default(),
            frame_max: intervals.into_iter().max().unwrap_or_default(),
            cpu_avg: sum(|s| s.stages.cpu()),
            gpu_avg: sum(|s| s.stages.gpu()),
            stages_avg: Stages {
                wait: sum(|s| s.stages.wait),
                input: sum(|s| s.stages.input),
                draw: sum(|s| s.stages.draw),
                render: sum(|s| s.stages.render),
                upload: sum(|s| s.stages.upload),
                profiler: sum(|s| s.stages.profiler),
                overlay: sum(|s| s.stages.overlay),
                compositor: sum(|s| s.stages.compositor),
                web: sum(|s| s.stages.web),
            },
            budget: self.budget(),
            frames: self.frames,
            dropped: self.dropped,
        }
    }
}

/// 最近傍順位法の百分位点。
fn percentile(mut v: Vec<Duration>, p: f64) -> Option<Duration> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let i = ((v.len() as f64 * p).ceil() as usize).clamp(1, v.len()) - 1;
    Some(v[i])
}

// ---- 表示 ----

const BG: u32 = 0x17181c;
const PLOT_BG: u32 = 0x202228;
const GRID: u32 = 0x30333b;
const TEXT: u32 = 0xd8dae0;
const DIM: u32 = 0x8a8f9c;
const BUDGET: u32 = 0xf2b134;
const DROP: u32 = 0xe5484d;

/// グラフに積む 1 段。
struct Layer {
    name: &'static str,
    color: u32,
    value: fn(&Stages) -> Duration,
}

const CPU_LAYERS: &[Layer] = &[
    Layer {
        name: "Input",
        color: 0x4c8bf5,
        value: |s| s.input,
    },
    Layer {
        name: "Draw (layout)",
        color: 0x9b6cf0,
        value: |s| s.draw,
    },
    Layer {
        name: "Overlay",
        color: 0xd4b106,
        value: |s| s.overlay,
    },
    Layer {
        name: "Web",
        color: 0xe0559b,
        value: |s| s.web,
    },
    Layer {
        name: "Profiler (self)",
        color: 0xe07b39,
        value: |s| s.profiler,
    },
    Layer {
        name: "Wait (idle)",
        color: 0x3a4150,
        value: |s| s.wait,
    },
];

const GPU_LAYERS: &[Layer] = &[
    Layer {
        name: "Render + readback",
        color: 0x2fb487,
        value: |s| s.render,
    },
    Layer {
        name: "Upload (VR)",
        color: 0x1d8fa8,
        value: |s| s.upload,
    },
    Layer {
        name: "Compositor (novr)",
        color: 0x7fb800,
        value: |s| s.compositor,
    },
];

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1e3)
}

/// プロファイラのウィンドウ。フレームループが毎フレーム描き直しを頼む。
#[derive(Default)]
pub struct ProfilerView;

impl Render for ProfilerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Instant::now();
        let Some(p) = cx.try_global::<Profiler>() else {
            return div().size_full().bg(rgb(BG));
        };
        let s = p.summary();
        let samples: Vec<Sample> = p.samples().iter().copied().collect();
        let stat = |label: &'static str, value: String, color: u32| {
            div()
                .flex()
                .flex_col()
                .child(div().text_xs().text_color(rgb(DIM)).child(label))
                .child(div().text_color(rgb(color)).child(value))
        };
        let drop_pct = if s.frames == 0 {
            0.0
        } else {
            s.dropped as f64 * 100.0 / s.frames as f64
        };
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(
                div()
                    .flex()
                    .gap_6()
                    .child(stat("FPS", format!("{:.1}", s.fps), TEXT))
                    .child(stat("Frame avg", ms(s.frame_avg), TEXT))
                    .child(stat("Frame p99", ms(s.frame_p99), TEXT))
                    .child(stat("Frame max", ms(s.frame_max), TEXT))
                    .child(stat("CPU avg", ms(s.cpu_avg), CPU_LAYERS[0].color))
                    .child(stat("GPU avg", ms(s.gpu_avg), GPU_LAYERS[0].color))
                    .child(stat("Budget", ms(s.budget), BUDGET))
                    .child(stat(
                        "Dropped",
                        format!("{} / {} ({drop_pct:.1}%)", s.dropped, s.frames),
                        if s.dropped > 0 { DROP } else { TEXT },
                    )),
            )
            .child(chart("CPU", &samples, s.budget, CPU_LAYERS, &s.stages_avg))
            .child(chart("GPU", &samples, s.budget, GPU_LAYERS, &s.stages_avg))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(DIM))
                    .child(format!(
                        "last {CAPACITY} frames · stats over last {WINDOW} · drop = interval > {DROP_FACTOR}× budget"
                    )),
            );
        cx.global_mut::<Profiler>().add_self_time(t.elapsed());
        root
    }
}

/// 段を積み上げた棒グラフ（1 フレーム 1 本、右端が最新）と凡例。縦軸は予算の 2 倍まで。
fn chart(
    title: &'static str,
    samples: &[Sample],
    budget: Duration,
    layers: &'static [Layer],
    avg: &Stages,
) -> impl IntoElement {
    let top = (budget.as_secs_f64() * 2e3).ceil().max(1.0);
    let axis = |v: f64| {
        div()
            .text_xs()
            .text_color(rgb(DIM))
            .child(format!("{v:.0}"))
    };
    let samples = samples.to_vec();
    let plot = canvas(
        |_, _, _| {},
        move |b: Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
            let t = Instant::now();
            window.paint_quad(fill(b, rgb(PLOT_BG)));
            let h = f32::from(b.size.height);
            let y_of = |v_ms: f64| b.bottom() - px((v_ms / top).min(1.0) as f32 * h);
            let hline = |window: &mut Window, y: Pixels, color: Hsla| {
                window.paint_quad(fill(
                    Bounds::new(point(b.left(), y), size(b.size.width, px(1.0))),
                    color,
                ))
            };
            for i in 1..4 {
                hline(window, y_of(top * i as f64 / 4.0), rgb(GRID).into());
            }
            // 1 フレーム 1 本だと幅 1px 未満の四角が数千個になり、描くだけで重い。COLUMN_PX ごとの列に平均してまとめる
            // （ドロップは列の中に 1 つでもあれば印を付ける）。新しいものが右端。
            let columns = (f32::from(b.size.width) / COLUMN_PX).floor().max(1.0) as usize;
            let per = CAPACITY.div_ceil(columns).max(1);
            let w = b.size.width / columns as f32;
            for (c, chunk) in samples.rchunks(per).enumerate() {
                let x = b.right() - w * (c + 1) as f32;
                let k = chunk.len() as f64;
                let mut acc = 0.0;
                for l in layers {
                    let v = chunk
                        .iter()
                        .map(|s| (l.value)(&s.stages).as_secs_f64())
                        .sum::<f64>()
                        * 1e3
                        / k;
                    let (y0, y1) = (y_of(acc), y_of(acc + v));
                    acc += v;
                    if y0 > y1 {
                        window.paint_quad(fill(
                            Bounds::new(point(x, y1), size(w, y0 - y1)),
                            rgb(l.color),
                        ));
                    }
                }
                if chunk.iter().any(|s| s.dropped) {
                    window.paint_quad(fill(
                        Bounds::new(point(x, b.top()), size(w, px(4.0))),
                        rgb(DROP),
                    ));
                }
            }
            hline(window, y_of(budget.as_secs_f64() * 1e3), rgb(BUDGET).into());
            cx.global_mut::<Profiler>().add_self_time(t.elapsed());
        },
    )
    .flex_1()
    .h_full();
    div()
        .flex()
        .flex_1()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_col()
                .justify_between()
                .w(px(28.0))
                .items_end()
                .child(axis(top))
                .child(axis(top / 2.0))
                .child(axis(0.0)),
        )
        .child(plot)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(190.0))
                .child(div().font_weight(gpui::FontWeight::BOLD).child(title))
                .children(layers.iter().map(|l| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size(px(10.0)).bg(rgb(l.color)))
                        .child(div().flex_1().child(l.name))
                        .child(div().text_color(rgb(DIM)).child(ms((l.value)(avg))))
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(10.0)).h(px(2.0)).bg(rgb(BUDGET)))
                        .child("Budget"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(10.0)).h(px(4.0)).bg(rgb(DROP)))
                        .child("Dropped frame"),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const MS: Duration = Duration::from_millis(1);

    /// テスト用: 書いた内容を後から読める出力先。
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

    fn stages(wait_ms: u64, input_ms: u64, render_ms: u64) -> Stages {
        Stages {
            wait: MS * wait_ms as u32,
            input: MS * input_ms as u32,
            render: MS * render_ms as u32,
            ..Default::default()
        }
    }

    #[test]
    fn counts_drops_against_budget_and_dumps_csv() {
        let out = Shared::default();
        let mut p = Profiler::new(Some(Box::new(out.clone())))
            .unwrap()
            .with_budget(10 * MS);
        for at in [10, 20, 36, 50, 80] {
            p.record(p.start + MS * at, stages(8, 1, 1));
        }
        // 間隔: 最初は段階の合計 10、以降 10, 16, 14, 30 → 15ms を超えるのは 16 と 30
        let s = p.summary();
        assert_eq!((s.frames, s.dropped), (5, 2));
        assert_eq!(s.frame_max, 30 * MS);
        assert_eq!((s.cpu_avg, s.gpu_avg), (MS, MS));
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(
            lines[3],
            "2,36.000,16.000,8.000,1.000,0.000,1.000,0.000,0.000,0.000,0.000,0.000,1.000,1.000,10.000,1"
        );
        assert_eq!(lines[0].split(',').count(), lines[3].split(',').count());
    }

    #[test]
    fn estimates_budget_from_vsync_intervals() {
        let mut p = Profiler::new(None).unwrap();
        let mut t = Instant::now();
        for i in 0..100 {
            // ほとんど 11ms、たまに 2 周期分かかる
            t += if i % 10 == 9 { 22 * MS } else { 11 * MS };
            p.record(t, Stages::default());
        }
        assert_eq!(p.budget(), 11 * MS);
        assert!(p.summary().dropped >= 9);
    }

    #[test]
    fn keeps_only_capacity_samples() {
        let mut p = Profiler::new(None).unwrap().with_budget(MS);
        let mut t = Instant::now();
        for _ in 0..CAPACITY + 5 {
            t += MS;
            p.record(t, Stages::default());
        }
        assert_eq!(p.samples().len(), CAPACITY);
        assert_eq!(p.samples()[0].frame, 5);
    }

    #[gpui::test]
    fn view_renders_with_samples(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut p = Profiler::new(None).unwrap().with_budget(10 * MS);
            let mut t = Instant::now();
            for _ in 0..10 {
                t += 12 * MS;
                p.record(t, stages(9, 1, 2));
            }
            cx.set_global(p);
        });
        let (_view, cx) = cx.add_window_view(|_, _| ProfilerView);
        cx.run_until_parked();
    }
}
