//! OpenVR (SteamVR) バックエンド: オーバーレイは `IVROverlay`、入力は SteamVR Input（`IVRInput`、[`input`]）。
//! C++ の vtable ではなく C API の関数表（`FnTable:IVRSystem_022` 等）を通して呼ぶ。

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{MaybeUninit, size_of};

use ash::vk::{self, Handle};
use glam::Mat3;
use openvr_bindings as sys;

mod client;
mod input;

use client::Client;
pub use client::runtime_path;
use input::{HandActions, Input};

use crate::gpu::{self, Gpu, vk_err};
use crate::{
    Frame, OverlayId, Pose, Quat, RawController, Runtime, Surface, Vec3, VrBackend, VrError,
};

pub(crate) fn available() -> bool {
    os::openvr_paths_file().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    let client = Client::init(sys::EVRApplicationType_VRApplication_Overlay)?;
    let system = client.interface(sys::IVRSystem_Version)?;
    let compositor = client.interface(sys::IVRCompositor_Version)?;
    // manifest の登録は最初の PollNextEvent より前でなければならない。
    let input = Input::init(client.interface(sys::IVRInput_Version)?)
        .inspect_err(|e| {
            logger::warn!(
                "SteamVR Input を使えないのでコントローラは無しで動く",
                error = e
            )
        })
        .ok();
    Ok(Box::new(OpenVrBackend {
        system,
        input,
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
    /// 使えなければ（manifest を登録できない等）コントローラは無し。
    input: Option<Input>,
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

    fn hmd_float(&self, prop: sys::ETrackedDeviceProperty) -> f32 {
        let mut err = Default::default();
        unsafe {
            f(self.system().GetFloatTrackedDeviceProperty)(
                sys::k_unTrackedDeviceIndex_Hmd as _,
                prop,
                &mut err,
            )
        }
    }

    /// 次の vsync まで眠り、このフレームで送る姿勢が表示されるまでの秒数（姿勢の予測に使う）を返す。
    /// オーバーレイアプリは WaitGetPoses でフレームを進められないので、直前の vsync からの経過時間と
    /// 表示周波数から残りを計算して眠る。
    /// 経過時間が 1 フレームを超えて返るドライバ（Quest 系で実測）があるので、周期で割った余りを使う。
    /// そうしないと待たずに空回りし、毎回全パネルを描き直してフレームレートを落とす。
    fn wait_vsync(&self) -> f32 {
        let hz = self.hmd_float(sys::ETrackedDeviceProperty_Prop_DisplayFrequency_Float);
        // ponytail: 周波数が取れないときは 90Hz とみなす。
        let period = if hz > 0.0 { 1.0 / hz } else { 1.0 / 90.0 };
        let (mut since, mut counter) = (0.0f32, Default::default());
        let synced = unsafe { f(self.system().GetTimeSinceLastVsync)(&mut since, &mut counter) };
        let left = if synced {
            period - since.rem_euclid(period)
        } else {
            period
        };
        std::thread::sleep(std::time::Duration::from_secs_f32(left));
        // ここから描画して送ったものは、コンポジタの次のフレームで表示される。
        period + self.hmd_float(sys::ETrackedDeviceProperty_Prop_SecondsFromVsyncToPhotons_Float)
    }

    fn overlay(&self) -> &sys::VR_IVROverlay_FnTable {
        unsafe { &*self.overlay }
    }

    fn string_property(&self, device: u32, prop: sys::ETrackedDeviceProperty) -> String {
        let mut buf = [0 as c_char; 256];
        let mut err = Default::default();
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
        let predict = self.wait_vsync();
        let s = self.system();
        let mut quit = false;
        let mut event = MaybeUninit::<sys::VREvent_t>::zeroed();
        while unsafe { f(s.PollNextEvent)(event.as_mut_ptr(), size_of::<sys::VREvent_t>() as u32) }
        {
            quit |= unsafe { event.assume_init_ref() }.eventType == sys::EVREventType_VREvent_Quit;
        }

        let mut poses: [sys::TrackedDevicePose_t; MAX_DEVICES] =
            unsafe { MaybeUninit::zeroed().assume_init() };
        // 表示される時刻の姿勢を予測させる（0 だと今の姿勢で、表示時には 1〜2 フレーム遅れて見える）。
        unsafe {
            f(s.GetDeviceToAbsoluteTrackingPose)(
                sys::ETrackingUniverseOrigin_TrackingUniverseStanding,
                predict,
                poses.as_mut_ptr(),
                MAX_DEVICES as u32,
            )
        };

        let mut frame = Frame {
            quit,
            ..Frame::default()
        };
        let hmd = &poses[sys::k_unTrackedDeviceIndex_Hmd as usize];
        if hmd.bPoseIsValid {
            frame.hmd = pose_from(&hmd.mDeviceToAbsoluteTracking.m);
        }
        for (device, pose) in poses.iter().enumerate().skip(1) {
            let device = device as u32;
            if !pose.bPoseIsValid {
                continue;
            }
            if unsafe { f(s.GetTrackedDeviceClass)(device) }
                == sys::ETrackedDeviceClass_TrackedDeviceClass_GenericTracker
            {
                frame.trackers.push((
                    self.string_property(
                        device,
                        sys::ETrackedDeviceProperty_Prop_SerialNumber_String,
                    ),
                    pose_from(&pose.mDeviceToAbsoluteTracking.m),
                ))
            }
        }
        if let Some(input) = &self.input {
            if let Err(e) = input.update() {
                logger::warn!("SteamVR Input の状態を取り込めない", error = e);
            }
            for (hand, device, aim, actions) in input.hands(predict) {
                let profile = profile(&self.string_property(
                    device,
                    sys::ETrackedDeviceProperty_Prop_ControllerType_String,
                ));
                frame.controllers.push(RawController {
                    hand,
                    profile: profile.into(),
                    // バインディングの `/pose/tip`（SteamVR が機種ごとに定義するレーザーの出口）。
                    aim,
                    inputs: inputs(profile, actions),
                });
            }
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

    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        let e = unsafe { f(self.overlay().SetOverlayWidthInMeters)(id.0, width_m) };
        log(check("SetOverlayWidthInMeters", e));
    }

    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        let o = self.overlay();
        let e = unsafe {
            if visible {
                f(o.ShowOverlay)(id.0)
            } else {
                f(o.HideOverlay)(id.0)
            }
        };
        log(check("ShowOverlay / HideOverlay", e));
    }

    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        let e = unsafe { f(self.overlay().SetOverlaySortOrder)(id.0, order) };
        log(check("SetOverlaySortOrder", e));
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
        logger::warn!("OpenVR の呼び出しに失敗", error = e);
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

/// SteamVR Input のアクションを、入力層が機種ごとに引く OpenXR の入力名で表す。
fn inputs(profile: &str, a: HandActions) -> Vec<(String, f32)> {
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    // Index のメニューは B ボタン。他の機種の表では menu/click で引く。
    let menu = if profile == "valve/index_controller" {
        "b/click"
    } else {
        "menu/click"
    };
    [
        ("select/click", on(a.select)),
        ("squeeze/value", on(a.grab)),
        ("squeeze/click", on(a.grab)),
        (menu, on(a.menu)),
        ("thumbstick/x", a.scroll.x),
        ("thumbstick/y", a.scroll.y),
        ("trackpad/x", a.scroll.x),
        ("trackpad/y", a.scroll.y),
    ]
    .into_iter()
    .map(|(n, v)| (n.to_owned(), v))
    .collect()
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

    /// どの機種でも、アクションが入力層の表で引ける名前に出る。
    #[test]
    fn actions_map_to_each_profile() {
        let a = HandActions {
            select: true,
            grab: true,
            menu: true,
            scroll: crate::Vec2::new(0.5, -1.0),
        };
        let v = inputs("valve/index_controller", a);
        assert_eq!(value(&v, "select/click"), Some(1.0));
        assert_eq!(value(&v, "squeeze/value"), Some(1.0));
        assert_eq!(value(&v, "b/click"), Some(1.0));
        assert_eq!(value(&v, "thumbstick/y"), Some(-1.0));
        let v = inputs("htc/vive_controller", a);
        assert_eq!(value(&v, "menu/click"), Some(1.0));
        assert_eq!(value(&v, "squeeze/click"), Some(1.0));
        assert_eq!(value(&v, "trackpad/x"), Some(0.5));
    }
}
