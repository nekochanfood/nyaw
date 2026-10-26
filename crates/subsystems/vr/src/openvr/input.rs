//! SteamVR Input（IVRInput）: action manifest と機種ごとの既定バインディングを書き出して登録し、手ごとのアクションを読む。
//! 旧来の `IVRSystem::GetControllerState` は、SteamVR Input が有効だと値が空（0）で返ることがあるので使わない。
//! バインディングは SteamVR の「コントローラーのバインド」画面で利用者が変えられる。

use std::ffi::{CString, c_char};
use std::mem::{MaybeUninit, size_of};

use openvr_bindings as sys;

use super::{f, pose_from};
use crate::{
    FingerTracking, Hand, HandSkeleton, JOINT_COUNT, PALM, Pose, Quat, Vec2, Vec3, VrError,
};

const FILES: [(&str, &str); 4] = [
    ("actions.json", include_str!("actions.json")),
    (
        "bindings_knuckles.json",
        include_str!("bindings_knuckles.json"),
    ),
    (
        "bindings_oculus_touch.json",
        include_str!("bindings_oculus_touch.json"),
    ),
    (
        "bindings_vive_controller.json",
        include_str!("bindings_vive_controller.json"),
    ),
];

/// 1 フレーム分の、片手のアクション。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct HandActions {
    pub select: bool,
    pub grab: bool,
    pub menu: bool,
    pub middle_touch: bool,
    pub right_touch: bool,
    /// ショートカットのダブルタップ（バインドが数える）。1 フレームだけ真。
    pub shortcut: bool,
    pub scroll: Vec2,
    /// 掴んでいるパネルの拡大縮小（-1..1）。既定のバインドは無く、SteamVR の画面で利用者が割り当てる。
    pub resize: f32,
}

pub(super) struct Input {
    table: *const sys::VR_IVRInput_FnTable,
    set: u64,
    aim: u64,
    select: u64,
    grab: u64,
    menu: u64,
    middle_touch: u64,
    right_touch: u64,
    shortcut: u64,
    scroll: u64,
    resize: u64,
    /// 手ごとの骨格（スケルタル入力）のアクション。
    skeletons: [(Hand, u64); 2],
    /// 手ごとの入力元（`/user/hand/left` 等）のハンドル。
    hands: [(Hand, u64); 2],
}

fn check(what: &str, e: sys::EVRInputError) -> Result<(), VrError> {
    if e == sys::EVRInputError_VRInputError_None {
        Ok(())
    } else {
        Err(VrError::Runtime(format!("{what}: EVRInputError {e}")))
    }
}

type GetHandle = unsafe extern "C" fn(*mut c_char, *mut u64) -> sys::EVRInputError;

fn handle(get: Option<GetHandle>, name: &str) -> Result<u64, VrError> {
    let c = CString::new(name).unwrap();
    let mut h = 0;
    check(name, unsafe { f(get)(c.as_ptr().cast_mut(), &mut h) })?;
    Ok(h)
}

impl Input {
    /// manifest を `os::config_dir()/openvr` に書き出して登録する。最初の `PollNextEvent` より前に呼ぶこと。
    pub fn init(table: *const sys::VR_IVRInput_FnTable) -> Result<Self, VrError> {
        let dir = os::config_dir()
            .ok_or_else(|| VrError::Runtime("config directory unknown".into()))?
            .join("openvr");
        for (name, body) in FILES {
            let path = dir.join(name);
            os::write_file(&path, body)
                .map_err(|e| VrError::Runtime(format!("{}: {e}", path.display())))?;
        }
        let t = unsafe { &*table };
        let manifest = CString::new(dir.join("actions.json").to_string_lossy().as_ref()).unwrap();
        check("SetActionManifestPath", unsafe {
            f(t.SetActionManifestPath)(manifest.as_ptr().cast_mut())
        })?;
        let action = |name| handle(t.GetActionHandle, name);
        Ok(Self {
            table,
            set: handle(t.GetActionSetHandle, "/actions/nyaw")?,
            aim: action("/actions/nyaw/in/aim")?,
            select: action("/actions/nyaw/in/select")?,
            grab: action("/actions/nyaw/in/grab")?,
            menu: action("/actions/nyaw/in/menu")?,
            middle_touch: action("/actions/nyaw/in/middle_touch")?,
            right_touch: action("/actions/nyaw/in/right_touch")?,
            shortcut: action("/actions/nyaw/in/shortcut")?,
            scroll: action("/actions/nyaw/in/scroll")?,
            resize: action("/actions/nyaw/in/resize")?,
            skeletons: [
                (Hand::Left, action("/actions/nyaw/in/skeleton_left")?),
                (Hand::Right, action("/actions/nyaw/in/skeleton_right")?),
            ],
            hands: [
                (
                    Hand::Left,
                    handle(t.GetInputSourceHandle, "/user/hand/left")?,
                ),
                (
                    Hand::Right,
                    handle(t.GetInputSourceHandle, "/user/hand/right")?,
                ),
            ],
        })
    }

