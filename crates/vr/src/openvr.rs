//! OpenVR (SteamVR) バックエンド: オーバーレイは `IVROverlay`、入力は `IVRSystem` のコントローラ状態。
//! C++ の vtable ではなく C API の関数表（`FnTable:IVRSystem_022` 等）を通して呼ぶ。

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{MaybeUninit, size_of};

use glam::Mat3;
use openvr_bindings as sys;

use crate::{
    Frame, Hand, OverlayId, Pose, Quat, RawController, Runtime, Surface, Vec3, VrBackend, VrError,
};

pub(crate) fn available() -> bool {
    os::openvr_paths_file().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    let mut err = sys::EVRInitError_VRInitError_None;
    unsafe { sys::VR_InitInternal(&mut err, sys::EVRApplicationType_VRApplication_Overlay) };
    if err != sys::EVRInitError_VRInitError_None {
        return Err(init_error(err));
    }
    // ここから先で失敗しても Drop が VR_ShutdownInternal を呼ぶ。
    let mut backend = OpenVrBackend {
        system: std::ptr::null(),
        overlay: std::ptr::null(),
        sent: HashMap::new(),
    };
    backend.system = interface(sys::IVRSystem_Version)?;
    backend.overlay = interface(sys::IVROverlay_Version)?;
    Ok(Box::new(backend))
}

fn init_error(err: sys::EVRInitError) -> VrError {
    let msg = unsafe { CStr::from_ptr(sys::VR_GetVRInitErrorAsEnglishDescription(err)) };
    VrError::Runtime(format!("VR_Init: {}", msg.to_string_lossy()))
}

/// `version` は `b"IVRSystem_022\0"` のようなバインディングの定数。C API の関数表を得る。
fn interface<T>(version: &[u8]) -> Result<*const T, VrError> {
    let version = CStr::from_bytes_with_nul(version)
        .unwrap()
        .to_str()
        .unwrap();
    let name = CString::new(format!("FnTable:{version}")).unwrap();
    let mut err = sys::EVRInitError_VRInitError_None;
    let table = unsafe { sys::VR_GetGenericInterface(name.as_ptr(), &mut err) };
    if err != sys::EVRInitError_VRInitError_None || table == 0 {
        return Err(init_error(err));
    }
    Ok(table as *const T)
}

/// 関数表の各項目は `Option<fn>`。ランタイムが埋めていなければ ABI の不一致なので落とす。
fn f<T>(func: Option<T>) -> T {
    func.expect("OpenVR の関数表に欠けている関数がある")
}

/// `VRControllerState_t`。Linux では詰め物 4 バイトで定義されているが、
/// openvr_bindings はそれを反映していない（Windows で 64 バイト、Linux で 60 バイト）ので自前で持つ。
#[cfg_attr(unix, repr(C, packed(4)))]
#[cfg_attr(not(unix), repr(C))]
#[derive(Clone, Copy, Default)]
struct ControllerState {
    packet: u32,
    pressed: u64,
    touched: u64,
    axes: [[f32; 2]; 5],
}

const MAX_DEVICES: usize = sys::k_unMaxTrackedDeviceCount as usize;

pub(crate) struct OpenVrBackend {
    system: *const sys::VR_IVRSystem_FnTable,
    overlay: *const sys::VR_IVROverlay_FnTable,
    /// オーバーレイごとに最後に送った画像（幅, 高さ, RGBA）。同じ画像は送り直さない。
    sent: HashMap<u64, (u32, u32, Vec<u8>)>,
}

impl OpenVrBackend {
    fn system(&self) -> &sys::VR_IVRSystem_FnTable {
        unsafe { &*self.system }
    }

    fn overlay(&self) -> &sys::VR_IVROverlay_FnTable {
        unsafe { &*self.overlay }
    }

    fn string_property(&self, device: u32, prop: sys::ETrackedDeviceProperty) -> String {
        let mut buf = [0 as c_char; 256];
        let mut err = 0;
        unsafe {
            f(self.system().GetStringTrackedDeviceProperty)(
                device,
                prop,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut err,
            );
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        }
    }

