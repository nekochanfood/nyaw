//! OpenVR (SteamVR) バックエンド: オーバーレイは `IVROverlay`、入力は `IVRSystem` のコントローラ状態。
//! C++ の vtable ではなく C API の関数表（`FnTable:IVRSystem_022` 等）を通して呼ぶ。

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{MaybeUninit, size_of};

use ash::vk::{self, Handle};
use glam::Mat3;
use openvr_bindings as sys;

mod client;

use client::Client;

use crate::gpu::{self, Gpu, vk_err};
use crate::{
    Frame, Hand, OverlayId, Pose, Quat, RawController, Runtime, Surface, Vec3, VrBackend, VrError,
};

pub(crate) fn available() -> bool {
    os::openvr_paths_file().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    let client = Client::init(sys::EVRApplicationType_VRApplication_Overlay)?;
    let system = client.interface(sys::IVRSystem_Version)?;
    let compositor = client.interface(sys::IVRCompositor_Version)?;
    Ok(Box::new(OpenVrBackend {
        system,
        overlay: client.interface(sys::IVROverlay_Version)?,
        textures: HashMap::new(),
        gpu: create_gpu(unsafe { &*system }, unsafe { &*compositor })?,
        client,
    }))
}

/// SteamVR が要求する拡張を有効にし、HMD が繋がっている GPU で Vulkan のデバイスを作る。
fn create_gpu(
    system: &sys::VR_IVRSystem_FnTable,
    compositor: &sys::VR_IVRCompositor_FnTable,
) -> Result<Gpu, VrError> {
    let entry = gpu::entry()?;
    let mut buf = [0 as c_char; 4096];
    unsafe {
        f(compositor.GetVulkanInstanceExtensionsRequired)(buf.as_mut_ptr(), buf.len() as u32)
    };
    let instance = gpu::create_instance(
        &entry,
        &gpu::extension_names(&gpu::c_buf_to_string(&buf)),
        |info| {
            unsafe { entry.create_instance(info, None) }
                .map(|i| i.handle())
                .map_err(vk_err("vkCreateInstance"))
        },
    )?;
    let mut physical = 0u64;
    unsafe {
        f(system.GetOutputDevice)(
            &mut physical,
            sys::ETextureType_TextureType_Vulkan,
            instance.handle().as_raw() as *mut sys::VkInstance_T,
        )
    };
    let physical = match physical {
        // 分からなければ最初の GPU にする。
        0 => *unsafe { instance.enumerate_physical_devices() }
            .map_err(vk_err("vkEnumeratePhysicalDevices"))?
            .first()
            .ok_or_else(|| VrError::Runtime("Vulkan: GPU が無い".into()))?,
        raw => vk::PhysicalDevice::from_raw(raw),
    };
    unsafe {
        f(compositor.GetVulkanDeviceExtensionsRequired)(
            physical.as_raw() as *mut sys::VkPhysicalDevice_T,
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    let extensions = gpu::extension_names(&gpu::c_buf_to_string(&buf));
    gpu::create_device(entry, instance, physical, &extensions, |instance, info| {
        unsafe { instance.create_device(physical, info, None) }
            .map(|d| d.handle())
            .map_err(vk_err("vkCreateDevice"))
    })
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

/// オーバーレイ 1 枚あたりのイメージの数。コンポジタが読んでいる最中のイメージに書き込まないよう順番に使う。
// ponytail: 3 枚あれば足りる想定。絵がちらつく・崩れるなら増やすか、送ったイメージの使用完了を待つ。
const RING: usize = 3;

struct Textures {
    width: u32,
    height: u32,
    images: Vec<gpu::Image>,
    next: usize,
}

pub(crate) struct OpenVrBackend {
    system: *const sys::VR_IVRSystem_FnTable,
    overlay: *const sys::VR_IVROverlay_FnTable,
    textures: HashMap<u64, Textures>,
    gpu: Gpu,
    /// SteamVR との接続。関数表の持ち主なので最後に破棄する。
    client: Client,
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

    fn release(&mut self, id: u64) {
        if let Some(old) = self.textures.remove(&id) {
            self.gpu.wait_idle();
            for image in old.images {
                self.gpu.destroy_image(image);
            }
        }
    }

    fn submit(&mut self, id: u64, width: u32, height: u32, rgba: &[u8]) -> Result<(), VrError> {
        if self
            .textures
            .get(&id)
            .is_none_or(|t| (t.width, t.height) != (width, height))
        {
            self.release(id);
            let usage = vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::SAMPLED;
            let images = (0..RING)
                .map(|_| self.gpu.create_image(width, height, usage))
                .collect::<Result<_, _>>()?;
            self.textures.insert(
                id,
                Textures {
                    width,
                    height,
                    images,
                    next: 0,
                },
            );
        }
        let t = self.textures.get_mut(&id).unwrap();
        let image = t.images[t.next].image;
        t.next = (t.next + 1) % RING;
        // SteamVR は Vulkan のテクスチャを TRANSFER_SRC_OPTIMAL で受け取る。
        self.gpu.upload(
            image,
            width,
            height,
            rgba,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        )?;
        let g = &self.gpu;
        let mut data = sys::VRVulkanTextureData_t {
            m_nImage: image.as_raw(),
            m_pDevice: g.device.handle().as_raw() as *mut _,
            m_pPhysicalDevice: g.physical.as_raw() as *mut _,
            m_pInstance: g.instance.handle().as_raw() as *mut _,
            m_pQueue: g.queue.as_raw() as *mut _,
            m_nQueueFamilyIndex: g.queue_family,
            m_nWidth: width,
            m_nHeight: height,
            m_nFormat: gpu::FORMAT.as_raw() as u32,
            m_nSampleCount: 1,
        };
        let mut texture = sys::Texture_t {
            handle: &mut data as *mut _ as *mut c_void,
            eType: sys::ETextureType_TextureType_Vulkan,
            eColorSpace: sys::EColorSpace_ColorSpace_Auto,
        };
        check("SetOverlayTexture", unsafe {
            f(self.overlay().SetOverlayTexture)(id, &mut texture)
        })
    }
}

impl Drop for OpenVrBackend {
    fn drop(&mut self) {
        for id in self.textures.keys().copied().collect::<Vec<_>>() {
            self.release(id);
        }
        self.client.shutdown();
        // この後、フィールドの gpu が Vulkan のデバイスを破棄する。
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
        log(self.submit(id.0, width, height, rgba));
    }
}

/// 疎通確認用: オーバーレイが今ランタイム側で持っている画像（幅, 高さ, RGBA）を読み戻す。
/// 同じプロセスで OpenVR に接続済みであること。
pub fn overlay_image(id: OverlayId) -> Result<(u32, u32, Vec<u8>), VrError> {
    let client = Client::load()?;
    let overlay =
        unsafe { &*client.interface::<sys::VR_IVROverlay_FnTable>(sys::IVROverlay_Version)? };
    let (mut w, mut h) = (0, 0);
    // まず大きさだけ問い合わせる（バッファが小さいと ArrayTooSmall で大きさが返る）。
    unsafe { f(overlay.GetOverlayImageData)(id.0, std::ptr::null_mut(), 0, &mut w, &mut h) };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    check("GetOverlayImageData", unsafe {
        f(overlay.GetOverlayImageData)(
            id.0,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() as u32,
            &mut w,
            &mut h,
        )
    })?;
    Ok((w, h, buf))
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
