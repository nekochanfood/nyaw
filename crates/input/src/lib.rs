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

/// アクションごとの入力名（OpenXR の語彙）。デバイスが報告した最初のものを採用する。
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
        select: &["trigger/value"],
        grab: &["squeeze/value"],
        menu: &["b/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "oculus/touch_controller",
        select: &["trigger/value"],
        grab: &["squeeze/value"],
        menu: &["menu/click", "y/click", "b/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "htc/vive_controller",
        select: &["trigger/value"],
        grab: &["squeeze/click"],
        menu: &["menu/click"],
        scroll: Some(("trackpad/x", "trackpad/y")),
    },
];

// ponytail: 固定しきい値でヒステリシスなし。トリガーがチャタリングするならアクション毎にヒステリシスを追加。
const PRESS: f32 = 0.5;

fn first(raw: &RawController, names: &[&str]) -> f32 {
    names.iter().find_map(|n| raw.input(n)).unwrap_or(0.0)
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

pub fn map(raw: &RawController) -> ControllerState {
    let p = profile(&raw.profile);
    ControllerState {
        hand: raw.hand,
        aim: raw.aim,
        select: first(raw, p.select) > PRESS,
        grab: first(raw, p.grab) > PRESS,
        menu: first(raw, p.menu) > PRESS,
        scroll: p.scroll.map_or(Vec2::ZERO, |(x, y)| {
            Vec2::new(first(raw, &[x]), first(raw, &[y]))
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
    fn different_devices_same_action() {
        let index = map(&raw(
            "valve/index_controller",
            &[("trigger/value", 0.9), ("thumbstick/y", 1.0)],
        ));
        let simple = map(&raw("khr/simple_controller", &[("select/click", 1.0)]));
        assert!(index.select && simple.select);
        assert_eq!(index.scroll, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn touch_menu_candidates_and_threshold() {
        let s = map(&raw(
            "oculus/touch_controller",
            &[("b/click", 1.0), ("squeeze/value", 0.3)],
        ));
        assert!(s.menu);
        assert!(!s.grab);
    }

    #[test]
    fn source_roundtrips_through_map() {
        for id in PROFILES.iter().map(|p| p.id) {
            let name = source(id, Action::Select).unwrap();
            assert!(map(&raw(id, &[(name, 1.0)])).select, "{id}");
        }
        assert_eq!(source("khr/simple_controller", Action::Grab), None);
    }

    #[test]
    fn unknown_device_falls_back_to_simple() {
        assert!(map(&raw("acme/mystery", &[("select/click", 1.0)])).select);
    }
}