    fn controller(&self, device: u32, pose: &sys::TrackedDevicePose_t) -> Option<RawController> {
        let s = self.system();
        let hand = match unsafe { f(s.GetControllerRoleForTrackedDeviceIndex)(device) } {
            sys::ETrackedControllerRole_TrackedControllerRole_LeftHand => Hand::Left,
            sys::ETrackedControllerRole_TrackedControllerRole_RightHand => Hand::Right,
            _ => return None,
        };
        let mut state = ControllerState::default();
        let ok = unsafe {
            f(s.GetControllerState)(
                device,
                &mut state as *mut ControllerState as *mut sys::VRControllerState_t,
                size_of::<ControllerState>() as u32,
            )
        };
        if !ok {
            return None;
        }
        let mut axis_types = [0; 5];
        for (n, t) in axis_types.iter_mut().enumerate() {
            let mut err = 0;
            let prop = sys::ETrackedDeviceProperty_Prop_Axis0Type_Int32 + n as u32;
            *t = unsafe { f(s.GetInt32TrackedDeviceProperty)(device, prop, &mut err) };
        }
        let profile = profile(&self.string_property(
            device,
            sys::ETrackedDeviceProperty_Prop_ControllerType_String,
        ));
        Some(RawController {
            hand,
            profile: profile.into(),
            // ponytail: トラッキング姿勢をそのままレーザーに使う。Index は前方が下向きに傾くので、
            // ずれが気になったら IVRInput のポーズアクション（/pose/aim）に切り替える。
            aim: pose_from(&pose.mDeviceToAbsoluteTracking.m),
            inputs: inputs(profile, state.pressed, axis_types, state.axes),
        })
    }
}

impl Drop for OpenVrBackend {
    fn drop(&mut self) {
        unsafe { sys::VR_ShutdownInternal() };
    }
}

impl VrBackend for OpenVrBackend {
    fn runtime(&self) -> Runtime {
        Runtime::OpenVr
    }

    fn surface(&self) -> Surface {
        Surface::Overlay
    }

    fn poll(&mut self) -> Frame {
        let s = self.system();
        // ponytail: イベントは読み捨てる。VREvent_Quit で自分から終了しないので、
        // SteamVR 終了時は数秒後に強制終了される。きれいに閉じたくなったら Frame に終了フラグを足す。
        let mut event = MaybeUninit::<sys::VREvent_t>::zeroed();
        while unsafe { f(s.PollNextEvent)(event.as_mut_ptr(), size_of::<sys::VREvent_t>() as u32) }
        {
        }

        let mut poses: [sys::TrackedDevicePose_t; MAX_DEVICES] =
            unsafe { MaybeUninit::zeroed().assume_init() };
        unsafe {
            f(s.GetDeviceToAbsoluteTrackingPose)(
                sys::ETrackingUniverseOrigin_TrackingUniverseStanding,
                0.0,
                poses.as_mut_ptr(),
                MAX_DEVICES as u32,
            )
        };

        let mut frame = Frame::default();
        let hmd = &poses[sys::k_unTrackedDeviceIndex_Hmd as usize];
        if hmd.bPoseIsValid {
            frame.hmd = pose_from(&hmd.mDeviceToAbsoluteTracking.m);
        }
        for (device, pose) in poses.iter().enumerate().skip(1) {
            let device = device as u32;
            if !pose.bPoseIsValid
                || unsafe { f(s.GetTrackedDeviceClass)(device) }
                    != sys::ETrackedDeviceClass_TrackedDeviceClass_Controller
            {
                continue;
            }
            frame.controllers.extend(self.controller(device, pose));
        }
        frame
    }

    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        let o = self.overlay();
        let key = CString::new(key).map_err(|e| VrError::Runtime(e.to_string()))?;
        let mut handle = 0;
        unsafe {
            check(
                "CreateOverlay",
                // 名前（ダッシュボードに出る表示名）もキーで済ませる。
                f(o.CreateOverlay)(
                    key.as_ptr().cast_mut(),
                    key.as_ptr().cast_mut(),
                    &mut handle,
                ),
            )?;
            check(
                "SetOverlayWidthInMeters",
                f(o.SetOverlayWidthInMeters)(handle, width_m),
            )?;
            check("ShowOverlay", f(o.ShowOverlay)(handle))?;
        }
        // OpenVR のハンドルをそのまま ID にする。
        Ok(OverlayId(handle))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        let mut m = sys::HmdMatrix34_t {
            m: matrix_from(pose),
        };
        let e = unsafe {
            f(self.overlay().SetOverlayTransformAbsolute)(
                id.0,
                sys::ETrackingUniverseOrigin_TrackingUniverseStanding,
                &mut m,
            )
        };
        log(check("SetOverlayTransformAbsolute", e));
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "RGBA バッファのサイズが不正"
        );
        if self
            .sent
            .get(&id.0)
            .is_some_and(|(w, h, prev)| (*w, *h) == (width, height) && prev == rgba)
        {
            return;
        }
        // ponytail: SetOverlayRaw は呼ぶたびに共有メモリのブロックを確保し、コンポジタが取り込むまで解放されない
        // （約 200 個で RequestFailed になる。null ドライバでは 10Hz で送り続けても溜まっていった）。
        // 同じ画像を送らないことで抑えているが、絵が変わり続けると詰まりうる。
        // そうなったら SetOverlayTexture で GPU テクスチャを直接渡す。
        let e = unsafe {
            f(self.overlay().SetOverlayRaw)(
                id.0,
                // 読むだけで書き換えない（API の型が *mut なだけ）。
                rgba.as_ptr().cast_mut() as *mut c_void,
                width,
                height,
                4,
            )
        };
        let r = check("SetOverlayRaw", e);
        if r.is_ok() {
            self.sent.insert(id.0, (width, height, rgba.to_vec()));
        }
        log(r);
    }
}

