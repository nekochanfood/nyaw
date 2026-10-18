//! 時計（net.bettaworx.clock）。時計・アラーム・タイマー・ストップウォッチ。
//! タイマーを使っている間は、進行中の作業の知らせ（`host::set_activity`）をダッシュボードに出す（ストップウォッチは出さない）。
//! 知らせはニャプリを閉じても残り、nyaw が数え続ける（タイマーが終われば nyaw が手首から鳴らす）。
//! タイマー・ストップウォッチ・アラームは保存領域に残し、開き直すと続きから。
//! アラームが鳴るのはニャプリを開いている間だけ（ホストにバックグラウンドの仕組みがまだ無い）。

use nyapp_sdk::host;
use nyapp_sdk::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Clock,
    Alarm,
    Timer,
    Stopwatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alarm {
    pub hour: u32,
    pub minute: u32,
    pub on: bool,
}

/// 保存領域のキー。
const ALARMS_KEY: &str = "alarms";
const TIMER_KEY: &str = "timer";
const WATCH_KEY: &str = "watch";

/// タイマーの状態: 設定した長さと、動いていれば終わる時刻、止めていれば残り（ms）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    pub set: i64,
    pub end: Option<i64>,
    pub left: i64,
}

/// タイマーを `180000,end=…` / `180000,left=…` の形にする / 戻す。
pub fn encode_timer(t: &Timer) -> String {
    match t.end {
        Some(end) => format!("{},end={end}", t.set),
        None => format!("{},left={}", t.set, t.left),
    }
}

pub fn decode_timer(s: &str) -> Option<Timer> {
    let (set, rest) = s.split_once(',')?;
    let set = set.parse().ok()?;
    let (key, value) = rest.split_once('=')?;
    let value: i64 = value.parse().ok()?;
    match key {
        "end" => Some(Timer {
            set,
            end: Some(value),
            left: set,
        }),
        "left" => Some(Timer {
            set,
            end: None,
            left: value,
        }),
        _ => None,
    }
}

/// アラームを `07:30:1;08:00:0` の形にする / 戻す。
pub fn encode_alarms(alarms: &[Alarm]) -> String {
    alarms
        .iter()
        .map(|a| format!("{:02}:{:02}:{}", a.hour, a.minute, a.on as u8))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn decode_alarms(s: &str) -> Vec<Alarm> {
    s.split(';')
        .filter_map(|a| {
            let mut p = a.split(':').map(|n| n.parse::<u32>().ok());
            let (hour, minute, on) = (p.next()??, p.next()??, p.next()??);
            (hour < 24 && minute < 60).then_some(Alarm {
                hour,
                minute,
                on: on == 1,
            })
        })
        .collect()
}

/// 地方時の日時。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    /// 0 = 日曜。
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 1970-01-01 からの日数（アラームを同じ日に二度鳴らさないため）。
    pub days: i64,
}

/// UNIX 時間（ms）と UTC からの差（秒）から地方時を求める。
pub fn local(now_ms: i64, offset: i32) -> Local {
    let secs = now_ms.div_euclid(1000) + offset as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    // 日数から年月日（Howard Hinnant の civil_from_days）。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + (month <= 2) as i64;
    Local {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7) as u32,
        hour: rem / 3600,
        minute: rem / 60 % 60,
        second: rem % 60,
        days,
    }
}

/// 経過時間（ms）を `分:秒.百分の一` に（1 時間を超えれば `時:分:秒`）。
pub fn stopwatch_text(ms: i64) -> String {
    let ms = ms.max(0);
    let (h, m, s, cs) = (
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms / 10 % 100,
    );
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}.{cs:02}")
    }
}

