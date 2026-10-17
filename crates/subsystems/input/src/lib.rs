//! 入力サブシステム（ドライバ）。VR サブシステムから来るデバイスごとの生入力を、
//! オーバーレイ層が理解する少数のアクションに変換する。コントローラ追加 = [`PROFILES`] に1行追加。
//! この層より上でプロファイル名を見てはいけない。

use vr::{Facing, Hand, HandSkeleton, Mat3, PALM, Pose, Quat, RawController, TIPS, Vec2, Vec3};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ClickButton {
    #[default]
    Left,
    Middle,
    Right,
}

/// オーバーレイ処理がどのコントローラからも必要とする状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerState {
    pub hand: Hand,
    /// レーザーの原点と向き（前方 = -Z）。
    pub aim: Pose,
    /// UI 要素のクリック・押下。
    pub select: bool,
    pub click_button: ClickButton,
    /// select の入力の強さ（0〜1。トリガーの引き具合）。押下の判定前の引き始めを知るため。
    pub trigger: f32,
    /// パネルを掴んで動かす。
    pub grab: bool,
    pub menu: bool,
    /// ショートカットボタン（左手の下のボタン。Touch は X、Index は左手の A）。何に使うかは画面が決める。右手では常に `false`。
    pub shortcut: bool,
    pub scroll: Vec2,
    /// 掴んでいるパネルの拡大縮小（-1..1、+ で大きく）。機種の割り当ては無く、[`vr::RESIZE_INPUT`] をそのまま読む。
    pub resize: f32,
    /// 顔に向けているのが手のひらか手の甲か。骨格だけの手（[`HAND_PROFILE`]）のときだけ分かる。
    pub facing: Option<Facing>,
}

/// アクションごとの入力名（OpenXR の語彙）。報告された候補のうち最も大きい値を採用する。
struct Profile {
    id: &'static str,
    select: &'static [&'static str],
    grab: &'static [&'static str],
    menu: &'static [&'static str],
    /// 左手のときだけ読む。
    shortcut: &'static [&'static str],
    scroll: Option<(&'static str, &'static str)>,
}

const SIMPLE: Profile = Profile {
    id: "khr/simple_controller",
    select: &["select/click"],
    grab: &[],
    menu: &["menu/click"],
    shortcut: &[],
    scroll: None,
};

