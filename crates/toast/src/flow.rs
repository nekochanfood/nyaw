//! 通知がどこに出るかの流れ（ウォーターフォール）。届いた通知は視界の端（HUD）→ 手首 → ダッシュボードへ進むだけで戻らない。
//! 一度でもダッシュボードで見た通知は、もう HUD にも手首にも出さない。

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// HUD に出しておく時間（新しい通知が届くたびに延びる）。過ぎても通知は手首で待つ。
pub(crate) const HUD_TIME: Duration = Duration::from_secs(10);
/// 手首から目を離してから、手首の通知を片付けるまで（ちらっと外れただけで消えないように）。
pub(crate) const WRIST_LINGER: Duration = Duration::from_millis(1500);

/// 通知ごとの進み具合。小さい方から大きい方へ進むだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    /// 届いたばかり（HUD に出せる）。
    Hud,
    /// 手首で見た。
    Wrist,
    /// ダッシュボードで見ている。
    Dashboard,
    /// ダッシュボードで見終えた・押した（もう出さない）。
    Done,
}

/// トーストのパネルの置き場所。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    Hidden,
    /// 視界の端（頭に付ける。設定の `notification_anchor`）。
    Hud,
    /// 手首の時計の上。
    Wrist,
    /// ダッシュボードの上（はめた画面があればその上）。
    Dashboard,
}

/// その時の周り。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Scene {
    /// ダッシュボードが出ている。
    pub dashboard: bool,
    /// 手首の時計を見ている（[`kernel::PanelStatus::gazed`]）。
    pub gazing: bool,
    /// ロックダウン中（シェルは控える。通知は待たせる）。
    pub lockdown: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Flow {
    stages: HashMap<u32, Stage>,
    /// 最後に通知が届いてから [`HUD_TIME`] 後。
    hud_until: Option<Instant>,
    /// 最後に手首を見ていた時刻。
    gazed_at: Option<Instant>,
}

impl Flow {
    /// 通知の一覧 `ids`（届いた順）と周りから、置き場所と出す通知（新しい順）を決め、段階を進める。
    pub(crate) fn step(&mut self, ids: &[u32], scene: Scene, now: Instant) -> (Place, Vec<u32>) {
        self.stages.retain(|id, _| ids.contains(id));
        for &id in ids {
            if let std::collections::hash_map::Entry::Vacant(e) = self.stages.entry(id) {
                e.insert(Stage::Hud);
                self.hud_until = Some(now + HUD_TIME);
            }
        }
        if scene.lockdown {
            return (Place::Hidden, Vec::new());
        }
        if scene.gazing {
            self.gazed_at = Some(now);
        }
        let (place, stage) = if scene.dashboard {
            self.advance(|s| s < Stage::Dashboard, Stage::Dashboard);
            (Place::Dashboard, Stage::Dashboard)
        } else {
            self.advance(|s| s == Stage::Dashboard, Stage::Done);
            let watching = self
                .gazed_at
                .is_some_and(|t| now.duration_since(t) < WRIST_LINGER);
            let fresh = self.hud_until.is_some_and(|t| now < t);
            if watching && self.stages.values().any(|&s| s <= Stage::Wrist) {
                self.advance(|s| s == Stage::Hud, Stage::Wrist);
                (Place::Wrist, Stage::Wrist)
            } else if fresh {
                (Place::Hud, Stage::Hud)
            } else {
                return (Place::Hidden, Vec::new());
            }
        };
        let shown: Vec<u32> = ids
            .iter()
            .rev()
            .copied()
            .filter(|id| self.stages[id] == stage)
            .collect();
        if shown.is_empty() {
            return (Place::Hidden, shown);
        }
        (place, shown)
    }

    /// 押した通知を片付ける（もう出さない）。
    pub(crate) fn done(&mut self, id: u32) {
        if let Some(s) = self.stages.get_mut(&id) {
            *s = Stage::Done;
        }
    }

