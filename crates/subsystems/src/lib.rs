//! カーネルの最下部。OS・ランタイム・コントローラ入力・描画の差を吸収するサブシステム群。
//! 上位のパネル管理や設定には依存せず、既存の各実装を共通の入口から提供する。
//! 新しい環境への対応は各サブシステム内に閉じ、上位層で環境ごとに分岐しない。

pub use {input, os, rendering, vr};

use std::collections::HashMap;

pub use input::ControllerState;
use vr::{Device, Hand, Pose, VrBackend};

/// 上位に渡す共通フレーム。機種名・生入力・ランタイム固有の状態は含めない。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub hmd: Pose,
    pub controllers: Vec<ControllerState>,
    pub trackers: Vec<(String, Pose)>,
    pub quit: bool,
    /// 手ブレのある入力に対して、上位のポインタ補正を有効にする。
    pub stabilize_pointer: bool,
    tracked: bool,
}

impl Frame {
    /// 取り付け先の姿勢。追跡していないデバイスには追従しない。
    pub fn pose(&self, device: &Device) -> Option<Pose> {
        if !self.tracked {
            return None;
        }
        match device {
            Device::Hmd => Some(self.hmd),
            Device::Controller(hand) => self
                .controllers
                .iter()
                .find(|c| c.hand == *hand)
                .map(|c| c.aim),
            Device::Tracker(id) => self.trackers.iter().find(|(i, _)| i == id).map(|(_, p)| *p),
        }
    }
}

/// ランタイムと入力履歴を所有し、環境に依らないフレームを生成する。
pub struct Subsystems<B: VrBackend = Box<dyn VrBackend>> {
    /// テストで入力注入・描画結果の確認をするための入口。
    pub backend: B,
    inputs: HashMap<Hand, (String, ControllerState)>,
}

impl<B: VrBackend> Subsystems<B> {
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend,
            inputs: HashMap::new(),
        }
    }

    /// 待ち・生入力の取得・機種別変換・押下のヒステリシスをまとめて処理する。
    pub fn poll(&mut self) -> Frame {
        let raw = self.backend.poll();
        // 切断後や別の機種への切り替えで、古い押下状態を持ち越さない。
        self.inputs.retain(|hand, (profile, _)| {
            raw.controllers
                .iter()
                .any(|c| c.hand == *hand && c.profile == *profile)
        });
        let controllers = raw
            .controllers
            .iter()
            .map(|c| {
                let prev = self.inputs.get(&c.hand).map(|(_, state)| state);
                let state = input::map(c, prev);
                let buttons = |s: &ControllerState| (s.select, s.grab, s.menu);
                if prev.is_none_or(|p| buttons(p) != buttons(&state)) {
                    logger::debug!("controller buttons", hand = ?c.hand, profile = c.profile,
                    select = state.select, grab = state.grab, menu = state.menu);
                }
                self.inputs.insert(c.hand, (c.profile.clone(), state));
                state
            })
            .collect();
        Frame {
            hmd: raw.hmd,
            controllers,
            trackers: raw.trackers,
            quit: raw.quit,
            stabilize_pointer: !raw.emulated,
            tracked: !raw.emulated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::{RawController, Vec2, Vec3, mock::MockBackend};

    fn controller(hand: Hand, profile: &str, inputs: &[(&str, f32)]) -> RawController {
        RawController {
            hand,
            profile: profile.into(),
            aim: Pose::IDENTITY,
            inputs: inputs.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn device_profiles_produce_the_same_common_actions() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let mut expected = None;
        for (profile, grab, menu, stick) in [
            (
                "valve/index_controller",
                "squeeze/value",
                "b/click",
                "thumbstick",
            ),
            (
                "oculus/touch_controller",
                "squeeze/value",
                "y/click",
                "thumbstick",
            ),
            (
                "htc/vive_controller",
                "squeeze/click",
                "menu/click",
                "trackpad",
            ),
        ] {
            systems.backend.frame.controllers = vec![controller(
                Hand::Right,
                profile,
                &[
                    ("trigger/value", 1.0),
                    (grab, 1.0),
                    (menu, 1.0),
                    (&format!("{stick}/x"), 0.25),
                    (&format!("{stick}/y"), -0.5),
                ],
            )];
            let frame = systems.poll();
            let state = frame.controllers[0];
            assert!(state.select && state.grab && state.menu);
            assert_eq!(state.scroll, Vec2::new(0.25, -0.5));
            assert_eq!(state.select_progress(), 1.0);
            assert_eq!(*expected.get_or_insert(state), state);
        }
    }

    #[test]
    fn hysteresis_is_independent_per_hand_and_resets_after_disconnect() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let right = |v| {
            controller(
                Hand::Right,
                "valve/index_controller",
                &[("trigger/value", v)],
            )
        };
        let left = |v| {
            controller(
                Hand::Left,
                "valve/index_controller",
                &[("trigger/value", v)],
            )
        };
        systems.backend.frame.controllers = vec![right(0.7), left(0.5)];
        let frame = systems.poll();
        assert!(frame.controllers[0].select);
        assert!(!frame.controllers[1].select);
        systems.backend.frame.controllers = vec![right(0.5), left(0.7)];
        assert!(systems.poll().controllers.iter().all(|c| c.select));
        systems.backend.frame.controllers = vec![left(0.5)];
        assert!(systems.poll().controllers[0].select);
        systems.backend.frame.controllers = vec![right(0.5), left(0.3)];
        assert!(systems.poll().controllers.iter().all(|c| !c.select));
    }

    #[test]
    fn changing_device_does_not_inherit_previous_button_state() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        systems.backend.frame.controllers = vec![controller(
            Hand::Right,
            "valve/index_controller",
            &[("trigger/value", 0.7)],
        )];
        assert!(systems.poll().controllers[0].select);
        systems.backend.frame.controllers[0].profile = "oculus/touch_controller".into();
        systems.backend.frame.controllers[0].inputs[0].1 = 0.5;
        assert!(!systems.poll().controllers[0].select);
    }

    #[test]
    fn common_frame_preserves_tracking_and_quit_without_exposing_emulation() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let head = Pose {
            position: Vec3::Y,
            ..Pose::IDENTITY
        };
        systems.backend.frame.hmd = head;
        systems.backend.frame.controllers = vec![controller(
            Hand::Left,
            "khr/simple_controller",
            &[("select/click", 1.0)],
        )];
        systems.backend.frame.trackers = vec![("waist".into(), Pose::IDENTITY)];
        systems.backend.frame.quit = true;
        let frame = systems.poll();
        assert!(frame.quit && frame.stabilize_pointer && frame.controllers[0].select);
        assert_eq!(frame.pose(&Device::Hmd), Some(head));
        assert_eq!(
            frame.pose(&Device::Controller(Hand::Left)),
            Some(Pose::IDENTITY)
        );
        assert_eq!(frame.pose(&Device::Controller(Hand::Right)), None);
        assert_eq!(
            frame.pose(&Device::Tracker("waist".into())),
            Some(Pose::IDENTITY)
        );
        assert_eq!(frame.pose(&Device::Tracker("missing".into())), None);
        systems.backend.frame.emulated = true;
        let frame = systems.poll();
        assert!(!frame.stabilize_pointer);
        for device in [
            Device::Hmd,
            Device::Controller(Hand::Left),
            Device::Tracker("waist".into()),
        ] {
            assert_eq!(frame.pose(&device), None);
        }
        assert_eq!(frame.hmd, head);
        assert!(frame.controllers[0].select);
    }
}