/// 残り時間（ms）を `分:秒` に（切り上げ。1 時間を超えれば `時:分:秒`）。
pub fn timer_text(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub struct Clock {
    ja: bool,
    tab: Tab,
    alarms: Vec<Alarm>,
    /// 追加するアラームの時刻。
    draft: (u32, u32),
    /// 最後に鳴らしたアラーム（日・時・分）。同じ分に二度鳴らさない。
    fired: Option<(i64, u32, u32)>,
    /// 鳴っている知らせ（止めるまで毎秒鳴らす）。
    ringing: Option<String>,
    timer: Timer,
    /// ストップウォッチの積み重ねと、動いていれば始めた時刻、ラップ。
    watch_acc: i64,
    watch_start: Option<i64>,
    laps: Vec<i64>,
    /// 今頼んでいる周期。
    ticker: u32,
}

impl Clock {
    fn t<'a>(&self, en: &'a str, ja: &'a str) -> &'a str {
        if self.ja { ja } else { en }
    }

    fn save_alarms(&self) {
        if let Err(e) = host::storage_set(ALARMS_KEY, &encode_alarms(&self.alarms)) {
            host::log(host::Level::Warn, &format!("cannot save alarms: {e:?}"));
        }
    }

    /// タイマー・ストップウォッチを保存領域に残し、知らせを今の状態に合わせる。
    fn save_progress(&self) {
        let watch = format!("{},{}", self.watch_acc, self.watch_start.unwrap_or(-1));
        for (key, value) in [(TIMER_KEY, encode_timer(&self.timer)), (WATCH_KEY, watch)] {
            if let Err(e) = host::storage_set(key, &value) {
                host::log(host::Level::Warn, &format!("cannot save {key}: {e:?}"));
            }
        }
        match self.activity() {
            Some(a) => {
                let _ = host::set_activity(&a);
            }
            None => host::clear_activity(),
        }
    }

    /// 出す知らせ（タイマーだけ。ストップウォッチは出さない）: 動いていればカウントダウン、止めていれば残りと進み具合
    /// （文字は輪の中に収まる残りだけ）。どちらでもなければ出さない。
    pub fn activity(&self) -> Option<Activity> {
        if let Some(end) = self.timer.end {
            return Some(Activity::Countdown {
                title: self.t("Timer", "タイマー").to_owned(),
                ends_at_ms: end,
                total_ms: self.timer.set,
            });
        }
        (self.timer.left < self.timer.set).then(|| Activity::Progress {
            title: self.t("Timer (paused)", "タイマー（一時停止）").to_owned(),
            text: timer_text(self.timer.left),
            progress: Some(1.0 - self.timer.left as f32 / self.timer.set as f32),
        })
    }

    fn watch_ms(&self, now: i64) -> i64 {
        self.watch_acc + self.watch_start.map_or(0, |s| now - s)
    }

    /// 毎回の確かめ: アラームの時刻・タイマーの終わり・鳴っている間の音。
    fn tick(&mut self, now: i64) {
        let t = local(now, host::utc_offset());
        if self.fired != Some((t.days, t.hour, t.minute))
            && let Some(a) = self
                .alarms
                .iter()
                .find(|a| a.on && a.hour == t.hour && a.minute == t.minute)
        {
            self.fired = Some((t.days, t.hour, t.minute));
            self.ringing = Some(format!(
                "{} {:02}:{:02}",
                self.t("Alarm", "アラーム"),
                a.hour,
                a.minute
            ));
        }
        if let Some(end) = self.timer.end
            && now >= end
        {
            self.timer.end = None;
            self.timer.left = self.timer.set;
            self.ringing = Some(self.t("Time's up", "時間です").to_owned());
            self.save_progress();
        }
        if self.ringing.is_some() {
            let _ = host::beep();
        }
    }

    /// 要る周期: ストップウォッチを見ている間は細かく、時刻・タイマー・アラーム・鳴っている間は 0.25 秒、ほかは止める。
    fn wanted_ticker(&self) -> u32 {
        if self.tab == Tab::Stopwatch && self.watch_start.is_some() {
            50
        } else if self.tab == Tab::Clock
            || self.timer.end.is_some()
            || self.ringing.is_some()
            || self.alarms.iter().any(|a| a.on)
        {
            250
        } else {
            0
        }
    }

    fn clock_page(&self, now: i64) -> Node {
        let t = local(now, host::utc_offset());
        let weekdays_en = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let weekdays_ja = ["日", "月", "火", "水", "木", "金", "土"];
        let date = if self.ja {
            format!(
                "{}年{}月{}日（{}）",
                t.year, t.month, t.day, weekdays_ja[t.weekday as usize]
            )
        } else {
            format!(
                "{} {}-{:02}-{:02}",
                weekdays_en[t.weekday as usize], t.year, t.month, t.day
            )
        };
        column([
            spacer(),
            text(format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second))
                .size(Size::Display)
                .bold(),
            text(date).size(Size::Lg).color(Color::Low),
            spacer(),
        ])
        .align(Align::Center)
        .grow()
    }

    fn alarm_page(&self) -> Node {
        let rows = self.alarms.iter().enumerate().map(|(i, a)| {
            row([
                text(format!("{:02}:{:02}", a.hour, a.minute))
                    .size(Size::Xl)
                    .bold(),
                spacer(),
                switch(format!("alarm-on-{i}"), a.on),
                button(format!("alarm-delete-{i}"), self.t("Delete", "削除"))
                    .variant(Variant::Ghost),
            ])
            .align(Align::Center)
            .gap(Gap::Md)
        });
        let list = if self.alarms.is_empty() {
            vec![text(self.t("No alarms", "アラームはありません")).color(Color::Low)]
        } else {
            rows.collect()
        };
        column([
            card(list).gap(Gap::Md).scroll().grow(),
            card([row([
                button("draft-hour-down", "−"),
                text(format!("{:02}:{:02}", self.draft.0, self.draft.1))
                    .size(Size::Xl)
                    .bold(),
                button("draft-hour-up", "+"),
                text(self.t("hour", "時")).color(Color::Low),
                button("draft-minute-down", "−"),
                button("draft-minute-up", "+"),
                text(self.t("min", "分")).color(Color::Low),
                spacer(),
                button("alarm-add", self.t("Add", "追加")).variant(Variant::Primary),
            ])
            .align(Align::Center)
            .gap(Gap::Sm)]),
        ])
        .gap(Gap::Md)
        .grow()
    }

    fn timer_page(&self, now: i64) -> Node {
        let left = self.timer.end.map_or(self.timer.left, |end| end - now);
        let running = self.timer.end.is_some();
        column([
            spacer(),
            text(timer_text(left)).size(Size::Display).bold(),
            row([
                button("timer-minus-minute", self.t("−1 min", "−1分")),
                button("timer-plus-minute", self.t("+1 min", "+1分")),
                button("timer-plus-10s", self.t("+10 s", "+10秒")),
            ])
            .gap(Gap::Sm),
            row([
                button(
                    "timer-start",
                    if running {
                        self.t("Pause", "一時停止")
                    } else {
                        self.t("Start", "開始")
                    },
                )
                .variant(Variant::Primary),
                button("timer-reset", self.t("Reset", "リセット")),
            ])
            .gap(Gap::Sm),
            spacer(),
        ])
        .align(Align::Center)
        .gap(Gap::Lg)
        .grow()
    }

    fn stopwatch_page(&self, now: i64) -> Node {
        let running = self.watch_start.is_some();
        let laps = self.laps.iter().enumerate().rev().map(|(i, ms)| {
            row([
                text(format!("{} {}", self.t("Lap", "ラップ"), i + 1)).color(Color::Low),
                spacer(),
                text(stopwatch_text(*ms)),
            ])
        });
        column([
            text(stopwatch_text(self.watch_ms(now)))
                .size(Size::Display)
                .bold(),
            row([
                button(
                    "watch-start",
                    if running {
                        self.t("Stop", "停止")
                    } else {
                        self.t("Start", "開始")
                    },
                )
                .variant(Variant::Primary),
                button("watch-lap", self.t("Lap", "ラップ")),
                button("watch-reset", self.t("Reset", "リセット")),
            ])
            .gap(Gap::Sm),
            card(laps).gap(Gap::Sm).scroll().grow(),
        ])
        .align(Align::Center)
        .gap(Gap::Lg)
        .grow()
    }
}

