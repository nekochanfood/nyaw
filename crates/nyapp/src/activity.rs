//! 進行中の作業の知らせ（ライブアクティビティ）。ニャプリが `host::set_activity` で出し、ダッシュボードのバーの
//! クイック設定と再生中の間に出る（iPhone の Dynamic Island のような進み具合の表示）。
//!
//! ニャプリを閉じても残る（サンドボックスは閉じると消えるので、知らせはここ（GPUI のグローバル）が持つ）。
//! 表示は終わる時刻・始めた時刻から nyaw が数え続ける。カウントダウンが終わると、ニャプリが開いていなければ
//! （開いていればニャプリ自身が知らせる）権限 `audio` があるとき手首（wrist）から音を鳴らし、「終わった」状態で残す。

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, Task};
use kernel::PanelCommand;
use kernel::audio::Sound;
use nyapp_sdk::Activity;
use ui::PanelControl;

use crate::{Installed, Permission, system};

/// 出ている知らせ 1 つ。
#[derive(Clone)]
pub struct Live {
    /// 出したニャプリ（名前・アイコン・押したときに開くもの）。
    pub app: Installed,
    pub activity: Activity,
    /// カウントダウンが終わった。
    pub finished: bool,
}

impl Live {
    /// 今（UNIX 時間の ms が `now`）の表示の文字（終わったカウントダウンは `None`。呼ぶ側が「終わった」と出す）。
    pub fn text(&self, now: i64) -> Option<String> {
        match &self.activity {
            Activity::Countdown { .. } if self.finished => None,
            Activity::Countdown { ends_at_ms, .. } => Some(clock(
                ends_at_ms.saturating_sub(now).max(0).saturating_add(999),
            )),
            Activity::Stopwatch { started_at_ms, .. } => {
                Some(clock(now.saturating_sub(*started_at_ms).max(0)))
            }
            Activity::Progress { text, .. } => Some(text.clone()),
        }
    }

    /// 今の進み具合（0〜1）。無ければ `None`。
    pub fn progress(&self, now: i64) -> Option<f32> {
        match &self.activity {
            Activity::Countdown { .. } if self.finished => Some(1.0),
            Activity::Countdown {
                ends_at_ms,
                total_ms,
                ..
            } => Some(
                (1.0 - ends_at_ms.saturating_sub(now) as f32 / *total_ms as f32).clamp(0.0, 1.0),
            ),
            Activity::Stopwatch { .. } => None,
            Activity::Progress { progress, .. } => *progress,
        }
    }
}

/// 出ている知らせ（出した順）と、ニャプリごとのカウントダウンのタイマー（置き換え・消去で捨てて止める）。
#[derive(Default)]
pub struct Activities(Vec<Live>, HashMap<String, (i64, Task<()>)>);

impl gpui::Global for Activities {}

/// 出ている知らせ（出した順）。
pub fn all(cx: &App) -> &[Live] {
    cx.try_global::<Activities>().map_or(&[], |a| &a.0)
}

/// ニャプリ `app` の知らせを出す・置き換える（`None` で消す）。
pub fn set(app: &Installed, activity: Option<Activity>, cx: &mut App) {
    let id = app.manifest.id.clone();
    let activities = cx.default_global::<Activities>();
    let armed = activities.1.get(&id).map(|(ends, _)| *ends);
    let list = &mut activities.0;
    let at = list.iter().position(|l| l.app.manifest.id == id);
    match (activity, at) {
        (None, at) => {
            if let Some(at) = at {
                list.remove(at);
            }
            activities.1.remove(&id);
        }
        (Some(activity), at) => {
            let countdown = match &activity {
                Activity::Countdown { ends_at_ms, .. } => Some(*ends_at_ms),
                _ => None,
            };
            let live = Live {
                app: app.clone(),
                activity,
                finished: false,
            };
            match at {
                Some(at) => list[at] = live,
                None => list.push(live),
            }
            match countdown {
                Some(ends) if armed == Some(ends) => {}
                Some(ends) => watch_countdown(id, ends, cx),
                None => {
                    activities.1.remove(&id);
                }
            }
        }
    }
}

/// ニャプリ `id` の知らせを消す（消したニャプリ・押して開いた終わったカウントダウン）。
pub fn clear(id: &str, cx: &mut App) {
    if let Some(a) = cx.try_global::<Activities>()
        && a.0.iter().any(|l| l.app.manifest.id == id)
    {
        let a = cx.default_global::<Activities>();
        a.0.retain(|l| l.app.manifest.id != id);
        a.1.remove(id);
    }
}

/// カウントダウンが `ends` に終わったら知らせる。それまでに置き換え・消されていれば何もしない。
fn watch_countdown(id: String, ends: i64, cx: &mut App) {
    // 長大な時刻でもタイマーの Instant 加算を溢れさせず、毎日残りを再確認する。
    const MAX_WAIT_MS: i64 = 24 * 60 * 60 * 1_000;
    let remaining = ends.saturating_sub(now_ms()).max(0);
    let capped = remaining > MAX_WAIT_MS;
    let wait = Duration::from_millis(remaining.min(MAX_WAIT_MS) as u64);
    let key = id.clone();
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        cx.update(|cx| {
            if capped {
                let current = cx.try_global::<Activities>().is_some_and(|activities| {
                    activities.0.iter().any(|live| {
                        live.app.manifest.id == id
                            && matches!(live.activity, Activity::Countdown { ends_at_ms, .. } if ends_at_ms == ends)
                            && !live.finished
                    })
                });
                if current {
                    watch_countdown(id, ends, cx);
                }
            } else {
                finish(&id, ends, cx);
            }
        });
    });
    cx.default_global::<Activities>().1.insert(key, (ends, task));
}