    fn t(&self) -> &sys::VR_IVRInput_FnTable {
        unsafe { &*self.table }
    }

    /// このフレームのアクションの状態を取り込む。手ごとの値を読む前に毎フレーム呼ぶ。
    /// アクションの状態を読み直す。`exclusive` なら、オーバーレイの全体優先度で更新して、このアクションセットが使う入力を
    /// シーンのアプリ（ゲーム）から奪う。
    pub fn update(&self, exclusive: bool) -> Result<(), VrError> {
        let mut active = sys::VRActiveActionSet_t {
            ulActionSet: self.set,
            ulRestrictedToDevice: 0,
            ulSecondaryActionSet: 0,
            unPadding: 0,
            nPriority: if exclusive {
                sys::k_nActionSetOverlayGlobalPriorityMax
            } else {
                0
            },
        };
        check("UpdateActionState", unsafe {
            f(self.t().UpdateActionState)(
                &mut active,
                size_of::<sys::VRActiveActionSet_t>() as u32,
                1,
            )
        })
    }

    /// 見えている手ごとに（手, デバイス番号, レーザーの姿勢, アクション）。姿勢は `predict` 秒後の予測。
    pub fn hands(&self, predict: f32) -> Vec<(Hand, u32, Pose, HandActions)> {
        self.hands
            .iter()
            .filter_map(|&(hand, source)| {
                let (device, aim) = self.aim(source, predict)?;
                Some((hand, device, aim, self.actions(source)))
            })
            .collect()
    }

    /// 骨格が取れている手。関節は `predict` 秒後の予測のトラッキング空間で、並びは OpenXR に揃える（[`crate::JOINT_COUNT`]）。
    pub fn skeletons(&self, predict: f32) -> Vec<HandSkeleton> {
        self.skeletons
            .iter()
            .filter_map(|&(hand, action)| {
                // 骨格のアクションの姿勢は骨格の根（eBone_Root）。
                let root = self.pose(action, 0, predict)?.1;
                let bones = self.bones(action)?;
                let mut joints = [Pose::IDENTITY; JOINT_COUNT];
                // OpenVR の eBone_Wrist..eBone_PinkyFinger4（1〜25）は OpenXR の手首〜小指の先と同じ並び。
                for (joint, bone) in joints.iter_mut().zip(bones.iter()).skip(1) {
                    *joint = root * *bone;
                }
                // OpenVR に手のひらは無いので、中指の中手骨（11）と基節（12）の中点に中手骨の向きで置く。
                joints[PALM] = Pose::new(
                    joints[11].position.lerp(joints[12].position, 0.5),
                    joints[11].orientation,
                );
                Some(HandSkeleton {
                    hand,
                    joints,
                    tracking: self.tracking(action),
                })
            })
            .collect()
    }

    /// 骨格の指をどれだけ実際に追っているか。聞けなければ、コントローラを握っているとみなす。
    fn tracking(&self, action: u64) -> FingerTracking {
        let mut level = sys::EVRSkeletalTrackingLevel_VRSkeletalTracking_Partial;
        let e = unsafe { f(self.t().GetSkeletalTrackingLevel)(action, &mut level) };
        match level {
            _ if e != sys::EVRInputError_VRInputError_None => FingerTracking::Partial,
            sys::EVRSkeletalTrackingLevel_VRSkeletalTracking_Estimated => FingerTracking::Estimated,
            sys::EVRSkeletalTrackingLevel_VRSkeletalTracking_Full => FingerTracking::Full,
            _ => FingerTracking::Partial,
        }
    }

