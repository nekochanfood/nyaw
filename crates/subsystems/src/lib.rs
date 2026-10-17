//! カーネルの最下部。OS・ランタイム・コントローラ入力・描画の差を吸収するサブシステム群。
//! 音声の出力・立体化は audio が担う。
//! 上位のパネル管理や設定には依存せず、既存の各実装を共通の入口から提供する。
//! 新しい環境への対応は各サブシステム内に閉じ、上位層で環境ごとに分岐しない。

pub use {audio, desktop, input, keyboard, os, rendering, vr};

use std::collections::HashMap;

pub use input::ControllerState;
use vr::{Device, Hand, HandSkeleton, Pose, VrBackend};

/// 上位に渡す共通フレーム。機種名・生入力・ランタイム固有の状態は含めない。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub hmd: Pose,
    pub controllers: Vec<ControllerState>,
    /// 骨格が取れている手（スケルタル入力）。
    pub hands: Vec<HandSkeleton>,
    pub trackers: Vec<(String, Pose)>,
    pub quit: bool,
    /// 手ブレのある入力に対して、上位のポインタ補正を有効にする。
    pub stabilize_pointer: bool,
    tracked: bool,
    /// 骨格で操作している手の、取り付け先の姿勢。ランタイムがその手のデバイスを追っていればその姿勢（ランタイムに
    /// 相対配置を任せられ、コントローラと同じく遅れなく付いてくる）、無ければ [`vr::HandSkeleton::grip`] の揺れを抑えたもの。
    grips: Vec<(Hand, Pose)>,
}

impl Frame {
    /// 取り付け先の姿勢。追跡していないデバイスには追従しない。
    pub fn pose(&self, device: &Device) -> Option<Pose> {
        if !self.tracked {
            return None;
        }
        match device {
            Device::Hmd => Some(self.hmd),
            // 骨格で操作している手は、レーザーではなく手の姿勢に付ける（手首を返せば付いてくる）。
            Device::Controller(hand) => self
                .grips
                .iter()
                .find(|(h, _)| h == hand)
                .map(|(_, p)| *p)
                .or_else(|| {
                    self.controllers
                        .iter()
                        .find(|c| c.hand == *hand)
                        .map(|c| c.aim)
                }),
            Device::Tracker(id) => self.trackers.iter().find(|(i, _)| i == id).map(|(_, p)| *p),
        }
    }
}

/// ランタイムと入力履歴を所有し、環境に依らないフレームを生成する。
pub struct Subsystems<B: VrBackend = Box<dyn VrBackend>> {
    /// テストで入力注入・描画結果の確認をするための入口。
    pub backend: B,
    /// 骨格だけの手の調整値（判定のしきい値・レーザーの揺れの抑え）。カーネルが設定から入れる。
    pub hand: input::HandTuning,
    inputs: HashMap<Hand, (String, ControllerState)>,
    /// 骨格だけで操作している手ごとの、レーザーの揺れの抑え。
    filters: HashMap<Hand, input::HandFilter>,
    last_poll: Option<std::time::Instant>,
    /// 手ごとの、骨格で操作しているか（true）と、もう一方へ切り替わりたいフレームが続いた数、
    /// 骨格で操作しているのはコントローラが見えなかったからか（戻ったらすぐコントローラに戻す）。
    sources: HashMap<Hand, Source>,
    /// 骨格で操作している手ごとの、取り付け先の姿勢の揺れの抑え。
    grips: HashMap<Hand, input::PoseFilter>,
}

/// 手の入力の元（[`Subsystems::poll`]）。
#[derive(Debug, Clone, Copy)]
struct Source {
    skeleton: bool,
    waited: u32,
    /// コントローラが見えなかったので骨格にしている（指を全部追っているからではない）。
    fallback: bool,
}

/// 骨格とコントローラのどちらでも操作できる手で、入力の元を切り替えるのに要る、続いたフレームの数（90Hz で約 0.2 秒）。
/// ハンドトラッキングとコントローラを持ち替える間はランタイムの申告（指の追跡の精度）が揺れ、毎フレーム入れ替わると
/// 押下の履歴とレーザーの抑えが捨てられてクリックが効かなくなる。
// ponytail: フレーム数なのでリフレッシュレートで時間が変わる。気になれば経過時間で数える。
const SOURCE_SWITCH_FRAMES: u32 = 18;