fn check(what: &str, e: sys::EVROverlayError) -> Result<(), VrError> {
    if e == sys::EVROverlayError_VROverlayError_None {
        Ok(())
    } else {
        Err(VrError::Runtime(format!("{what}: EVROverlayError {e}")))
    }
}

/// 戻り値でエラーを返せないメソッド用。フレームは続けたいので表示だけする。
fn log(r: Result<(), VrError>) {
    if let Err(e) = r {
        eprintln!("{e}");
    }
}

/// `Prop_ControllerType_String` を OpenXR のインタラクションプロファイル名に翻訳する。
fn profile(controller_type: &str) -> &'static str {
    match controller_type {
        "knuckles" => "valve/index_controller",
        "oculus_touch" => "oculus/touch_controller",
        "vive_controller" => "htc/vive_controller",
        _ => "khr/simple_controller",
    }
}

fn button(pressed: u64, id: sys::EVRButtonId) -> f32 {
    if pressed & (1 << id) != 0 { 1.0 } else { 0.0 }
}

/// 旧来のコントローラ状態（ボタンのビット列と軸 5 本）を OpenXR の入力名に翻訳する。
/// 軸の意味はデバイスごとに `Prop_Axis{n}Type_Int32` で宣言されている。
fn inputs(
    profile: &str,
    pressed: u64,
    axis_types: [i32; 5],
    axes: [[f32; 2]; 5],
) -> Vec<(String, f32)> {
    let mut v = Vec::new();
    let mut put = |name: &str, value: f32| v.push((name.to_owned(), value));

    let mut seen = [false; 3];
    for (t, [x, y]) in axis_types.into_iter().zip(axes) {
        // 同じ種類の軸が複数あるときは最初のものだけ使う。
        match t as u32 {
            sys::EVRControllerAxisType_k_eControllerAxis_Trigger if !seen[0] => {
                seen[0] = true;
                put("trigger/value", x);
            }
            sys::EVRControllerAxisType_k_eControllerAxis_Joystick if !seen[1] => {
                seen[1] = true;
                put("thumbstick/x", x);
                put("thumbstick/y", y);
            }
            sys::EVRControllerAxisType_k_eControllerAxis_TrackPad if !seen[2] => {
                seen[2] = true;
                put("trackpad/x", x);
                put("trackpad/y", y);
            }
            _ => {}
        }
    }

    put(
        "select/click",
        button(pressed, sys::EVRButtonId_k_EButton_SteamVR_Trigger),
    );
    let grip = button(pressed, sys::EVRButtonId_k_EButton_Grip);
    put("squeeze/value", grip);
    put("squeeze/click", grip);
    // 旧来の ApplicationMenu は、Index では B ボタン、Touch では B / Y ボタンに当たる。
    let menu = if profile == "valve/index_controller" {
        "b/click"
    } else {
        "menu/click"
    };
    put(
        menu,
        button(pressed, sys::EVRButtonId_k_EButton_ApplicationMenu),
    );
    v
}