    /// 骨格の根から見たボーン（OpenVR の 31 本）の姿勢。コントローラを握った形に縛らない可動域（WithoutController）で、
    /// 握りこぶし・指を伸ばしきった形まで出る（握る・つまむの判定に要る）。
    fn bones(&self, action: u64) -> Option<[Pose; 31]> {
        let mut raw: [sys::VRBoneTransform_t; 31] = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(self.t().GetSkeletalBoneData)(
                action,
                sys::EVRSkeletalTransformSpace_VRSkeletalTransformSpace_Model,
                sys::EVRSkeletalMotionRange_VRSkeletalMotionRange_WithoutController,
                raw.as_mut_ptr(),
                raw.len() as u32,
            )
        };
        (e == sys::EVRInputError_VRInputError_None).then(|| {
            raw.map(|b| {
                let (p, q) = (b.position.v, b.orientation);
                Pose::new(
                    Vec3::new(p[0], p[1], p[2]),
                    Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize(),
                )
            })
        })
    }

    fn aim(&self, source: u64, predict: f32) -> Option<(u32, Pose)> {
        self.pose(self.aim, source, predict)
    }

    /// 姿勢のアクション（`source` = 0 で手を限らない）の（デバイス番号, 姿勢）。
    fn pose(&self, action: u64, source: u64, predict: f32) -> Option<(u32, Pose)> {
        let t = self.t();
        let mut data: sys::InputPoseActionData_t = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(t.GetPoseActionDataRelativeToNow)(
                action,
                sys::ETrackingUniverseOrigin_TrackingUniverseStanding,
                predict,
                &mut data,
                size_of::<sys::InputPoseActionData_t>() as u32,
                source,
            )
        };
        if e != sys::EVRInputError_VRInputError_None || !data.bActive || !data.pose.bPoseIsValid {
            return None;
        }
        let mut info: sys::InputOriginInfo_t = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(t.GetOriginTrackedDeviceInfo)(
                data.activeOrigin,
                &mut info,
                size_of::<sys::InputOriginInfo_t>() as u32,
            )
        };
        (e == sys::EVRInputError_VRInputError_None).then(|| {
            (
                info.trackedDeviceIndex,
                pose_from(&data.pose.mDeviceToAbsoluteTracking.m),
            )
        })
    }

    fn actions(&self, source: u64) -> HandActions {
        let t = self.t();
        // `pulse` は押し続けても状態が変わった 1 フレームだけ真（ダブルタップのバインドの出力）。
        let digital = |action, pulse: bool| {
            let mut d: sys::InputDigitalActionData_t =
                unsafe { MaybeUninit::zeroed().assume_init() };
            let e = unsafe {
                f(t.GetDigitalActionData)(
                    action,
                    &mut d,
                    size_of::<sys::InputDigitalActionData_t>() as u32,
                    source,
                )
            };
            e == sys::EVRInputError_VRInputError_None
                && d.bActive
                && d.bState
                && (!pulse || d.bChanged)
        };
        let mut a: sys::InputAnalogActionData_t = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(t.GetAnalogActionData)(
                self.scroll,
                &mut a,
                size_of::<sys::InputAnalogActionData_t>() as u32,
                source,
            )
        };
        let scroll = if e == sys::EVRInputError_VRInputError_None && a.bActive {
            Vec2::new(a.x, a.y)
        } else {
            Vec2::ZERO
        };
        let mut r: sys::InputAnalogActionData_t = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(t.GetAnalogActionData)(
                self.resize,
                &mut r,
                size_of::<sys::InputAnalogActionData_t>() as u32,
                source,
            )
        };
        let resize = if e == sys::EVRInputError_VRInputError_None && r.bActive {
            r.x
        } else {
            0.0
        };
        HandActions {
            select: digital(self.select, false),
            grab: digital(self.grab, false),
            menu: digital(self.menu, false),
            middle_touch: digital(self.middle_touch, false),
            right_touch: digital(self.right_touch, false),
            shortcut: digital(self.shortcut, true),
            scroll,
            resize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON の中の `"output"` を全部集める（sources・poses・skeleton のどこにあっても）。
    fn outputs(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    match x.as_str() {
                        Some(s) if k == "output" => out.push(s.to_owned()),
                        _ => outputs(x, out),
                    }
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| outputs(x, out)),
            _ => {}
        }
    }

    fn json(body: &str) -> serde_json::Value {
        serde_json::from_str(body).expect("valid JSON")
    }

    /// 既定バインディングは manifest に載っていて、出力先はどれも manifest のアクション。
    /// 利用者が割り当てる `resize` 以外の入力アクションは、どの機種でも最低限バインドされている。
    #[test]
    fn bindings_match_manifest() {
        let manifest = json(FILES[0].1);
        let names: Vec<&str> = manifest["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        let listed = manifest["default_bindings"].to_string();
        for (name, body) in &FILES[1..] {
            assert!(listed.contains(name), "{name} missing from default_bindings");
            let mut bound = Vec::new();
            outputs(&json(body), &mut bound);
            for out in &bound {
                assert!(names.contains(&out.as_str()), "{name}: {out}");
            }
            for need in ["aim", "select", "grab", "menu", "scroll"] {
                let full = format!("/actions/nyaw/in/{need}");
                assert!(bound.contains(&full), "{name}: {need} unbound");
            }
        }
    }

    /// SteamVR のバインド画面に出る名前が、どのアクションにもある。
    #[test]
    fn every_action_is_localized() {
        let manifest = json(FILES[0].1);
        let loc = &manifest["localization"][0];
        for a in manifest["actions"].as_array().unwrap() {
            let n = a["name"].as_str().unwrap();
            assert!(loc[n].is_string(), "{n} has no label");
        }
    }
}
