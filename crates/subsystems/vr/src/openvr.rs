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
    Device, Frame, Hand, OverlayId, Pose, Quat, RawController, Runtime, Vec3, VrBackend, VrError,
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
                "SteamVR Input unavailable, running without controllers",
                error = e
            )
        })
        .ok();
    Ok(Box::new(OpenVrBackend {
        system,
        input,
        overlay: client.interface(sys::IVROverlay_Version)?,
        textures: HashMap::new(),
        devices: HashMap::new(),
        relative: HashMap::new(),
        last_seen: HashMap::new(),
        fixed: HashMap::new(),
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
            .ok_or_else(|| VrError::Runtime("Vulkan: no GPU".into()))?,
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
    func.expect("OpenVR function table is missing a function")
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
    /// 最後の poll で見えていたデバイスの番号と、その生の姿勢から [`Frame::pose`] の姿勢への相対姿勢
    /// （コントローラは `/pose/tip`）。相対配置をランタイムに任せるため。
    devices: HashMap<Device, (u32, Pose)>,
    /// ランタイムに相対配置を任せたオーバーレイ → （取り付け先, 相対姿勢）。同じ取り付け先（左手のコントローラなど）の
    /// 裏のデバイスは入れ替わる（ハンドトラッキングとコントローラの持ち替え）ので、そのとき付け直す。
    relative: HashMap<u64, (Device, Pose)>,
    /// 取り付け先ごとの、最後に見えていたときの [`Frame::pose`] の姿勢。デバイスが消えたら、そこに空間固定する。
    last_seen: HashMap<Device, Pose>,
    /// 高さを固定したオーバーレイの（幅, 高さ）（m）。OpenVR は高さを縦横比で決めるので、テクセルの縦横比で合わせる。
    fixed: HashMap<u64, (f32, f32)>,
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

    /// 高さを固定したオーバーレイの見た目の高さを、テクセルの縦横比（横 / 縦）で合わせる。
    /// 表示の高さ = 幅 × テクスチャの高さ / テクスチャの幅 / テクセルの縦横比。
    fn apply_height(&self, id: u64) {
        let (Some(&(w, h)), Some(t)) = (self.fixed.get(&id), self.textures.get(&id)) else {
            return;
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let aspect = t.height as f32 * w / (t.width as f32 * h);
        let e = unsafe { f(self.overlay().SetOverlayTexelAspect)(id, aspect) };
        log(check("SetOverlayTexelAspect", e));
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

    /// 相対配置を任せたオーバーレイのうち、取り付け先の裏のデバイスが入れ替わった・消えたものを付け直す
    /// （そのままだと、もう追われていない前のデバイスに付いたまま見えなくなる）。消えたら最後の姿勢に空間固定する。
    fn reattach(&mut self, old: &HashMap<Device, (u32, Pose)>) {
        let index = |m: &HashMap<Device, (u32, Pose)>, d: &Device| m.get(d).map(|&(i, _)| i);
        let moved: Vec<(u64, Device, Pose)> = self
            .relative
            .iter()
            .filter(|(_, (d, _))| index(old, d) != index(&self.devices, d))
            .map(|(&id, (d, offset))| (id, d.clone(), *offset))
            .collect();
        for (id, device, offset) in moved {
            match self.devices.get(&device) {
                Some(&(index, to)) => {
                    log(self.attach(id, index, to * offset));
                }
                None => {
                    if let Some(&at) = self.last_seen.get(&device) {
                        self.place_absolute(id, at * offset);
                    }
                }
            }
        }
    }

    fn attach(&self, id: u64, index: u32, offset: Pose) -> Result<(), VrError> {
        let mut m = sys::HmdMatrix34_t {
            m: matrix_from(offset),
        };
        let e = unsafe {
            f(self.overlay().SetOverlayTransformTrackedDeviceRelative)(id, index, &mut m)
        };
        check("SetOverlayTransformTrackedDeviceRelative", e)
    }

    fn place_absolute(&self, id: u64, pose: Pose) {
        let mut m = sys::HmdMatrix34_t {
            m: matrix_from(pose),
        };
        let e = unsafe {
            f(self.overlay().SetOverlayTransformAbsolute)(
                id,
                sys::ETrackingUniverseOrigin_TrackingUniverseStanding,
                &mut m,
            )
        };
        log(check("SetOverlayTransformAbsolute", e));
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
        let mut replacement = if self
            .textures
            .get(&id)
            .is_none_or(|t| (t.width, t.height) != (width, height))
        {
            let usage = vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::SAMPLED;
            let images = create_ring(
                || self.gpu.create_image(width, height, usage),
                |image| self.gpu.destroy_image(image),
            )?;
            Some(Textures {
                width,
                height,
                images,
                next: 0,
            })
        } else {
            None
        };
        let t = replacement
            .as_mut()
            .unwrap_or_else(|| self.textures.get_mut(&id).unwrap());
        let image = t.images[t.next].image;
        t.next = (t.next + 1) % RING;
        // SteamVR は Vulkan のテクスチャを TRANSFER_SRC_OPTIMAL で受け取る。
        let result = (|| {
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
        })();
        if let Some(new) = replacement {
            if result.is_ok() {
                if let Some(old) = self.textures.insert(id, new) {
                    old.images
                        .into_iter()
                        .for_each(|image| self.gpu.retire(image));
                }
            } else {
                self.gpu.wait_idle();
                for image in new.images {
                    self.gpu.destroy_image(image);
                }
            }
        }
        result
    }
}

fn create_ring<T, E>(
    mut create: impl FnMut() -> Result<T, E>,
    mut destroy: impl FnMut(T),
) -> Result<Vec<T>, E> {
    let mut images = Vec::with_capacity(RING);
    for _ in 0..RING {
        match create() {
            Ok(image) => images.push(image),
            Err(e) => {
                for image in images {
                    destroy(image);
                }
                return Err(e);
            }
        }
    }
    Ok(images)
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
        let mut devices = HashMap::new();
        let hmd = &poses[sys::k_unTrackedDeviceIndex_Hmd as usize];
        if hmd.bPoseIsValid {
            frame.hmd = pose_from(&hmd.mDeviceToAbsoluteTracking.m);
            devices.insert(
                Device::Hmd,
                (sys::k_unTrackedDeviceIndex_Hmd as _, Pose::IDENTITY),
            );
        }
        for (device, pose) in poses.iter().enumerate().skip(1) {
            let device = device as u32;
            // 切断されていても「姿勢は有効」と申告するドライバがある（standable のボディトラッキング）。
            if !pose.bPoseIsValid || !pose.bDeviceIsConnected {
                continue;
            }
            if unsafe { f(s.GetTrackedDeviceClass)(device) }
                == sys::ETrackedDeviceClass_TrackedDeviceClass_GenericTracker
            {
                let serial = self
                    .string_property(device, sys::ETrackedDeviceProperty_Prop_SerialNumber_String);
                devices.insert(Device::Tracker(serial.clone()), (device, Pose::IDENTITY));
                frame
                    .trackers
                    .push((serial, pose_from(&pose.mDeviceToAbsoluteTracking.m)))
            }
        }
        if let Some(input) = &self.input {
            if let Err(e) = input.update() {
                logger::warn!("failed to sync SteamVR Input state", error = e);
            }
            for (hand, device, aim, actions) in input.hands(predict) {
                let profile = profile(&self.string_property(
                    device,
                    sys::ETrackedDeviceProperty_Prop_ControllerType_String,
                ));
                // 生の姿勢と同じ時刻の予測なので、本体から見た tip は一定。
                let raw = poses.get(device as usize).filter(|p| p.bPoseIsValid);
                if let Some(raw) = raw {
                    let tip = pose_from(&raw.mDeviceToAbsoluteTracking.m).inverse() * aim;
                    devices.insert(Device::Controller(hand), (device, tip));
                }
                frame.hand_devices.push((hand, aim));
                frame.controllers.push(RawController {
                    hand,
                    profile: profile.into(),
                    // バインディングの `/pose/tip`（SteamVR が機種ごとに定義するレーザーの出口）。
                    aim,
                    inputs: inputs(profile, hand, actions),
                });
            }
            frame.hands = input.skeletons(predict);
        }
        // 入力のアクションで取れなかった手も、手の役割に割り当たっているデバイスがあれば取り付け先にする
        // （ハンドトラッキングの手のデバイスなど。ほかのオーバーレイも役割のデバイスに付けて追従している）。
        for (hand, role) in [
            (Hand::Left, sys::ETrackedControllerRole_TrackedControllerRole_LeftHand),
            (Hand::Right, sys::ETrackedControllerRole_TrackedControllerRole_RightHand),
        ] {
            if devices.contains_key(&Device::Controller(hand)) {
                continue;
            }
            let device = unsafe { f(s.GetTrackedDeviceIndexForControllerRole)(role) };
            let Some(pose) = poses
                .get(device as usize)
                .filter(|p| p.bPoseIsValid && p.bDeviceIsConnected)
            else {
                continue;
            };
            devices.insert(Device::Controller(hand), (device, Pose::IDENTITY));
            frame
                .hand_devices
                .push((hand, pose_from(&pose.mDeviceToAbsoluteTracking.m)));
        }
        let old = std::mem::replace(&mut self.devices, devices);
        for (device, &(index, to)) in &self.devices {
            let raw = pose_from(&poses[index as usize].mDeviceToAbsoluteTracking.m);
            self.last_seen.insert(device.clone(), raw * to);
        }
        self.reattach(&old);
        frame
    }

    fn max_overlays(&self) -> Option<usize> {
        // ランタイム全体（ほかのアプリの分も含む）の数。実際に作れる数はこれより少ないことがある。
        Some(sys::k_unMaxOverlayCount as usize)
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
            // GPUI の描画は透明な背景に合成した事前乗算アルファ。
            check(
                "SetOverlayFlag",
                f(o.SetOverlayFlag)(handle, sys::VROverlayFlags_IsPremultiplied, true),
            )?;
            check("ShowOverlay", f(o.ShowOverlay)(handle))?;
        }
        // OpenVR のハンドルをそのまま ID にする。
        Ok(OverlayId(handle))
    }

    fn destroy_overlay(&mut self, id: OverlayId) {
        self.relative.remove(&id.0);
        unsafe {
            let _ = f(self.overlay().DestroyOverlay)(id.0);
        }
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.relative.remove(&id.0);
        self.place_absolute(id.0, pose);
    }

    fn set_overlay_pose_relative(&mut self, id: OverlayId, device: &Device, offset: Pose) -> bool {
        let Some(&(index, to_pose)) = self.devices.get(device) else {
            return false;
        };
        let ok = self.attach(id.0, index, to_pose * offset).is_ok();
        if ok {
            self.relative.insert(id.0, (device.clone(), offset));
        }
        ok
    }

    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        let e = unsafe { f(self.overlay().SetOverlayWidthInMeters)(id.0, width_m) };
        log(check("SetOverlayWidthInMeters", e));
        if let Some(size) = self.fixed.get_mut(&id.0) {
            size.0 = width_m;
            self.apply_height(id.0);
        }
    }

    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        let width = self.fixed.get(&id.0).map_or(0.0, |s| s.0);
        let width = if width > 0.0 {
            width
        } else {
            let mut w = 0.0;
            unsafe { f(self.overlay().GetOverlayWidthInMeters)(id.0, &mut w) };
            w
        };
        self.fixed.insert(id.0, (width, height_m));
        self.apply_height(id.0);
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

    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        let e = unsafe { f(self.overlay().SetOverlayAlpha)(id.0, alpha.clamp(0.0, 1.0)) };
        log(check("SetOverlayAlpha", e));
        true
    }

    fn set_overlay_tint(&mut self, id: OverlayId, brightness: f32) -> bool {
        let b = brightness.clamp(0.0, 1.0);
        let e = unsafe { f(self.overlay().SetOverlayColor)(id.0, b, b, b) };
        log(check("SetOverlayColor", e));
        true
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "invalid RGBA buffer size"
        );
        log(self.submit(id.0, width, height, rgba));
        self.apply_height(id.0);
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

/// 実ランタイムテスト用: 最後に設定された（幅, 不透明度, 表示）を読み戻す。
pub fn overlay_state(id: OverlayId) -> Result<(f32, f32, bool), VrError> {
    let client = Client::load()?;
    let overlay =
        unsafe { &*client.interface::<sys::VR_IVROverlay_FnTable>(sys::IVROverlay_Version)? };
    let (mut width, mut alpha) = (0.0, 0.0);
    check("GetOverlayWidthInMeters", unsafe {
        f(overlay.GetOverlayWidthInMeters)(id.0, &mut width)
    })?;
    check("GetOverlayAlpha", unsafe {
        f(overlay.GetOverlayAlpha)(id.0, &mut alpha)
    })?;
    Ok((width, alpha, unsafe { f(overlay.IsOverlayVisible)(id.0) }))
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
        logger::warn!("OpenVR call failed", error = e);
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
fn inputs(profile: &str, hand: Hand, a: HandActions) -> Vec<(String, f32)> {
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    // Index のメニューは B ボタン。他の機種の表では menu/click で引く。
    let menu = if profile == "valve/index_controller" {
        "b/click"
    } else {
        "menu/click"
    };
    let (middle, right) = match hand {
        Hand::Left => ("y/touch", "x/touch"),
        Hand::Right => ("b/touch", "a/touch"),
    };
    [
        ("select/click", on(a.select)),
        ("squeeze/value", on(a.grab)),
        ("squeeze/click", on(a.grab)),
        (menu, on(a.menu)),
        (middle, on(a.middle_touch)),
        (right, on(a.right_touch)),
        // X ボタン: Touch は x/click、Index は左手の a/click。
        ("x/click", on(a.shortcut)),
        ("a/click", on(a.shortcut)),
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

    #[test]
    fn failed_ring_creation_releases_earlier_images() {
        let mut next = 0;
        let mut destroyed = Vec::new();
        let result = create_ring(
            || {
                next += 1;
                (next < RING).then_some(next).ok_or("out of memory")
            },
            |image| destroyed.push(image),
        );
        assert_eq!(result, Err("out of memory"));
        assert_eq!(destroyed, (1..RING).collect::<Vec<_>>());
    }

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
        // 90° about the Y axis: local -Z (forward) points to world -X.
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
            middle_touch: true,
            right_touch: true,
            shortcut: true,
            scroll: crate::Vec2::new(0.5, -1.0),
        };
        let v = inputs("valve/index_controller", Hand::Right, a);
        assert_eq!(value(&v, "select/click"), Some(1.0));
        assert_eq!(value(&v, "squeeze/value"), Some(1.0));
        assert_eq!(value(&v, "b/click"), Some(1.0));
        assert_eq!(value(&v, "a/click"), Some(1.0));
        assert_eq!(value(&v, "thumbstick/y"), Some(-1.0));
        let v = inputs("htc/vive_controller", Hand::Left, a);
        assert_eq!(value(&v, "menu/click"), Some(1.0));
        assert_eq!(value(&v, "squeeze/click"), Some(1.0));
        assert_eq!(value(&v, "trackpad/x"), Some(0.5));
        let left = inputs("oculus/touch_controller", Hand::Left, a);
        let right = inputs("oculus/touch_controller", Hand::Right, a);
        assert_eq!(value(&left, "x/touch"), Some(1.0));
        assert_eq!(value(&left, "a/touch"), None);
        assert_eq!(value(&right, "b/touch"), Some(1.0));
        assert_eq!(value(&right, "y/touch"), None);
    }
}