impl<B: VrBackend> Subsystems<B> {
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend,
            hand: input::HandTuning::default(),
            inputs: HashMap::new(),
            filters: HashMap::new(),
            last_poll: None,
            sources: HashMap::new(),
            grips: HashMap::new(),
        }
    }

    /// 待ち・生入力の取得・機種別変換・押下のヒステリシスをまとめて処理する。
    pub fn poll(&mut self) -> Frame {
        let mut raw = self.backend.poll();
        let now = std::time::Instant::now();
        let dt = self
            .last_poll
            .replace(now)
            .map_or(0.0, |t| (now - t).as_secs_f32());
        // コントローラを持っていない手は、骨格から作ったコントローラで操作する（スケルタル入力）。
        // 指を全部追っている手（光学式のハンドトラッキング）は、ランタイムがコントローラとして見せていても骨格で操作する
        // （ランタイムのつまみ → トリガーの割り当てには握る・小指のつまみが無い）。
        // 両方ある手は、指の追跡の精度で決め、切り替わりたい状態が続いてから切り替える（片方しか無ければすぐ）。
        // コントローラが一瞬見えなかったので骨格にしていた手は、コントローラが戻ればすぐ戻す（待つ間クリックがつまみの判定になる）。
        for hand in [Hand::Left, Hand::Right] {
            let skeleton = raw.hands.iter().find(|s| s.hand == hand);
            let controller = raw.controllers.iter().any(|c| c.hand == hand);
            let full = skeleton.is_some_and(|s| s.tracking == vr::FingerTracking::Full);
            let want = skeleton.is_some() && (full || !controller);
            let s = self.sources.entry(hand).or_insert(Source {
                skeleton: want,
                waited: 0,
                fallback: want && !full,
            });
            let before = s.skeleton;
            if skeleton.is_none() || !controller {
                *s = Source {
                    skeleton: want,
                    waited: 0,
                    fallback: want && !full,
                };
            } else if want == s.skeleton {
                s.waited = 0;
            } else if s.fallback {
                *s = Source {
                    skeleton: want,
                    waited: 0,
                    fallback: false,
                };
            } else {
                s.waited += 1;
                if s.waited >= SOURCE_SWITCH_FRAMES {
                    (s.skeleton, s.waited) = (want, 0);
                }
            }
            if s.skeleton != before {
                logger::debug!("hand input source switched", hand = ?hand, skeleton = s.skeleton);
            }
        }
        let bare: Vec<_> = raw
            .hands
            .iter()
            .filter(|s| self.sources.get(&s.hand).is_some_and(|s| s.skeleton))
            .map(|s| {
                let mut c = input::hand_controller(s, raw.hmd, &self.hand);
                let prev = self
                    .inputs
                    .get(&s.hand)
                    .filter(|(p, _)| p == input::HAND_PROFILE)
                    .map(|(_, state)| state);
                self.filters
                    .entry(s.hand)
                    .or_default()
                    .steady(&mut c, prev, dt, &self.hand);
                c
            })
            .collect();
        // 見失った・コントローラを持った手の抑えは捨てる（戻ったとき前の位置から追わない）。
        self.filters
            .retain(|hand, _| bare.iter().any(|c| c.hand == *hand));
        self.grips
            .retain(|hand, _| bare.iter().any(|c| c.hand == *hand));
        let grips = raw
            .hands
            .iter()
            .filter(|s| bare.iter().any(|c| c.hand == s.hand))
            .map(|s| match raw.hand_devices.iter().find(|(h, _)| *h == s.hand) {
                Some(&device) => device,
                None => (
                    s.hand,
                    self.grips.entry(s.hand).or_default().steady(s.grip(), dt),
                ),
            })
            .collect();
        raw.controllers
            .retain(|c| bare.iter().all(|b| b.hand != c.hand));
        raw.controllers.extend(bare);
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
                let buttons =
                    |s: &ControllerState| (s.select, s.grab, s.menu, s.shortcut, s.facing);
                if prev.is_none_or(|p| buttons(p) != buttons(&state)) {
                    logger::debug!("controller buttons", hand = ?c.hand, profile = c.profile,
                    select = state.select, grab = state.grab, menu = state.menu,
                    shortcut = state.shortcut, facing = ?state.facing, inputs = ?c.inputs);
                }
                self.inputs.insert(c.hand, (c.profile.clone(), state));
                state
            })
            .collect();
        Frame {
            hmd: raw.hmd,
            controllers,
            hands: raw.hands,
            trackers: raw.trackers,
            quit: raw.quit,
            stabilize_pointer: !raw.emulated,
            tracked: !raw.emulated,
            grips,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::{Quat, RawController, Vec2, Vec3, mock::MockBackend};

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

    /// コントローラを持たない手だけが、骨格から作ったコントローラになる（つまむとクリック）。
    #[test]
    fn bare_hands_become_controllers() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let mut joints = [Pose::IDENTITY; vr::JOINT_COUNT];
        joints[vr::TIPS[1]].position = Vec3::new(0.0, 0.0, -0.005);
        let hand = |hand, tracking| vr::HandSkeleton {
            hand,
            joints,
            tracking,
        };
        systems.backend.frame.hands = vec![
            hand(Hand::Left, vr::FingerTracking::Partial),
            hand(Hand::Right, vr::FingerTracking::Partial),
        ];
        systems.backend.frame.controllers =
            vec![controller(Hand::Right, "valve/index_controller", &[])];
        let frame = systems.poll();
        assert_eq!(frame.controllers.len(), 2);
        let right = frame.controllers.iter().find(|c| c.hand == Hand::Right).unwrap();
        assert!(!right.select && right.facing.is_none(), "the held controller wins");
        let left = frame.controllers.iter().find(|c| c.hand == Hand::Left).unwrap();
        assert!(left.select && left.facing.is_some(), "pinching the bare hand clicks");
        assert_eq!(frame.hands.len(), 2);
        // 指を全部追っている手は、ランタイムがコントローラとして見せていても骨格で操作する。
        systems.backend.frame.hands[1].tracking = vr::FingerTracking::Full;
        // 持ち替えの揺れと区別するため、続いてから切り替わる。
        for _ in 1..SOURCE_SWITCH_FRAMES {
            let right = systems.poll().controllers.into_iter().find(|c| c.hand == Hand::Right);
            assert!(right.unwrap().metrics.is_none(), "still the controller");
        }
        let frame = systems.poll();
        assert_eq!(frame.controllers.len(), 2);
        let right = frame.controllers.iter().find(|c| c.hand == Hand::Right).unwrap();
        assert!(right.select && right.metrics.is_some(), "the optical hand drives");
        // 骨格で操作している手に取り付けたものは、レーザーではなく手の姿勢に付く。
        // （指の追跡の精度が上がってから SOURCE_SWITCH_FRAMES 続いたので、もう骨格で操作している。）
        let grip = frame.hands[1].grip();
        assert_eq!(frame.pose(&Device::Controller(Hand::Right)), Some(grip));
        assert_ne!(grip, right.aim);
        // ランタイムがその手のデバイスを追っていれば、そのデバイスに付く（コントローラと同じく相対配置で追従できる）。
        let device = Pose::new(Vec3::new(0.3, 1.0, -0.2), Quat::from_rotation_y(0.5));
        systems.backend.frame.hand_devices = vec![(Hand::Right, device)];
        let frame = systems.poll();
        assert_eq!(frame.pose(&Device::Controller(Hand::Right)), Some(device));
    }

    /// 持ち替えの間に指の追跡の精度が揺れても、入力の元は入れ替わらず、押下が続く。
    #[test]
    fn flapping_tracking_level_keeps_the_input_source() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let joints = [Pose::IDENTITY; vr::JOINT_COUNT];
        systems.backend.frame.controllers = vec![controller(
            Hand::Right,
            "valve/index_controller",
            &[("trigger/value", 1.0)],
        )];
        for i in 0..100 {
            // 最初はコントローラを握っていて（一部）、ときどき「全部」と申告される。
            let tracking = if i % 3 == 2 {
                vr::FingerTracking::Full
            } else {
                vr::FingerTracking::Partial
            };
            systems.backend.frame.hands = vec![vr::HandSkeleton {
                hand: Hand::Right,
                joints,
                tracking,
            }];
            let c = systems.poll().controllers[0];
            assert!(c.select && c.metrics.is_none(), "frame {i}");
        }
    }

    /// コントローラが一瞬見えなくなって骨格で操作した手は、コントローラが戻ればすぐコントローラに戻る（クリックはトリガー）。
    #[test]
    fn controller_dropout_returns_to_the_controller_at_once() {
        let mut systems = Subsystems::with_backend(MockBackend::default());
        let joints = [Pose::IDENTITY; vr::JOINT_COUNT];
        systems.backend.frame.hands = vec![vr::HandSkeleton {
            hand: Hand::Right,
            joints,
            tracking: vr::FingerTracking::Partial,
        }];
        let held = controller(Hand::Right, "valve/index_controller", &[("trigger/value", 1.0)]);
        systems.backend.frame.controllers = vec![held.clone()];
        assert!(systems.poll().controllers[0].metrics.is_none());
        systems.backend.frame.controllers.clear();
        assert!(systems.poll().controllers[0].metrics.is_some(), "falls back to the skeleton");
        systems.backend.frame.controllers = vec![held];
        let c = systems.poll().controllers[0];
        assert!(c.metrics.is_none() && c.select, "back to the controller at once");
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
