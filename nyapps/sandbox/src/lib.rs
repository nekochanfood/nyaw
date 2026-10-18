//! サンドボックス診断（net.bettaworx.sandbox）。ニャプリのサンドボックスの中で何ができ、何ができないかを
//! ホスト関数ごとに確かめて一覧にする（マニフェストの権限は `audio` だけ。`media` は無いので拒まれるのが正しい）。
//! 下の「クラッシュ」のボタンは意図的にニャプリを落とす。ホストが落ちずにパネルを閉じ、強制終了のダイアログを
//! 出すことを確かめる。画面の語彙（ボタン・スイッチ・文字・面）の見本も兼ねる。

use std::hint::black_box;

use nyapp_sdk::host::{self, Error};
use nyapp_sdk::*;

/// 1 つの確かめの結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn probe(name: &'static str, ok: bool, detail: impl Into<String>) -> Probe {
    Probe {
        name,
        ok,
        detail: detail.into(),
    }
}

/// ホスト関数を順に確かめる（起動時と「もう一度」で呼ぶ）。
pub fn run_probes() -> Vec<Probe> {
    let mut out = Vec::new();
    let now = host::now_ms();
    // 2020-01-01 より後。
    out.push(probe(
        "clock",
        now > 1_577_836_800_000,
        format!("now_ms = {now}"),
    ));
    let offset = host::utc_offset();
    out.push(probe(
        "utc_offset",
        offset.abs() <= 14 * 3600,
        format!("{offset} s"),
    ));
    let (a, b) = (host::random(), host::random());
    out.push(probe("random", a != b, format!("{a:x} / {b:x}")));
    let lang = host::language();
    out.push(probe("language", lang == "en" || lang == "ja", lang));

    let set = host::storage_set("probe", "こんにちは");
    let read = host::storage_get("probe");
    out.push(probe(
        "storage round trip",
        set.is_ok() && read.as_deref() == Some("こんにちは"),
        format!("{set:?} → {read:?}"),
    ));
    let removed = host::storage_remove("probe");
    out.push(probe(
        "storage remove",
        removed.is_ok() && host::storage_get("probe").is_none(),
        format!("{removed:?}"),
    ));
    let big = host::storage_set("big", &"x".repeat(2 << 20));
    out.push(probe(
        "storage quota (2 MiB is refused)",
        big == Err(Error::Quota),
        format!("{big:?}"),
    ));
    let empty = host::storage_set("", "v");
    out.push(probe(
        "storage empty key is refused",
        empty == Err(Error::Invalid),
        format!("{empty:?}"),
    ));

    let audio = host::has_permission("audio");
    let beep = host::beep();
    out.push(probe(
        "permission audio (declared)",
        audio && beep.is_ok(),
        format!("{audio} / {beep:?}"),
    ));
    let media = host::has_permission("media");
    let playing = host::now_playing();
    out.push(probe(
        "permission media (not declared) is denied",
        !media && playing == Err(Error::Denied),
        format!("{media} / {playing:?}"),
    ));
    let unknown = host::has_permission("camera");
    out.push(probe("unknown permission", !unknown, format!("{unknown}")));

    // メモリは上限（64 MiB）の内なら使える。
    let block = black_box(vec![7u8; 8 << 20]);
    out.push(probe(
        "memory 8 MiB",
        block.iter().step_by(4096).all(|&b| b == 7),
        format!("{} bytes", block.len()),
    ));
    host::log(host::Level::Info, "sandbox probe finished");
    out.push(probe("log", true, "see nyaw.log (target nyapp)"));
    out
}

/// 意図的なクラッシュ。どれもホストには trap として届く。
pub fn crash(kind: &str) {
    match kind {
        "panic" => panic!("intentional panic from the sandbox probe"),
        // 燃料（命令数の上限）が尽きるまで回る。
        "loop" => {
            let mut n = 0u64;
            loop {
                n = black_box(n.wrapping_add(1));
            }
        }
        // メモリの上限を超えて確保する（確保に失敗して abort する）。
        "memory" => {
            let v = vec![1u8; 512 << 20];
            black_box(&v);
        }
        // 呼び出しが深すぎて wasm の呼び出しの上限を超える。
        "stack" => {
            black_box(deep(0));
        }
        _ => {}
    }
}

/// 止まらない再帰（足し算と掛け算を混ぜ、ループに直されないようにする）。
fn deep(n: u32) -> u32 {
    if n == u32::MAX {
        return 0;
    }
    deep(black_box(n + 1)).wrapping_mul(3) ^ n
}

pub struct SandboxProbe {
    ja: bool,
    probes: Vec<Probe>,
    ticks: u32,
    ticking: bool,
    switch_on: bool,
    pointer: Option<(f32, f32)>,
    clicks: u32,
}

