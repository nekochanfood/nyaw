//! Layer 2: コントローラ抽象化（ドライバ層）。VR 層から来るデバイスごとの生入力を、
//! オーバーレイ層が理解する少数のアクションに変換する。コントローラ追加 = [`PROFILES`] に1行追加。
//! この層より上でプロファイル名を見てはいけない。

use vr::{Hand, Pose, RawController, Vec2};

/// オーバーレイ処理がどのコントローラからも必要とする状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerState {
    pub hand: Hand,
    /// レーザーの原点と向き（前方 = -Z）。
    pub aim: Pose,
    /// UI 要素のクリック・押下。
    pub select: bool,
    /// パネルを掴んで動かす。
    pub grab: bool,
    pub menu: bool,
    pub scroll: Vec2,
}

/// アクションごとの入力名（OpenXR の語彙）。報告された候補のうち最も大きい値を採用する。
struct Profile {
    id: &'static str,
    select: &'static [&'static str],
    grab: &'static [&'static str],
    menu: &'static [&'static str],
    scroll: Option<(&'static str, &'static str)>,
}

const SIMPLE: Profile = Profile {
    id: "khr/simple_controller",
    select: &["select/click"],
    grab: &[],
    menu: &["menu/click"],
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
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "oculus/touch_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/value"],
        menu: &["menu/click", "y/click", "b/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "htc/vive_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/click"],
        menu: &["menu/click"],
        scroll: Some(("trackpad/x", "trackpad/y")),
    },
];

/// ヒステリシス: 押下は `PRESS` 超え、離すのは `RELEASE` 未満になってから。
// ponytail: 全アクション共通のしきい値。アクション毎に変えたくなったら Profile に持たせる。
const PRESS: f32 = 0.6;
const RELEASE: f32 = 0.4;

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
}

/// 逆引き: `profile` で `action` を起こす入力名。テストでの入力生成や操作ヒント表示用。
pub fn source(profile_id: &str, action: Action) -> Option<&'static str> {
    let p = profile(profile_id);
    match action {
        Action::Select => p.select,
        Action::Grab => p.grab,
        Action::Menu => p.menu,
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
    ControllerState {
        hand: raw.hand,
        aim: raw.aim,
        select: held(strongest(raw, p.select), was(|c| c.select)),
        grab: held(strongest(raw, p.grab), was(|c| c.grab)),
        menu: held(strongest(raw, p.menu), was(|c| c.menu)),
        scroll: p.scroll.map_or(Vec2::ZERO, |(x, y)| {
            Vec2::new(raw.input(x).unwrap_or(0.0), raw.input(y).unwrap_or(0.0))
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
}