/// ニャプリ `id` の、`ends` に終わるカウントダウンを終わったことにする。開いていなければ手首から音を鳴らす。
fn finish(id: &str, ends: i64, cx: &mut App) {
    let Some(live) = cx.try_global::<Activities>().and_then(|a| {
        a.0.iter().find(|l| {
            l.app.manifest.id == id
                && matches!(l.activity, Activity::Countdown { ends_at_ms, .. } if ends_at_ms == ends)
                && !l.finished
        })
    }) else {
        return;
    };
    let audible = live.app.manifest.permits(Permission::Audio) && crate::panel_of(id, cx).is_none();
    if let Some(l) = cx
        .default_global::<Activities>()
        .0
        .iter_mut()
        .find(|l| l.app.manifest.id == id)
    {
        l.finished = true;
    }
    if audible && let Some(wrist) = crate::panel_of(system::WRIST.id, cx) {
        cx.default_global::<PanelControl>()
            .panels
            .entry(wrist)
            .or_default()
            .commands
            .push(PanelCommand::Play(Sound::Wav(crate::view::beep()), false));
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// ms を `分:秒`（1 時間を超えれば `時:分:秒`）に。
fn clock(ms: i64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn app(permissions: Vec<Permission>) -> Installed {
        Installed {
            manifest: crate::Manifest {
                id: "net.bettaworx.clock".into(),
                name: "Clock".into(),
                names: Default::default(),
                version: "1".into(),
                api: nyapp_sdk::API,
                permissions,
                update: None,
            },
            path: "clock.nyapp".into(),
            icon: Default::default(),
        }
    }

    #[test]
    fn texts_and_progress_follow_the_clock() {
        let countdown = Live {
            app: app(vec![]),
            activity: Activity::Countdown {
                title: "t".into(),
                ends_at_ms: 70_000,
                total_ms: 100_000,
            },
            finished: false,
        };
        assert_eq!(countdown.text(0).as_deref(), Some("1:10"));
        assert_eq!(
            countdown.text(69_500).as_deref(),
            Some("0:01"),
            "残りは切り上げ"
        );
        assert_eq!(countdown.progress(-30_000), Some(0.0));
        assert_eq!(countdown.progress(20_000), Some(0.5));
        let done = Live {
            finished: true,
            ..countdown
        };
        assert_eq!((done.text(0), done.progress(0)), (None, Some(1.0)));
        let watch = Live {
            app: app(vec![]),
            activity: Activity::Stopwatch {
                title: "t".into(),
                started_at_ms: 0,
            },
            finished: false,
        };
        assert_eq!(watch.text(3_723_000).as_deref(), Some("1:02:03"));
        assert_eq!(watch.progress(1), None);
    }

    #[test]
    fn extreme_activity_times_do_not_overflow() {
        let countdown = Live {
            app: app(vec![]),
            activity: Activity::Countdown {
                title: "t".into(),
                ends_at_ms: i64::MIN,
                total_ms: 1,
            },
            finished: false,
        };
        assert_eq!(countdown.text(i64::MAX).as_deref(), Some("0:00"));
        assert_eq!(countdown.progress(i64::MAX), Some(1.0));
        let stopwatch = Live {
            activity: Activity::Stopwatch {
                title: "t".into(),
                started_at_ms: i64::MIN,
            },
            ..countdown
        };
        assert!(stopwatch.text(i64::MAX).is_some());
    }

    /// ニャプリごとに 1 つで、出し直すと置き換わり、消すと無くなる。
    #[gpui::test]
    fn one_activity_per_nyapp(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let clock = app(vec![]);
            let progress = |text: &str| Activity::Progress {
                title: "t".into(),
                text: text.into(),
                progress: None,
            };
            set(&clock, Some(progress("a")), cx);
            set(&clock, Some(progress("b")), cx);
            assert_eq!(all(cx).len(), 1);
            assert_eq!(all(cx)[0].text(0).as_deref(), Some("b"));
            set(&clock, None, cx);
            assert!(all(cx).is_empty());
            set(&clock, Some(progress("c")), cx);
            clear("net.bettaworx.clock", cx);
            assert!(all(cx).is_empty());
        });
    }

    /// カウントダウンが終わると「終わった」になり、ニャプリを閉じていれば（権限 audio があれば）手首から鳴らす。
    #[gpui::test]
    fn finished_countdown_rings_on_the_wrist_when_the_nyapp_is_closed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::opened(system::WRIST.id, 0, cx);
            let ends = now_ms() + 60_000;
            set(
                &app(vec![Permission::Audio]),
                Some(Activity::Countdown {
                    title: "t".into(),
                    ends_at_ms: ends,
                    total_ms: 60_000,
                }),
                cx,
            );
        });
        cx.run_until_parked();
        assert!(!cx.update(|cx| all(cx)[0].finished));
        cx.executor().advance_clock(Duration::from_secs(61));
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(all(cx)[0].finished);
            let wrist = &cx.global::<PanelControl>().panels[&0].commands;
            assert!(
                matches!(wrist.as_slice(), [PanelCommand::Play(..)]),
                "手首から鳴らす"
            );
        });
    }

    /// 置き換えられたカウントダウンは、前の終わる時刻では終わらない。
    #[gpui::test]
    fn replaced_countdowns_do_not_finish_early(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let clock = app(vec![]);
            let countdown = |ms: i64| Activity::Countdown {
                title: "t".into(),
                ends_at_ms: now_ms() + ms,
                total_ms: ms,
            };
            set(&clock, Some(countdown(10_000)), cx);
            set(&clock, Some(countdown(600_000)), cx);
        });
        cx.executor().advance_clock(Duration::from_secs(11));
        cx.run_until_parked();
        assert!(!cx.update(|cx| all(cx)[0].finished));
    }
}
