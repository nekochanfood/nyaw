//! SteamVR Input（IVRInput）: action manifest と機種ごとの既定バインディングを書き出して登録し、手ごとのアクションを読む。
//! 旧来の `IVRSystem::GetControllerState` は、SteamVR Input が有効だと値が空（0）で返ることがあるので使わない。
//! バインディングは SteamVR の「コントローラーのバインド」画面で利用者が変えられる。

use std::ffi::{CString, c_char};
use std::mem::{MaybeUninit, size_of};

use openvr_bindings as sys;

use super::{f, pose_from};
use crate::{Hand, Pose, Vec2, VrError};

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
    pub scroll: Vec2,
}

pub(super) struct Input {
    table: *const sys::VR_IVRInput_FnTable,
    set: u64,
    aim: u64,
    select: u64,
    grab: u64,
    menu: u64,
    scroll: u64,
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
            .ok_or_else(|| VrError::Runtime("設定ディレクトリが分からない".into()))?
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
            scroll: action("/actions/nyaw/in/scroll")?,
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
    pub fn update(&self) -> Result<(), VrError> {
        let mut active = sys::VRActiveActionSet_t {
            ulActionSet: self.set,
            ulRestrictedToDevice: 0,
            ulSecondaryActionSet: 0,
            unPadding: 0,
            nPriority: 0,
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
    // ponytail: openvr_bindings の構造体は Windows の詰め物で定義されている。Linux で大きさが合わず
    // エラーになるなら、ControllerState と同じく自前で定義し直す。
    pub fn hands(&self, predict: f32) -> Vec<(Hand, u32, Pose, HandActions)> {
        self.hands
            .iter()
            .filter_map(|&(hand, source)| {
                let (device, aim) = self.aim(source, predict)?;
                Some((hand, device, aim, self.actions(source)))
            })
            .collect()
    }

    fn aim(&self, source: u64, predict: f32) -> Option<(u32, Pose)> {
        let t = self.t();
        let mut data: sys::InputPoseActionData_t = unsafe { MaybeUninit::zeroed().assume_init() };
        let e = unsafe {
            f(t.GetPoseActionDataRelativeToNow)(
                self.aim,
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
        let digital = |action| {
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
            e == sys::EVRInputError_VRInputError_None && d.bActive && d.bState
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
        HandActions {
            select: digital(self.select),
            grab: digital(self.grab),
            menu: digital(self.menu),
            scroll,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 既定バインディングは manifest に載っていて、出力先はどれも manifest のアクション。
    #[test]
    fn bindings_match_manifest() {
        let manifest = FILES[0].1;
        for (name, body) in &FILES[1..] {
            assert!(manifest.contains(name), "{name} が default_bindings に無い");
            for out in body.split("\"output\": \"").skip(1) {
                let action = &out[..out.find('"').unwrap()];
                assert!(
                    manifest.contains(&format!("\"name\": \"{action}\"")),
                    "{name}: {action}"
                );
            }
        }
    }
}