const PROFILES: &[Profile] = &[
    SIMPLE,
    Profile {
        id: "valve/index_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/value"],
        menu: &["b/click"],
        shortcut: &["a/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "oculus/touch_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/value"],
        menu: &["menu/click", "y/click", "b/click"],
        shortcut: &["x/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    // コントローラの無い手（スケルタル入力。[`hand_controller`] が作る）。親指と人差し指でつまむとクリック、握ると掴む。
    Profile {
        id: HAND_PROFILE,
        select: &["pinch_ext/value"],
        grab: &["grasp_ext/value"],
        menu: &[],
        // 左手の親指と小指でつまむ（顔に手のひらを向けている間だけ。二回でダッシュボード）。
        shortcut: &[LITTLE_PINCH_INPUT],
        scroll: None,
    },
    Profile {
        id: "htc/vive_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/click"],
        menu: &["menu/click"],
        shortcut: &[],
        scroll: Some(("trackpad/x", "trackpad/y")),
    },
];

/// 骨格だけの手のプロファイル（OpenXR の XR_EXT_hand_interaction に揃える）。
pub const HAND_PROFILE: &str = "ext/hand_interaction_ext";
/// 骨格だけの手の、親指と小指のつまみの強さ（手のひらを顔に向けていなければ 0）。OpenXR に無いので nyaw の名前。
const LITTLE_PINCH_INPUT: &str = "nyaw/little_pinch";
/// 骨格だけの手が顔へ手のひらを向けている度合い（[`vr::HandSkeleton::palm_toward`]。-1〜1）。
const PALM_TOWARD_INPUT: &str = "nyaw/palm_toward";
/// 小指のつまみを受け付ける、手のひらを顔へ向けている度合い（cos。約 60° 以内）。
const PALM_TOWARD_MIN: f32 = 0.5;

/// つまみ（親指の先と人差し指の先の距離、m）をこの間で 1〜0 の強さにする。押下は約 2.6cm、離すのは約 3.4cm（[`PRESS`] / `RELEASE`）。
// ponytail: 手の大きさによらない固定の距離。小さい手で押しにくい・大きい手で誤爆するなら、手の長さで割った比にする。
const PINCH_FULL_M: f32 = 0.01;
const PINCH_NONE_M: f32 = 0.05;
/// 握り（中指・薬指・小指の先から手のひらまでの平均距離、m）をこの間で 1〜0 の強さにする。押下は約 6cm。
const GRASP_FULL_M: f32 = 0.04;
const GRASP_NONE_M: f32 = 0.09;
/// レーザーの元にする肩の位置（頭から見て、外側へ・下へ。m）。肩から手へ向けると、手首の揺れでレーザーが振れにくい。
const SHOULDER_OUT_M: f32 = 0.15;
const SHOULDER_DOWN_M: f32 = 0.2;

/// `d` が `full` で 1、`none` で 0、間は線形。
fn closeness(d: f32, full: f32, none: f32) -> f32 {
    ((none - d) / (none - full)).clamp(0.0, 1.0)
}

/// コントローラの無い手の骨格を、[`HAND_PROFILE`] の生入力にする（上位からは普通のコントローラに見える）。
/// レーザーは推定した肩から親指と人差し指の付け根の間へ向けた線で、元はその付け根の間（つまんでも動かない所）。
pub fn hand_controller(skeleton: &HandSkeleton, hmd: Pose) -> RawController {
    let j = |i: usize| skeleton.joints[i].position;
    let head = hmd.leveled();
    let out = match skeleton.hand {
        Hand::Left => -SHOULDER_OUT_M,
        Hand::Right => SHOULDER_OUT_M,
    };
    let shoulder = head.transform_point(Vec3::new(out, 0.0, 0.0)) - Vec3::Y * SHOULDER_DOWN_M;
    // 親指の基節（3）と人差し指の基節（7）の間。
    let origin = j(3).lerp(j(7), 0.5);
    let back = (shoulder - origin).normalize_or(Vec3::Z);
    let x = Vec3::Y.cross(back).normalize_or(Vec3::X);
    let aim = Pose::new(
        origin,
        Quat::from_mat3(&Mat3::from_cols(x, back.cross(x), back)),
    );
    let toward = skeleton.palm_toward(hmd.position);
    let little = if toward >= PALM_TOWARD_MIN {
        closeness(skeleton.pinch_distance(TIPS[4]), PINCH_FULL_M, PINCH_NONE_M)
    } else {
        0.0
    };
    let grasp = TIPS[2..]
        .iter()
        .map(|&t| j(t).distance(j(PALM)))
        .sum::<f32>()
        / 3.0;
    RawController {
        hand: skeleton.hand,
        profile: HAND_PROFILE.into(),
        aim,
        inputs: vec![
            (
                "pinch_ext/value".into(),
                closeness(skeleton.pinch_distance(TIPS[1]), PINCH_FULL_M, PINCH_NONE_M),
            ),
            (
                "grasp_ext/value".into(),
                closeness(grasp, GRASP_FULL_M, GRASP_NONE_M),
            ),
            (LITTLE_PINCH_INPUT.into(), little),
            (PALM_TOWARD_INPUT.into(), toward),
        ],
    }
}

/// ヒステリシス: 押下は `PRESS` 超え、離すのは `RELEASE` 未満になってから。
// ponytail: 全アクション共通のしきい値。アクション毎に変えたくなったら Profile に持たせる。
pub const PRESS: f32 = 0.6;
const RELEASE: f32 = 0.4;

impl ControllerState {
    /// 押下になるまでの進み（0〜1）。ポインタ補正側は押下のしきい値を知らなくてよい。
    pub fn select_progress(&self) -> f32 {
        (self.trigger / PRESS).clamp(0.0, 1.0)
    }
}

fn held(v: f32, was: bool) -> bool {
    v > if was { RELEASE } else { PRESS }
}

fn strongest(raw: &RawController, names: &[&str]) -> f32 {
    names
        .iter()
        .filter_map(|n| raw.input(n))
        .fold(0.0, f32::max)
}

/// 未知のデバイスは KHR simple controller のバインドにフォールバックする。
fn profile(id: &str) -> &'static Profile {
    PROFILES.iter().find(|p| p.id == id).unwrap_or(&SIMPLE)
}

/// ボタン系の抽象アクション。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Select,
    Grab,
    Menu,
    /// 左手の X。何に使うかは画面が決める（今は wrist が二回押しでダッシュボードを開く）。
    Shortcut,
}

/// 逆引き: `profile` で `action` を起こす入力名。テストでの入力生成や操作ヒント表示用。
pub fn source(profile_id: &str, action: Action) -> Option<&'static str> {
    let p = profile(profile_id);
    match action {
        Action::Select => p.select,
        Action::Grab => p.grab,
        Action::Menu => p.menu,
        Action::Shortcut => p.shortcut,
    }
    .first()
    .copied()
}

/// 逆引き: `profile` でスクロールを起こす入力名（x, y）。スクロールできない機種は `None`。
pub fn scroll_source(profile_id: &str) -> Option<(&'static str, &'static str)> {
    profile(profile_id).scroll
}