impl App for Clock {
    fn new() -> Self {
        let now = host::now_ms();
        let mut timer = host::storage_get(TIMER_KEY)
            .and_then(|s| decode_timer(&s))
            .unwrap_or(Timer {
                set: 3 * 60_000,
                end: None,
                left: 3 * 60_000,
            });
        // 閉じている間に終わっていれば、知らせは nyaw が済ませている。最初に戻す。
        if timer.end.is_some_and(|end| end <= now) {
            (timer.end, timer.left) = (None, timer.set);
        }
        let (watch_acc, watch_start) = host::storage_get(WATCH_KEY)
            .and_then(|s| {
                let (acc, start) = s.split_once(',')?;
                Some((
                    acc.parse().ok()?,
                    start.parse::<i64>().ok().filter(|&s| s >= 0),
                ))
            })
            .unwrap_or((0, None));
        let clock = Self {
            ja: host::language() == "ja",
            tab: Tab::Clock,
            alarms: host::storage_get(ALARMS_KEY)
                .map(|s| decode_alarms(&s))
                .unwrap_or_default(),
            draft: (7, 0),
            fired: None,
            ringing: None,
            timer,
            watch_acc,
            watch_start,
            laps: Vec::new(),
            ticker: 0,
        };
        // 知らせを今の状態に合わせる（前の起動の知らせが残っていれば置き換える・消す）。
        clock.save_progress();
        clock
    }

