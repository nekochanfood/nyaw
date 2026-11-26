//! 通知表示先のウォーターフォール遷移。届いた通知は HUD → 手首 → ダッシュボードへ一方向に進み、戻らない。
//! ダッシュボードで一度でも表示された通知は HUD・手首には再表示しない。

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// HUD 表示時間（新規通知の到着ごとに延長される）。超過後も通知は手首で待機する。
pub(crate) const HUD_TIME: Duration = Duration::from_secs(10);
/// 手首から視線を外してから通知を非表示にするまでの猶予時間（一瞬の視線逸れで消えないようにする）。
pub(crate) const WRIST_LINGER: Duration = Duration::from_millis(1500);

/// 通知ごとの遷移段階。小さい値から大きい値へ一方向に進行する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    /// 到着直後（HUD 表示可能）。
    Hud,
    /// 手首で表示済み。
    Wrist,
    /// ダッシュボードで表示中。
    Dashboard,
    /// ダッシュボードで確認済みまたはアクティベート済み（以降表示しない）。
    Done,
}

/// トーストパネルの表示位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    Hidden,
    /// 視界の端（頭部追従。設定の `notification_anchor` で位置を指定）。
    Hud,
    /// 手首の時計上部。
    Wrist,
    /// ダッシュボード上部（埋め込み画面がある場合はその上）。
    Dashboard,
}

/// 現在のシーン状態。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Scene {
    /// ダッシュボードが表示されている。
    pub dashboard: bool,
    /// 通知センター（ダッシュボードの一覧画面）が表示されている。全通知がそこに並ぶため、トーストは非表示にする（閲覧済みとなる）。
    pub center: bool,
    /// 手首の時計を注視中（[`kernel::PanelStatus::gazed`]）。
    pub gazing: bool,
    /// ロックダウン中（シェル表示を抑制し、通知を保留する）。
    pub lockdown: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Flow {
    stages: HashMap<u32, Stage>,
    /// 最後の通知到着から [`HUD_TIME`] 経過後の時刻。
    hud_until: Option<Instant>,
    /// 最後に手首を注視していた時刻。
    gazed_at: Option<Instant>,
}

impl Flow {
    /// 通知一覧 `ids`（到着順）とシーン状態から、表示位置と対象通知（新しい順）を決定し、段階を進める。
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
            if scene.center {
                return (Place::Hidden, Vec::new());
            }
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

    /// アクティベートされた通知を完了状態にする（以降表示しない）。
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
        center: false,
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

    /// 通知センター表示中は新規到着分を含めすべて非表示とし、閲覧済みにする（閉じても再表示されない）。
    #[test]
    fn the_notification_center_takes_over() {
        let mut f = Flow::default();
        let t = Instant::now();
        f.step(&[], IDLE, t);
        let center = Scene {
            center: true,
            ..DASHBOARD
        };
        assert_eq!(f.step(&[1], center, t), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1, 2], center, t), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1, 2], GAZING, t), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1, 2], IDLE, t), (Place::Hidden, vec![]));
    }

    /// 時計を注視していなければ HUD に表示され、注視すると手首へ、ダッシュボードを開くとダッシュボードへ遷移する。
    /// ダッシュボードを閉じた後、ダッシュボードで表示済みの通知は再表示されない。
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
            "手首にも遷移しない"
        );
        assert_eq!(
            f.step(&[1], DASHBOARD, later),
            (Place::Hidden, vec![]),
            "確認済み"
        );
    }

    /// HUD 表示時間の超過後も、未閲覧の通知は手首で待機する。手首で表示された通知は HUD に戻らない。
    #[test]
    fn unseen_notifications_wait_on_the_wrist() {
        let mut f = Flow::default();
        let t = Instant::now();
        f.step(&[1], IDLE, t);
        let late = t + HUD_TIME;
        assert_eq!(f.step(&[1], IDLE, late), (Place::Hidden, vec![]));
        assert_eq!(f.step(&[1], GAZING, late), (Place::Wrist, vec![1]));
        // 視線を外してもしばらくは手首に残留し、猶予時間後に非表示になる（HUD には戻らない）。
        assert_eq!(f.step(&[1], IDLE, late), (Place::Wrist, vec![1]));
        let away = late + WRIST_LINGER;
        assert_eq!(f.step(&[1], IDLE, away), (Place::Hidden, vec![]));
        // 新規到着分のみが HUD に表示され、時計を注視すると手首にまとめて（新しい順に）表示される。
        assert_eq!(f.step(&[1, 2], IDLE, away), (Place::Hud, vec![2]));
        assert_eq!(f.step(&[1, 2], GAZING, away), (Place::Wrist, vec![2, 1]));
    }

    /// ダッシュボード表示中に到着した通知はダッシュボードに表示される。アクティベート済み・OS で削除された通知は表示しない。
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
        // 削除済み通知の ID が再出現した場合も（OS は ID を再利用しないが）、新規通知として扱う。
        assert_eq!(f.step(&[1], IDLE, t), (Place::Hud, vec![1]));
    }

    /// ロックダウン中は一切表示せず、解除後（HUD 表示時間内であれば）表示する。
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