/// `prev` は同じ手の前フレームの結果（ヒステリシス用。初回は `None`）。
pub fn map(raw: &RawController, prev: Option<&ControllerState>) -> ControllerState {
    let p = profile(&raw.profile);
    let was = |f: fn(&ControllerState) -> bool| prev.is_some_and(f);
    let trigger = strongest(raw, p.select);
    ControllerState {
        hand: raw.hand,
        aim: raw.aim,
        select: held(trigger, was(|c| c.select)),
        click_button: if p.id == "oculus/touch_controller" {
            let (right, middle) = match raw.hand {
                Hand::Left => ("x/touch", "y/touch"),
                Hand::Right => ("a/touch", "b/touch"),
            };
            if raw.input(right).unwrap_or(0.0) > 0.5 {
                ClickButton::Right
            } else if raw.input(middle).unwrap_or(0.0) > 0.5 {
                ClickButton::Middle
            } else {
                ClickButton::Left
            }
        } else {
            ClickButton::Left
        },
        trigger,
        grab: held(strongest(raw, p.grab), was(|c| c.grab)),
        menu: held(strongest(raw, p.menu), was(|c| c.menu)),
        shortcut: raw.hand == Hand::Left && held(strongest(raw, p.shortcut), was(|c| c.shortcut)),
        scroll: p.scroll.map_or(Vec2::ZERO, |(x, y)| {
            Vec2::new(raw.input(x).unwrap_or(0.0), raw.input(y).unwrap_or(0.0))
        }),
        resize: raw.input(vr::RESIZE_INPUT).unwrap_or(0.0).clamp(-1.0, 1.0),
        facing: raw.input(PALM_TOWARD_INPUT).map(|t| {
            if t >= 0.0 {
                Facing::Palm
            } else {
                Facing::Back
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(profile: &str, inputs: &[(&str, f32)]) -> RawController {
        RawController {
            hand: Hand::Right,
            profile: profile.into(),
            aim: Pose::IDENTITY,
            inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn hysteresis_holds_between_thresholds() {
        let r = |v| raw("valve/index_controller", &[("trigger/value", v)]);
        let a = map(&r(0.5), None);
        assert!(!a.select); // 0.6 未満では押下にならない
        let b = map(&r(0.7), Some(&a));
        assert!(b.select);
        assert!(map(&r(0.5), Some(&b)).select); // 0.4 まで保持
        assert!(!map(&r(0.3), Some(&b)).select);
    }

    #[test]
    fn different_devices_same_action() {
        let index = map(
            &raw(
                "valve/index_controller",
                &[("trigger/value", 0.9), ("thumbstick/y", 1.0)],
            ),
            None,
        );
        let simple = map(
            &raw("khr/simple_controller", &[("select/click", 1.0)]),
            None,
        );
        assert!(index.select && simple.select);
        assert_eq!(index.scroll, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn touch_menu_candidates_and_threshold() {
        let s = map(
            &raw(
                "oculus/touch_controller",
                &[("b/click", 1.0), ("squeeze/value", 0.3)],
            ),
            None,
        );
        assert!(s.menu);
        assert!(!s.grab);
    }

    #[test]
    fn source_roundtrips_through_map() {
        for id in PROFILES.iter().map(|p| p.id) {
            let name = source(id, Action::Select).unwrap();
            assert!(map(&raw(id, &[(name, 1.0)]), None).select, "{id}");
        }
        assert_eq!(source("khr/simple_controller", Action::Grab), None);
        let (x, y) = scroll_source("valve/index_controller").unwrap();
        let s = map(&raw("valve/index_controller", &[(x, 0.5), (y, -1.0)]), None).scroll;
        assert_eq!(s, Vec2::new(0.5, -1.0));
        assert_eq!(scroll_source("khr/simple_controller"), None);
    }

    /// X ボタンは左手だけ。右手の同じボタン（Index の A）は数えない。
    #[test]
    fn shortcut_is_left_hand_x() {
        let left = |r: RawController| RawController {
            hand: Hand::Left,
            ..r
        };
        let touch = raw("oculus/touch_controller", &[("x/click", 1.0)]);
        assert!(map(&left(touch), None).shortcut);
        let index = raw("valve/index_controller", &[("a/click", 1.0)]);
        assert!(!map(&index, None).shortcut);
        assert!(map(&left(index), None).shortcut);
        assert_eq!(source("htc/vive_controller", Action::Shortcut), None);
    }

    /// 開いた右手（肩より上、前に出す）: 指先は手のひらから 9cm 前、親指の先は人差し指の先から 6cm 離れている。
    fn open_hand() -> HandSkeleton {
        let mut joints = [Pose::IDENTITY; vr::JOINT_COUNT];
        for (i, j) in joints.iter_mut().enumerate() {
            j.position = Vec3::new(0.3, 1.5, -0.3 - i as f32 * 0.003);
        }
        for (i, &t) in TIPS.iter().enumerate() {
            joints[t].position = joints[PALM].position + Vec3::new(i as f32 * 0.06 - 0.06, 0.0, -0.09);
        }
        HandSkeleton {
            hand: Hand::Right,
            joints,
        }
    }

    #[test]
    fn hand_pinch_clicks_and_fist_grabs() {
        let head = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
        let mut hand = open_hand();
        let open = map(&hand_controller(&hand, head), None);
        assert!(!open.select && !open.grab);
        // レーザーは前方（-Z）へ、肩（右下）から手へ向かうので少し上・右へ。
        let f = open.aim.forward();
        assert!(f.z < -0.5 && f.x > 0.0 && f.y > 0.0, "{f}");

        hand.joints[TIPS[1]].position = hand.joints[TIPS[0]].position + Vec3::new(0.0, 0.0, 0.015);
        let pinch = map(&hand_controller(&hand, head), Some(&open));
        assert!(pinch.select && !pinch.grab);
        // 少し離しても（3cm）離すしきい値まではクリックのまま。
        hand.joints[TIPS[1]].position = hand.joints[TIPS[0]].position + Vec3::new(0.0, 0.0, 0.03);
        assert!(map(&hand_controller(&hand, head), Some(&pinch)).select);

        for &t in &TIPS[2..] {
            hand.joints[t].position = hand.joints[PALM].position + Vec3::new(0.0, -0.03, 0.0);
        }
        assert!(map(&hand_controller(&hand, head), None).grab);
    }

    /// 左手の親指と小指のつまみは、手のひらを顔に向けている間だけショートカットになる。向きは状態に残る。
    #[test]
    fn little_pinch_is_shortcut_only_with_palm_toward_face() {
        let mut hand = open_hand();
        hand.hand = Hand::Left;
        hand.joints[TIPS[4]].position = hand.joints[TIPS[0]].position + Vec3::new(0.0, 0.0, 0.01);
        // 手のひらの面は手首・人差し指・小指の基節で決まる。左手で手のひらが上（+Y）を向くように置く。
        let palm = hand.joints[PALM].position;
        hand.joints[vr::WRIST].position = palm + Vec3::new(0.0, 0.0, 0.05);
        hand.joints[7].position = palm + Vec3::new(-0.03, 0.0, -0.03);
        hand.joints[22].position = palm + Vec3::new(0.03, 0.0, -0.03);
        let above = Pose::new(palm + Vec3::new(0.0, 0.4, 0.2), Quat::IDENTITY);
        let below = Pose::new(palm + Vec3::new(0.0, -0.4, 0.2), Quat::IDENTITY);
        let s = map(&hand_controller(&hand, above), None);
        assert_eq!(s.facing, Some(Facing::Palm));
        assert!(s.shortcut);
        let s = map(&hand_controller(&hand, below), None);
        assert_eq!(s.facing, Some(Facing::Back));
        assert!(!s.shortcut);
        // 右手では数えない。コントローラでは向きが分からない。
        hand.hand = Hand::Right;
        assert!(!map(&hand_controller(&hand, above), None).shortcut);
        assert_eq!(map(&raw("valve/index_controller", &[]), None).facing, None);
    }

    #[test]
    fn unknown_device_falls_back_to_simple() {
        assert!(map(&raw("acme/mystery", &[("select/click", 1.0)]), None).select);
    }

    /// トリガーの軸が 0 のままでも、ボタンのビットが立っていれば押せる。
    #[test]
    fn trigger_button_selects_when_axis_is_silent() {
        let r = raw(
            "oculus/touch_controller",
            &[("trigger/value", 0.0), ("select/click", 1.0)],
        );
        assert!(map(&r, None).select);
    }

    #[test]
    fn touch_click_modifiers_are_scoped_to_their_hand() {
        for (hand, own_right, own_middle, other_right, other_middle) in [
            (Hand::Left, "x/touch", "y/touch", "a/touch", "b/touch"),
            (Hand::Right, "a/touch", "b/touch", "x/touch", "y/touch"),
        ] {
            let mut r = raw(
                "oculus/touch_controller",
                &[(other_right, 1.0), (other_middle, 1.0)],
            );
            r.hand = hand;
            assert_eq!(map(&r, None).click_button, ClickButton::Left);
            r.inputs.push((own_middle.into(), 1.0));
            assert_eq!(map(&r, None).click_button, ClickButton::Middle);
            r.inputs.push((own_right.into(), 1.0));
            assert_eq!(map(&r, None).click_button, ClickButton::Right);
        }
    }
}