    fn view(&mut self) -> Node {
        let wanted = self.wanted_ticker();
        if wanted != self.ticker {
            host::set_ticker(wanted);
            self.ticker = wanted;
        }
        let now = host::now_ms();
        let tab = |id: &str, label: &str, t: Tab| {
            button(id, label)
                .selected(self.tab == t)
                .variant(Variant::Ghost)
                .grow()
        };
        let tabs = row([
            tab("tab-clock", self.t("Clock", "時計"), Tab::Clock),
            tab("tab-alarm", self.t("Alarm", "アラーム"), Tab::Alarm),
            tab("tab-timer", self.t("Timer", "タイマー"), Tab::Timer),
            tab(
                "tab-stopwatch",
                self.t("Stopwatch", "ストップウォッチ"),
                Tab::Stopwatch,
            ),
        ])
        .gap(Gap::Sm);
        let page = match self.tab {
            Tab::Clock => self.clock_page(now),
            Tab::Alarm => self.alarm_page(),
            Tab::Timer => self.timer_page(now),
            Tab::Stopwatch => self.stopwatch_page(now),
        };
        let mut children = vec![tabs];
        if let Some(message) = &self.ringing {
            children.push(card([row([
                text(message.clone())
                    .size(Size::Lg)
                    .bold()
                    .color(Color::Accent),
                spacer(),
                button("ring-stop", self.t("Stop", "止める")).variant(Variant::Danger),
            ])
            .align(Align::Center)]));
        }
        children.push(page);
        column(children).gap(Gap::Md).grow()
    }

    fn event(&mut self, event: Event) {
        let now = host::now_ms();
        match event {
            Event::Tick => self.tick(now),
            Event::Toggle { id, on } => {
                if let Some(a) = id
                    .strip_prefix("alarm-on-")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| self.alarms.get_mut(i))
                {
                    a.on = on;
                    self.save_alarms();
                }
            }
            Event::Click { id } => match id.as_str() {
                "tab-clock" => self.tab = Tab::Clock,
                "tab-alarm" => self.tab = Tab::Alarm,
                "tab-timer" => self.tab = Tab::Timer,
                "tab-stopwatch" => self.tab = Tab::Stopwatch,
                "ring-stop" => self.ringing = None,
                "draft-hour-up" => self.draft.0 = (self.draft.0 + 1) % 24,
                "draft-hour-down" => self.draft.0 = (self.draft.0 + 23) % 24,
                "draft-minute-up" => self.draft.1 = (self.draft.1 + 5) % 60,
                "draft-minute-down" => self.draft.1 = (self.draft.1 + 55) % 60,
                "alarm-add" => {
                    self.alarms.push(Alarm {
                        hour: self.draft.0,
                        minute: self.draft.1,
                        on: true,
                    });
                    self.alarms.sort_by_key(|a| (a.hour, a.minute));
                    self.save_alarms();
                }
                "timer-plus-minute" | "timer-plus-10s" | "timer-minus-minute"
                    if self.timer.end.is_none() =>
                {
                    let delta = match id.as_str() {
                        "timer-plus-minute" => 60_000,
                        "timer-plus-10s" => 10_000,
                        _ => -60_000,
                    };
                    self.timer.set = (self.timer.set + delta).clamp(10_000, 24 * 3_600_000);
                    self.timer.left = self.timer.set;
                    self.save_progress();
                }
                "timer-start" => {
                    match self.timer.end.take() {
                        Some(end) => self.timer.left = (end - now).max(0),
                        None => self.timer.end = Some(now + self.timer.left),
                    }
                    self.save_progress();
                }
                "timer-reset" => {
                    self.timer.end = None;
                    self.timer.left = self.timer.set;
                    self.save_progress();
                }
                "watch-start" => {
                    match self.watch_start.take() {
                        Some(start) => self.watch_acc += now - start,
                        None => self.watch_start = Some(now),
                    }
                    self.save_progress();
                }
                "watch-lap" if self.watch_start.is_some() => self.laps.push(self.watch_ms(now)),
                "watch-reset" => {
                    (self.watch_acc, self.watch_start) = (0, None);
                    self.laps.clear();
                    self.save_progress();
                }
                other => {
                    if let Some(i) = other
                        .strip_prefix("alarm-delete-")
                        .and_then(|i| i.parse::<usize>().ok())
                        .filter(|&i| i < self.alarms.len())
                    {
                        self.alarms.remove(i);
                        self.save_alarms();
                    }
                }
            },
            Event::Pointer { .. } => {}
        }
    }
}