/// `HmdMatrix34_t`（行優先の 3x4。左 3 列が回転、右端の列が平行移動）から姿勢へ。
fn pose_from(m: &[[f32; 4]; 3]) -> Pose {
    let col = |c: usize| Vec3::new(m[0][c], m[1][c], m[2][c]);
    let rotation = Mat3::from_cols(col(0), col(1), col(2));
    Pose::new(col(3), Quat::from_mat3(&rotation).normalize())
}

fn matrix_from(pose: Pose) -> [[f32; 4]; 3] {
    let r = Mat3::from_quat(pose.orientation);
    let p = pose.position;
    std::array::from_fn(|row| {
        let r = r.row(row);
        [r.x, r.y, r.z, p[row]]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(v: &[(String, f32)], name: &str) -> Option<f32> {
        v.iter().find(|(n, _)| n == name).map(|(_, x)| *x)
    }

    #[test]
    fn matrix_roundtrip() {
        let pose = Pose::new(
            Vec3::new(0.3, 1.5, -0.8),
            Quat::from_rotation_y(0.9) * Quat::from_rotation_x(-0.4),
        );
        let back = pose_from(&matrix_from(pose));
        assert!(back.position.abs_diff_eq(pose.position, 1e-5));
        assert!(back.orientation.dot(pose.orientation).abs() > 1.0 - 1e-5);
    }

    #[test]
    fn matrix_layout_is_row_major_with_translation_last() {
        // Y 軸まわり 90°: ローカル -Z（前方）がワールド -X を向く。
        let m = matrix_from(Pose::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        ));
        assert_eq!([m[0][3], m[1][3], m[2][3]], [1.0, 2.0, 3.0]);
        assert!(pose_from(&m).forward().abs_diff_eq(Vec3::NEG_X, 1e-5));
    }

    #[test]
    fn controller_types_map_to_openxr_profiles() {
        assert_eq!(profile("knuckles"), "valve/index_controller");
        assert_eq!(profile("oculus_touch"), "oculus/touch_controller");
        assert_eq!(profile("vive_controller"), "htc/vive_controller");
        assert_eq!(profile("unknown_thing"), "khr/simple_controller");
    }

    #[test]
    fn legacy_state_maps_to_openxr_inputs() {
        let axis_types = [
            sys::EVRControllerAxisType_k_eControllerAxis_Joystick as i32,
            sys::EVRControllerAxisType_k_eControllerAxis_Trigger as i32,
            sys::EVRControllerAxisType_k_eControllerAxis_Trigger as i32,
            0,
            0,
        ];
        let axes = [[0.1, -0.2], [0.8, 0.0], [0.3, 0.0], [0.0; 2], [0.0; 2]];
        let pressed = (1 << sys::EVRButtonId_k_EButton_Grip)
            | (1 << sys::EVRButtonId_k_EButton_ApplicationMenu);
        let v = inputs("valve/index_controller", pressed, axis_types, axes);
        assert_eq!(value(&v, "thumbstick/x"), Some(0.1));
        assert_eq!(value(&v, "thumbstick/y"), Some(-0.2));
        // 同じ種類の 2 本目の軸は無視する。
        assert_eq!(value(&v, "trigger/value"), Some(0.8));
        assert_eq!(value(&v, "squeeze/value"), Some(1.0));
        assert_eq!(value(&v, "b/click"), Some(1.0));
        assert_eq!(value(&v, "menu/click"), None);
        assert_eq!(value(&v, "select/click"), Some(0.0));
    }

    #[test]
    fn vive_menu_is_menu_click() {
        let pressed = 1 << sys::EVRButtonId_k_EButton_ApplicationMenu;
        let v = inputs("htc/vive_controller", pressed, [0; 5], [[0.0; 2]; 5]);
        assert_eq!(value(&v, "menu/click"), Some(1.0));
    }

    #[test]
    fn controller_state_matches_openvr_layout() {
        let expected = if cfg!(unix) { 60 } else { 64 };
        assert_eq!(size_of::<ControllerState>(), expected);
    }
}