    fn advance(&mut self, from: impl Fn(Stage) -> bool, to: Stage) {
        for s in self.stages.values_mut().filter(|s| from(**s)) {
            *s = to;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: Scene = Scene {
        dashboard: false,
        gazing: false,
        lockdown: false,
    };
    const GAZING: Scene = Scene {
        gazing: true,
        ..IDLE
    };
    const DASHBOARD: Scene = Scene {
        dashboard: true,
        ..IDLE
    };

    /// 時計を見ていないと視界の端に出て、時計を見ると手首へ、ダッシュボードを開くとダッシュボードへ進む。
    /// 閉じると、ダッシュボードで見たものはもう出ない。
    #[test]
    fn a_notification_flows_from_the_hud_to_the_wrist_to_the_dashboard() {
        let mut f = Flow::default();
        let t = Instant::now();
        assert_eq!(f.step(&[], IDLE, t), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1], IDLE, t), (Place::Hud, vec![1]));
        assert_eq!(f.step(&[1], GAZING, t), (Place::Wrist, vec![1]));
        assert_eq!(f.step(&[1], DASHBOARD, t), (Place::Dashboard, vec![1]));
        assert_eq!(f.step(&[1], IDLE, t), (Place::Hidden, vec![]));
        let later = t + WRIST_LINGER;
        assert_eq!(
            f.step(&[1], GAZING, later),
            (Place::Hidden, vec![]),
            "手首にも戻らない"
        );
        assert_eq!(
            f.step(&[1], DASHBOARD, later),
            (Place::Hidden, vec![]),
            "見終えた"
        );
    }

    /// HUD の時間が過ぎても、見ていない通知は手首で待つ。手首で見た通知は HUD に戻らない。
    #[test]
    fn unseen_notifications_wait_on_the_wrist() {
        let mut f = Flow::default();
        let t = Instant::now();
        f.step(&[1], IDLE, t);
        let late = t + HUD_TIME;
        assert_eq!(f.step(&[1], IDLE, late), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1], GAZING, late), (Place::Wrist, vec![1]));
        // 目を離してもしばらくは手首に残り、その後は片付く（HUD には戻らない）。
        assert_eq!(f.step(&[1], IDLE, late), (Place::Wrist, vec![1]));
        let away = late + WRIST_LINGER;
        assert_eq!(f.step(&[1], IDLE, away), (Place::Hidden, vec![]));
        // 新しく届いたものだけが HUD に出て、時計を見ると手首でまとめて（新しい順に）出る。
        assert_eq!(f.step(&[1, 2], IDLE, away), (Place::Hud, vec![2]));
        assert_eq!(f.step(&[1, 2], GAZING, away), (Place::Wrist, vec![2, 1]));
    }

    /// ダッシュボードを開いている間に届いたものはダッシュボードに出る。押したもの・OS で消えたものは出さない。
    #[test]
    fn activated_and_removed_notifications_leave() {
        let mut f = Flow::default();
        let t = Instant::now();
        f.step(&[1], DASHBOARD, t);
        assert_eq!(
            f.step(&[1, 2], DASHBOARD, t),
            (Place::Dashboard, vec![2, 1])
        );
        f.done(2);
        assert_eq!(f.step(&[1, 2], DASHBOARD, t), (Place::Dashboard, vec![1]));
        assert_eq!(f.step(&[2], DASHBOARD, t), (Place::Hidden, vec![]));
        // 消えた通知の ID が戻ってきても（OS は使い回さないが）、新しい通知として扱う。
        assert_eq!(f.step(&[1], IDLE, t), (Place::Hud, vec![1]));
    }

    /// ロックダウン中は何も出さず、解けたら（時間内なら）出す。
    #[test]
    fn lockdown_holds_notifications_back() {
        let mut f = Flow::default();
        let t = Instant::now();
        let locked = Scene {
            lockdown: true,
            ..GAZING
        };
        assert_eq!(f.step(&[1], locked, t), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1], IDLE, t), (Place::Hud, vec![1]));
    }
}