export!(Clock);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_from_unix_ms() {
        // 2026-10-18 13:59:11 UTC は日曜。日本時間（+9h）では 22:59:11。
        let t = local(1_792_331_951_000, 9 * 3600);
        assert_eq!((t.year, t.month, t.day, t.weekday), (2026, 10, 18, 0));
        assert_eq!((t.hour, t.minute, t.second), (22, 59, 11));
        assert_eq!(local(0, 0).weekday, 4, "1970-01-01 は木曜");
        let leap = local(951_782_400_000, 0);
        assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
    }

    #[test]
    fn alarms_round_trip_through_storage_text() {
        let alarms = vec![
            Alarm {
                hour: 7,
                minute: 30,
                on: true,
            },
            Alarm {
                hour: 23,
                minute: 5,
                on: false,
            },
        ];
        assert_eq!(decode_alarms(&encode_alarms(&alarms)), alarms);
        assert_eq!(
            decode_alarms("25:00:1;bad;07:00:1"),
            [Alarm {
                hour: 7,
                minute: 0,
                on: true
            }]
        );
    }

    #[test]
    fn timer_round_trips_through_storage_text() {
        let running = Timer {
            set: 180_000,
            end: Some(1_000_000),
            left: 180_000,
        };
        let paused = Timer {
            set: 180_000,
            end: None,
            left: 61_000,
        };
        for t in [running, paused] {
            assert_eq!(decode_timer(&encode_timer(&t)), Some(t));
        }
        assert_eq!(decode_timer("bad"), None);
    }

    fn clock(timer: Timer, watch_start: Option<i64>) -> Clock {
        Clock {
            ja: true,
            tab: Tab::Timer,
            alarms: Vec::new(),
            draft: (7, 0),
            fired: None,
            ringing: None,
            timer,
            watch_acc: 5_000,
            watch_start,
            laps: Vec::new(),
            ticker: 0,
        }
    }

    /// 動いているタイマーはカウントダウン、止めたタイマーは残り。ストップウォッチは動いていても知らせない。
    #[test]
    fn activity_follows_the_timer_only() {
        let idle = Timer {
            set: 60_000,
            end: None,
            left: 60_000,
        };
        assert_eq!(clock(idle, None).activity(), None);
        assert_eq!(
            clock(idle, Some(10_000)).activity(),
            None,
            "ストップウォッチは知らせない"
        );
        let running = Timer {
            end: Some(90_000),
            ..idle
        };
        assert!(matches!(
            clock(running, Some(0)).activity(),
            Some(Activity::Countdown {
                ends_at_ms: 90_000,
                total_ms: 60_000,
                ..
            })
        ));
        let paused = Timer {
            left: 15_000,
            ..idle
        };
        let Some(Activity::Progress {
            title,
            text,
            progress,
        }) = clock(paused, None).activity()
        else {
            panic!("止めたタイマーは残りを出す");
        };
        assert_eq!(
            (title.as_str(), text.as_str(), progress),
            ("タイマー（一時停止）", "00:15", Some(0.75))
        );
    }

    #[test]
    fn time_texts() {
        assert_eq!(stopwatch_text(61_230), "01:01.23");
        assert_eq!(stopwatch_text(3_723_000), "1:02:03");
        assert_eq!(timer_text(59_001), "01:00", "残りは切り上げ");
        assert_eq!(timer_text(0), "00:00");
    }
}