impl SandboxProbe {
    fn t<'a>(&self, en: &'a str, ja: &'a str) -> &'a str {
        if self.ja { ja } else { en }
    }
}

impl App for SandboxProbe {
    fn new() -> Self {
        Self {
            ja: host::language() == "ja",
            probes: run_probes(),
            ticks: 0,
            ticking: false,
            switch_on: false,
            pointer: None,
            clicks: 0,
        }
    }

    fn view(&mut self) -> Node {
        let passed = self.probes.iter().filter(|p| p.ok).count();
        let rows = self.probes.iter().map(|p| {
            row([
                text(if p.ok { "✓" } else { "✗" }).bold().color(if p.ok {
                    Color::Accent
                } else {
                    Color::Danger
                }),
                text(p.name),
                spacer(),
                text(p.detail.clone()).size(Size::Sm).color(Color::Low),
            ])
            .gap(Gap::Md)
            .align(Align::Center)
        });
        let section = |title: &str| text(title).size(Size::Sm).bold().color(Color::Low);
        let (px, py) = self.pointer.unwrap_or((-100.0, -100.0));
        column([
            row([
                text(format!(
                    "{} {passed}/{}",
                    self.t("Host functions", "ホスト関数"),
                    self.probes.len()
                ))
                .size(Size::Lg)
                .bold(),
                spacer(),
                button("probe-rerun", self.t("Run again", "もう一度")),
            ])
            .align(Align::Center),
            card(rows).gap(Gap::Xs),
            section(self.t(
                "Crash (the panel closes and nyaw reports it)",
                "クラッシュ（パネルが閉じ、nyaw が知らせる）",
            )),
            row([
                button("crash-panic", "panic").variant(Variant::Danger),
                button("crash-loop", self.t("Infinite loop", "無限ループ"))
                    .variant(Variant::Danger),
                button("crash-memory", self.t("Use 512 MiB", "512 MiB 確保"))
                    .variant(Variant::Danger),
                button("crash-stack", self.t("Stack overflow", "スタックあふれ"))
                    .variant(Variant::Danger),
            ])
            .gap(Gap::Sm),
            section(self.t("Ticker and widgets", "周期と部品")),
            card([
                row([
                    button(
                        "ticker-toggle",
                        if self.ticking {
                            self.t("Stop ticks", "周期を止める")
                        } else {
                            self.t("Tick every 100 ms", "100ms ごとに受け取る")
                        },
                    )
                    .selected(self.ticking),
                    text(format!("ticks = {}", self.ticks)),
                    spacer(),
                    switch("probe-switch", self.switch_on),
                    text(format!("clicks = {}", self.clicks)).color(Color::Low),
                ])
                .gap(Gap::Md)
                .align(Align::Center),
                row([
                    button("variant-default", "Default"),
                    button("variant-primary", "Primary").variant(Variant::Primary),
                    button("variant-danger", "Danger").variant(Variant::Danger),
                    button("variant-ghost", "Ghost").variant(Variant::Ghost),
                ])
                .gap(Gap::Sm),
                row([
                    text("Sm").size(Size::Sm),
                    text("Md"),
                    text("Lg").size(Size::Lg),
                    text("Xl").size(Size::Xl).color(Color::Accent),
                    text(format!("pointer = {px:.0}, {py:.0}")).color(Color::Low),
                ])
                .gap(Gap::Lg)
                .align(Align::Center),
                canvas(
                    "probe-canvas",
                    480.0,
                    80.0,
                    vec![
                        Shape::Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 480.0,
                            h: 80.0,
                            radius: 12.0,
                            color: Color::Background,
                        },
                        Shape::Circle {
                            x: px,
                            y: py,
                            r: 12.0,
                            color: Color::Accent,
                        },
                    ],
                ),
            ])
            .gap(Gap::Md),
        ])
        .gap(Gap::Md)
        .scroll()
        .grow()
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Click { id } => {
                self.clicks += 1;
                if let Some(kind) = id.strip_prefix("crash-") {
                    crash(kind);
                }
                match id.as_str() {
                    "probe-rerun" => self.probes = run_probes(),
                    "ticker-toggle" => {
                        self.ticking = !self.ticking;
                        host::set_ticker(if self.ticking { 100 } else { 0 });
                    }
                    _ => {}
                }
            }
            Event::Toggle { on, .. } => self.switch_on = on,
            Event::Pointer { id, x, y } if id == "probe-canvas" => self.pointer = Some((x, y)),
            Event::Pointer { .. } => {}
            Event::Tick => self.ticks += 1,
        }
    }
}

export!(SandboxProbe);
