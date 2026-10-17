//! OpenXR バックエンド: オーバーレイは XR_EXTX_overlay セッション + クアッドレイヤー、
//! 入力はインタラクションプロファイルごとに推奨バインドしたアクションセット。
//! 描画は Vulkan（XR_KHR_vulkan_enable2）のスワップチェーンへ書き込む。

use std::collections::HashMap;

use ash::vk::{self, Handle};
use openxr as xr;

use crate::gpu::{self, Gpu};
use crate::{Frame, Hand, OverlayId, Pose, Quat, RawController, Runtime, Vec3, VrBackend, VrError};

pub(crate) fn available() -> bool {
    os::openxr_runtime_manifest().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    Ok(Box::new(OpenXrBackend::new()?))
}

fn xr_err(what: &str) -> impl FnOnce(xr::sys::Result) -> VrError + '_ {
    move |e| VrError::Runtime(format!("{what}: {e}"))
}

/// プロファイルごとに読む入力（OpenXR の語彙、`/user/hand/*/input/` 以下）。
/// 入力層の `PROFILES` が使うものを、ランタイムにバインドを提案するために列挙する。
const BINDINGS: &[(&str, &[&str])] = &[
    ("khr/simple_controller", &["select/click", "menu/click"]),
    (
        "valve/index_controller",
        &[
            "trigger/value",
            "squeeze/value",
            "b/click",
            "thumbstick/x",
            "thumbstick/y",
        ],
    ),
    (
        "oculus/touch_controller",
        &[
            "trigger/value",
            "squeeze/value",
            "menu/click",
            "y/click",
            "b/click",
            "x/touch",
            "y/touch",
            "a/touch",
            "b/touch",
            "thumbstick/x",
            "thumbstick/y",
        ],
    ),
    (
        "htc/vive_controller",
        &[
            "trigger/value",
            "squeeze/click",
            "menu/click",
            "trackpad/x",
            "trackpad/y",
        ],
    ),
];

/// その入力がその手のコントローラにあるか。左右で違うのは Touch のボタンだけ。
fn on_hand(profile: &str, input: &str, hand: Hand) -> bool {
    match (profile, input) {
        (
            "oculus/touch_controller",
            "menu/click" | "x/click" | "y/click" | "x/touch" | "y/touch",
        ) => hand == Hand::Left,
        ("oculus/touch_controller", "a/click" | "b/click" | "a/touch" | "b/touch") => {
            hand == Hand::Right
        }
        _ => true,
    }
}

/// `"trigger/value"` → `"trigger_value"`（アクション名に `/` は使えない）。
fn action_name(input: &str) -> String {
    input.replace('/', "_")
}

/// XR_HTCX_vive_tracker_interaction のロール（`/user/vive_tracker_htcx/role/<名前>`）。
/// トラッカーの ID にはこの名前をそのまま使う。
const TRACKER_ROLES: &[&str] = &[
    "waist",
    "chest",
    "left_foot",
    "right_foot",
    "left_knee",
    "right_knee",
    "left_elbow",
    "right_elbow",
    "left_shoulder",
    "right_shoulder",
];

const HANDS: [(Hand, &str); 2] = [(Hand::Left, "left"), (Hand::Right, "right")];

struct Overlay {
    width_m: f32,
    /// 固定された高さ。無ければテクスチャの縦横比で決まる。
    height_m: Option<f32>,
    pose: Pose,
    visible: bool,
    color_scale: xr::sys::CompositionLayerColorScaleBiasKHR,
    sort: u32,
    swapchain: Option<Swapchain>,
}

struct Swapchain {
    handle: xr::Swapchain<xr::Vulkan>,
    width: u32,
    height: u32,
    images: Vec<vk::Image>,
    /// 最後に書き込んだ画像の index。
    last: Option<usize>,
}

pub(crate) struct OpenXrBackend {
    // フィールドは宣言順に破棄される。セッション由来のものを先に、Vulkan、インスタンスの順に。
    overlays: HashMap<u64, Overlay>,
    next_id: u64,
    /// 1 フレームに出せるレイヤーの数（`maxLayerCount`）。超えると xrEndFrame が失敗して何も出なくなる。
    max_layers: usize,
    stage: xr::Space,
    view: xr::Space,
    aim_spaces: Vec<xr::Space>,
    /// ロールごとのトラッカーの空間。拡張が無いランタイムでは空。
    tracker_spaces: Vec<(&'static str, xr::Space)>,
    actions: Vec<(String, xr::Action<f32>)>,
    button_actions: Vec<(String, xr::Action<bool>)>,
    /// レーザーの姿勢。`aim_spaces` の元なので持っておく。
    _aim: xr::Action<xr::Posef>,
    _tracker: Option<xr::Action<xr::Posef>>,
    action_set: xr::ActionSet,
    hand_paths: [xr::Path; 2],
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
    /// セッションが動いていて、フレームを回してよいか。
    running: bool,
    /// `begin` 済みで `end` していないフレームの表示時刻。
    in_frame: Option<xr::Time>,
    /// ランタイムとの接続が失われた（落ちた等）。以後は何もしない。
    lost: bool,
    /// セッションが EXITING / LOSS_PENDING になった。`Frame::quit` で上位へ伝える。
    quit: bool,
    gpu: Gpu,
    instance: xr::Instance,
}

impl OpenXrBackend {
    fn new() -> Result<Self, VrError> {
        let entry = xr::Entry::linked(&()).map_err(xr_err("OpenXR loader"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(xr_err("xrEnumerateInstanceExtensionProperties"))?;
        for (ok, name) in [
            (available.extx_overlay, "XR_EXTX_overlay"),
            (available.khr_vulkan_enable2, "XR_KHR_vulkan_enable2"),
        ] {
            if !ok {
                return Err(VrError::Runtime(format!(
                    "OpenXR runtime does not support {name}"
                )));
            }
        }
        let mut extensions = xr::ExtensionSet::default();
        extensions.extx_overlay = true;
        extensions.khr_vulkan_enable2 = true;
        extensions.khr_composition_layer_color_scale_bias =
            available.khr_composition_layer_color_scale_bias;
        // トラッカーは任意。無いランタイムでもコントローラだけで動く。
        let has_trackers = available.htcx_vive_tracker_interaction;
        extensions.htcx_vive_tracker_interaction = has_trackers;
        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: "nyaw",
                    engine_name: "nyaw",
                    api_version: xr::Version::new(1, 0, 0),
                    ..Default::default()
                },
                &extensions,
                &[],
                &(),
            )
            .map_err(xr_err("xrCreateInstance"))?;
        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(xr_err("xrGetSystem"))?;

        let max_layers = instance
            .system_properties(system)
            .map_err(xr_err("xrGetSystemProperties"))?
            .graphics_properties
            .max_layer_count as usize;
        let gpu = create_gpu(&instance, system)?;
        let session = create_overlay_session(&instance, system, &gpu)?;
        let (session, waiter, stream) = session;

        let stage_type = if session
            .enumerate_reference_spaces()
            .map_err(xr_err("xrEnumerateReferenceSpaces"))?
            .contains(&xr::ReferenceSpaceType::STAGE)
        {
            xr::ReferenceSpaceType::STAGE
        } else {
            xr::ReferenceSpaceType::LOCAL
        };
        let stage = session
            .create_reference_space(stage_type, xr::Posef::IDENTITY)
            .map_err(xr_err("xrCreateReferenceSpace"))?;
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(xr_err("xrCreateReferenceSpace"))?;

        let path = |s: &str| instance.string_to_path(s).map_err(xr_err("xrStringToPath"));
        let hand_paths = [path("/user/hand/left")?, path("/user/hand/right")?];
        let action_set = instance
            .create_action_set("nyaw", "nyaw", 0)
            .map_err(xr_err("xrCreateActionSet"))?;
        let aim = action_set
            .create_action::<xr::Posef>("aim", "aim", &hand_paths)
            .map_err(xr_err("xrCreateAction"))?;
        let mut actions: Vec<(String, xr::Action<f32>)> = Vec::new();
        let mut button_actions: Vec<(String, xr::Action<bool>)> = Vec::new();
        for input in BINDINGS.iter().flat_map(|(_, inputs)| inputs.iter()) {
            if input.ends_with("/touch") || input.ends_with("/click") {
                if button_actions.iter().any(|(n, _)| n == input) {
                    continue;
                }
                let name = action_name(input);
                let action = action_set
                    .create_action::<bool>(&name, &name, &hand_paths)
                    .map_err(xr_err("xrCreateAction"))?;
                button_actions.push((input.to_string(), action));
                continue;
            }
            if actions.iter().any(|(n, _)| n == input) {
                continue;
            }
            let name = action_name(input);
            let action = action_set
                .create_action::<f32>(&name, &name, &hand_paths)
                .map_err(xr_err("xrCreateAction"))?;
            actions.push((input.to_string(), action));
        }
        for (profile, inputs) in BINDINGS {
            let mut bindings = Vec::new();
            for (hand, side) in HANDS {
                bindings.push(xr::Binding::new(
                    &aim,
                    path(&format!("/user/hand/{side}/input/aim/pose"))?,
                ));
                for input in inputs.iter().filter(|i| on_hand(profile, i, hand)) {
                    if let Some((_, action)) = button_actions.iter().find(|(n, _)| n == input) {
                        bindings.push(xr::Binding::new(
                            action,
                            path(&format!("/user/hand/{side}/input/{input}"))?,
                        ));
                        continue;
                    }
                    let (_, action) = actions.iter().find(|(n, _)| n == input).unwrap();
                    bindings.push(xr::Binding::new(
                        action,
                        path(&format!("/user/hand/{side}/input/{input}"))?,
                    ));
                }
            }
            // 対応していないプロファイルはランタイムが拒否するが、他のプロファイルは使えるので続ける。
            if let Err(e) = instance.suggest_interaction_profile_bindings(
                path(&format!("/interaction_profiles/{profile}"))?,
                &bindings,
            ) {
                eprintln!("OpenXR: {profile} bindings rejected: {e}");
            }
        }
        let tracker = if has_trackers {
            let role_paths = TRACKER_ROLES
                .iter()
                .map(|r| path(&format!("/user/vive_tracker_htcx/role/{r}")))
                .collect::<Result<Vec<_>, _>>()?;
            let action = action_set
                .create_action::<xr::Posef>("tracker", "tracker", &role_paths)
                .map_err(xr_err("xrCreateAction"))?;
            let bindings = TRACKER_ROLES
                .iter()
                .map(|r| {
                    Ok(xr::Binding::new(
                        &action,
                        path(&format!("/user/vive_tracker_htcx/role/{r}/input/grip/pose"))?,
                    ))
                })
                .collect::<Result<Vec<_>, VrError>>()?;
            if let Err(e) = instance.suggest_interaction_profile_bindings(
                path("/interaction_profiles/htc/vive_tracker_htcx")?,
                &bindings,
            ) {
                eprintln!("OpenXR: tracker bindings rejected: {e}");
            }
            Some((action, role_paths))
        } else {
            None
        };
        session
            .attach_action_sets(&[&action_set])
            .map_err(xr_err("xrAttachSessionActionSets"))?;
        let aim_spaces = hand_paths
            .iter()
            .map(|&p| {
                aim.create_space(&session, p, xr::Posef::IDENTITY)
                    .map_err(xr_err("xrCreateActionSpace"))
            })
            .collect::<Result<_, _>>()?;
        let mut tracker_spaces = Vec::new();
        if let Some((action, role_paths)) = &tracker {
            for (&role, &p) in TRACKER_ROLES.iter().zip(role_paths) {
                let space = action
                    .create_space(&session, p, xr::Posef::IDENTITY)
                    .map_err(xr_err("xrCreateActionSpace"))?;
                tracker_spaces.push((role, space));
            }
        }

        Ok(Self {
            overlays: HashMap::new(),
            next_id: 0,
            max_layers,
            stage,
            view,
            aim_spaces,
            tracker_spaces,
            actions,
            button_actions,
            _aim: aim,
            _tracker: tracker.map(|(a, _)| a),
            action_set,
            hand_paths,
            stream,
            waiter,
            session,
            running: false,
            in_frame: None,
            lost: false,
            quit: false,
            gpu,
            instance,
        })
    }

    /// セッションやインスタンスが失われたら、以後のフレームと送信を止める（エラーを出し続けないため）。
    fn check_lost(&mut self, e: xr::sys::Result) {
        if matches!(
            e,
            xr::sys::Result::ERROR_SESSION_LOST | xr::sys::Result::ERROR_INSTANCE_LOST
        ) {
            eprintln!("OpenXR: lost connection to runtime");
            self.lost = true;
            self.quit = true;
            self.running = false;
            self.in_frame = None;
        }
    }

    /// 姿勢を取れないフレーム。終了要求だけは伝える。
    fn idle(&self) -> Frame {
        Frame {
            quit: self.quit,
            ..Frame::default()
        }
    }

    fn handle_events(&mut self) {
        let mut buf = xr::EventDataBuffer::new();
        while let Ok(Some(event)) = self.instance.poll_event(&mut buf) {
            let xr::Event::SessionStateChanged(e) = event else {
                continue;
            };
            match e.state() {
                xr::SessionState::READY => {
                    match self
                        .session
                        .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                    {
                        Ok(_) => self.running = true,
                        Err(e) => eprintln!("OpenXR: xrBeginSession: {e}"),
                    }
                }
                xr::SessionState::STOPPING => {
                    self.end_frame();
                    self.running = false;
                    if let Err(e) = self.session.end() {
                        eprintln!("OpenXR: xrEndSession: {e}");
                    }
                }
                xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                    self.running = false;
                    self.in_frame = None;
                    self.quit = true;
                }
                _ => {}
            }
        }
    }

    /// 開いているフレームを、画像が届いているオーバーレイのクアッドを載せて閉じる。
    fn end_frame(&mut self) {
        let Some(time) = self.in_frame.take() else {
            return;
        };
        // レイヤーは後ろのものほど手前に合成される。描画順、同じなら作った順に並べる。
        let mut overlays: Vec<_> = self.overlays.iter().filter(|(_, o)| o.visible).collect();
        overlays.sort_by_key(|(id, o)| (o.sort, **id));
        let quads: Vec<_> = overlays
            .into_iter()
            .filter_map(|(_, o)| {
                let sc = o.swapchain.as_ref()?;
                let height_m = o
                    .height_m
                    .unwrap_or(o.width_m * sc.height as f32 / sc.width as f32);
                let mut quad = xr::CompositionLayerQuad::new()
                    .layer_flags(
                        // GPUI の描画は透明な背景に合成した事前乗算アルファ。
                        xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                    )
                    .space(&self.stage)
                    .eye_visibility(xr::EyeVisibility::BOTH)
                    .sub_image(
                        xr::SwapchainSubImage::new()
                            .swapchain(&sc.handle)
                            .image_rect(xr::Rect2Di {
                                offset: xr::Offset2Di { x: 0, y: 0 },
                                extent: xr::Extent2Di {
                                    width: sc.width as i32,
                                    height: sc.height as i32,
                                },
                            }),
                    )
                    .pose(to_posef(o.pose))
                    .size(xr::Extent2Df {
                        width: o.width_m,
                        height: height_m,
                    })
                    .into_raw();
                if self
                    .instance
                    .exts()
                    .khr_composition_layer_color_scale_bias
                    .is_some()
                {
                    quad.next = (&o.color_scale
                        as *const xr::sys::CompositionLayerColorScaleBiasKHR)
                        .cast();
                }
                // SAFETY: swapchain・stage・拡張構造体は、下の xrEndFrame が終わるまで保持される。
                Some(unsafe { xr::CompositionLayerQuad::from_raw(quad) })
            })
            .collect();
        let layers: Vec<&xr::CompositionLayerBase<xr::Vulkan>> =
            quads.iter().map(|q| &**q).collect();
        if let Err(e) = self
            .stream
            .end(time, xr::EnvironmentBlendMode::OPAQUE, &layers)
        {
            eprintln!("OpenXR: xrEndFrame: {e}");
        }
    }

    /// いま追跡できているトラッカー（ロール名, 姿勢）。つながっていないロールは位置が無効で落ちる。
    fn trackers(&self, time: xr::Time) -> Vec<(String, Pose)> {
        self.tracker_spaces
            .iter()
            .filter_map(|(role, space)| {
                let l = space.locate(&self.stage, time).ok()?;
                l.location_flags
                    .contains(
                        xr::SpaceLocationFlags::POSITION_VALID
                            | xr::SpaceLocationFlags::ORIENTATION_VALID,
                    )
                    .then(|| (role.to_string(), from_posef(l.pose)))
            })
            .collect()
    }

    fn controllers(&self, time: xr::Time) -> Vec<RawController> {
        let mut out = Vec::new();
        for (i, (hand, _)) in HANDS.into_iter().enumerate() {
            let hand_path = self.hand_paths[i];
            let Ok(profile_path) = self.session.current_interaction_profile(hand_path) else {
                continue;
            };
            if profile_path == xr::Path::NULL {
                continue;
            }
            let Ok(profile) = self.instance.path_to_string(profile_path) else {
                continue;
            };
            let profile = profile
                .trim_start_matches("/interaction_profiles/")
                .to_owned();
            let Ok(location) = self.aim_spaces[i].locate(&self.stage, time) else {
                continue;
            };
            if !location.location_flags.contains(
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
            ) {
                continue;
            }
            let mut inputs: Vec<(String, f32)> = self
                .actions
                .iter()
                .filter_map(|(name, action)| {
                    let state = action.state(&self.session, hand_path).ok()?;
                    state.is_active.then(|| (name.clone(), state.current_state))
                })
                .collect();
            inputs.extend(self.button_actions.iter().filter_map(|(name, action)| {
                let state = action.state(&self.session, hand_path).ok()?;
                state
                    .is_active
                    .then(|| (name.clone(), f32::from(state.current_state as u8)))
            }));
            out.push(RawController {
                hand,
                profile,
                aim: from_posef(location.pose),
                inputs,
            });
        }
        out
    }

    fn submit(&mut self, id: u64, width: u32, height: u32, rgba: &[u8]) -> Result<(), VrError> {
        let overlay = self
            .overlays
            .get_mut(&id)
            .ok_or_else(|| VrError::Runtime(format!("unknown overlay {id}")))?;
        if overlay
            .swapchain
            .as_ref()
            .is_none_or(|s| (s.width, s.height) != (width, height))
        {
            if let Some(old) = overlay.swapchain.take() {
                // 古い画像を GPU がまだ使っているかもしれない。LOD の切り替えなどで作り直すたびに待つ。
                self.gpu.wait_idle();
                drop(old);
            }
            let handle = self
                .session
                .create_swapchain(&xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                        | xr::SwapchainUsageFlags::TRANSFER_DST
                        | xr::SwapchainUsageFlags::TRANSFER_SRC,
                    format: gpu::FORMAT.as_raw() as u32,
                    sample_count: 1,
                    width,
                    height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(xr_err("xrCreateSwapchain"))?;
            let images = handle
                .enumerate_images()
                .map_err(xr_err("xrEnumerateSwapchainImages"))?
                .into_iter()
                .map(vk::Image::from_raw)
                .collect();
            overlay.swapchain = Some(Swapchain {
                handle,
                width,
                height,
                images,
                last: None,
            });
        }
        let sc = overlay.swapchain.as_mut().unwrap();
        let index = sc
            .handle
            .acquire_image()
            .map_err(xr_err("xrAcquireSwapchainImage"))?;
        sc.handle
            .wait_image(xr::Duration::from_nanos(100_000_000))
            .map_err(xr_err("xrWaitSwapchainImage"))?;
        // OpenXR はカラー用のスワップチェーンを COLOR_ATTACHMENT_OPTIMAL で返してもらう決まり。
        let uploaded = self.gpu.upload(
            sc.images[index as usize],
            width,
            height,
            rgba,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        );
        sc.last = Some(index as usize);
        sc.handle
            .release_image()
            .map_err(xr_err("xrReleaseSwapchainImage"))?;
        uploaded
    }
}

/// ランタイムに Vulkan のインスタンスとデバイスを作らせる（XR_KHR_vulkan_enable2 の決まり）。
fn create_gpu(instance: &xr::Instance, system: xr::SystemId) -> Result<Gpu, VrError> {
    // 作成前に要件を問い合わせるのが仕様上の義務。中身は Vulkan 1.1 なら満たす想定で見ない。
    instance
        .graphics_requirements::<xr::Vulkan>(system)
        .map_err(xr_err("xrGetVulkanGraphicsRequirements2KHR"))?;
    let entry = gpu::entry()?;
    let get_proc = unsafe {
        std::mem::transmute::<vk::PFN_vkGetInstanceProcAddr, xr::sys::platform::VkGetInstanceProcAddr>(
            entry.static_fn().get_instance_proc_addr,
        )
    };
    let vk_instance = gpu::create_instance(&entry, &[], |info| {
        unsafe { instance.create_vulkan_instance(system, get_proc, info as *const _ as *const _) }
            .map_err(xr_err("xrCreateVulkanInstanceKHR"))?
            .map(|raw| vk::Instance::from_raw(raw as u64))
            .map_err(|e| VrError::Runtime(format!("vkCreateInstance: {}", vk::Result::from_raw(e))))
    })?;
    let physical = vk::PhysicalDevice::from_raw(
        unsafe { instance.vulkan_graphics_device(system, vk_instance.handle().as_raw() as _) }
            .map_err(xr_err("xrGetVulkanGraphicsDevice2KHR"))? as u64,
    );
    gpu::create_device(entry, vk_instance, physical, &[], |_, info| {
        unsafe {
            instance.create_vulkan_device(
                system,
                get_proc,
                physical.as_raw() as _,
                info as *const _ as *const _,
            )
        }
        .map_err(xr_err("xrCreateVulkanDeviceKHR"))?
        .map(|raw| vk::Device::from_raw(raw as u64))
        .map_err(|e| VrError::Runtime(format!("vkCreateDevice: {}", vk::Result::from_raw(e))))
    })
}

type SessionParts = (
    xr::Session<xr::Vulkan>,
    xr::FrameWaiter,
    xr::FrameStream<xr::Vulkan>,
);

/// `SessionCreateInfoOverlayEXTX` を繋いでセッションを作る。openxr クレートの `create_session` は
/// `next` を繋げないので、生の関数を呼んでから包む。
fn create_overlay_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    gpu: &Gpu,
) -> Result<SessionParts, VrError> {
    let overlay = xr::sys::SessionCreateInfoOverlayEXTX {
        ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE,
        next: std::ptr::null(),
        create_flags: xr::OverlaySessionCreateFlagsEXTX::EMPTY,
        // 他のオーバーレイとの重なり順。大きいほど手前。
        session_layers_placement: 1,
    };
    let binding = xr::sys::GraphicsBindingVulkanKHR {
        ty: xr::sys::GraphicsBindingVulkanKHR::TYPE,
        next: &overlay as *const _ as *const _,
        instance: gpu.instance.handle().as_raw() as _,
        physical_device: gpu.physical.as_raw() as _,
        device: gpu.device.handle().as_raw() as _,
        queue_family_index: gpu.queue_family,
        queue_index: 0,
    };
    let info = xr::sys::SessionCreateInfo {
        ty: xr::sys::SessionCreateInfo::TYPE,
        next: &binding as *const _ as *const _,
        create_flags: Default::default(),
        system_id: system,
    };
    let mut raw = <xr::sys::Session as xr::sys::Handle>::NULL;
    let r = unsafe { (instance.fp().create_session)(instance.as_raw(), &info, &mut raw) };
    if r.into_raw() < 0 {
        return Err(VrError::Runtime(format!("xrCreateSession: {r}")));
    }
    Ok(unsafe { xr::Session::from_raw(instance.clone(), raw, Box::new(())) })
}

fn to_posef(p: Pose) -> xr::Posef {
    let q = p.orientation;
    xr::Posef {
        orientation: xr::Quaternionf {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
        },
        position: xr::Vector3f {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
        },
    }
}

fn from_posef(p: xr::Posef) -> Pose {
    let (q, v) = (p.orientation, p.position);
    Pose::new(
        Vec3::new(v.x, v.y, v.z),
        Quat::from_xyzw(q.x, q.y, q.z, q.w),
    )
}

impl VrBackend for OpenXrBackend {
    fn max_overlays(&self) -> Option<usize> {
        Some(self.max_layers)
    }

    fn runtime(&self) -> Runtime {
        Runtime::OpenXr
    }

    fn poll(&mut self) -> Frame {
        // 前回の poll から今までに送られた画像を載せて、前のフレームを閉じる。
        self.end_frame();
        if self.lost {
            return self.idle();
        }
        self.handle_events();
        if !self.running {
            return self.idle();
        }
        let state = match self.waiter.wait() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("OpenXR: xrWaitFrame: {e}");
                self.check_lost(e);
                return self.idle();
            }
        };
        if let Err(e) = self.stream.begin() {
            eprintln!("OpenXR: xrBeginFrame: {e}");
            return self.idle();
        }
        let time = state.predicted_display_time;
        self.in_frame = Some(time);

        if let Err(e) = self
            .session
            .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])
        {
            eprintln!("OpenXR: xrSyncActions: {e}");
        }
        let hmd = self
            .view
            .locate(&self.stage, time)
            .ok()
            .filter(|l| {
                l.location_flags
                    .contains(xr::SpaceLocationFlags::POSITION_VALID)
            })
            .map_or(Pose::IDENTITY, |l| from_posef(l.pose));
        Frame {
            hmd,
            controllers: self.controllers(time),
            trackers: self.trackers(time),
            quit: self.quit,
            emulated: false,
        }
    }

    fn create_overlay(&mut self, _key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        if self.overlays.len() >= self.max_layers {
            return Err(VrError::Runtime(format!(
                "OpenXR: no more than {} layers per frame",
                self.max_layers
            )));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.overlays.insert(
            id,
            Overlay {
                width_m,
                height_m: None,
                pose: Pose::IDENTITY,
                visible: true,
                color_scale: xr::sys::CompositionLayerColorScaleBiasKHR {
                    ty: xr::sys::CompositionLayerColorScaleBiasKHR::TYPE,
                    next: std::ptr::null(),
                    color_scale: xr::Color4f {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    color_bias: xr::Color4f {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    },
                },
                sort: 0,
                swapchain: None,
            },
        );
        Ok(OverlayId(id))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        if let Some(o) = self.overlays.get_mut(&id.0) {
            o.pose = pose;
        }
    }

    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        if let Some(o) = self.overlays.get_mut(&id.0) {
            o.width_m = width_m;
        }
    }

    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        if let Some(o) = self.overlays.get_mut(&id.0) {
            o.height_m = Some(height_m);
        }
    }

    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        if let Some(o) = self.overlays.get_mut(&id.0) {
            o.visible = visible;
        }
    }

    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        if self
            .instance
            .exts()
            .khr_composition_layer_color_scale_bias
            .is_none()
        {
            return false;
        }
        if let Some(o) = self.overlays.get_mut(&id.0) {
            // 拡張は事前乗算を外してから適用するので、RGB は保ち alpha だけを減らす。
            o.color_scale.color_scale.a = alpha.clamp(0.0, 1.0);
        }
        true
    }

    fn set_overlay_tint(&mut self, id: OverlayId, brightness: f32) -> bool {
        if self
            .instance
            .exts()
            .khr_composition_layer_color_scale_bias
            .is_none()
        {
            return false;
        }
        if let Some(o) = self.overlays.get_mut(&id.0) {
            let b = brightness.clamp(0.0, 1.0);
            (
                o.color_scale.color_scale.r,
                o.color_scale.color_scale.g,
                o.color_scale.color_scale.b,
            ) = (b, b, b);
        }
        true
    }

    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        if let Some(o) = self.overlays.get_mut(&id.0) {
            o.sort = order;
        }
    }

    fn destroy_overlay(&mut self, id: OverlayId) {
        if let Some(o) = self.overlays.remove(&id.0) {
            self.gpu.wait_idle();
            drop(o);
        }
    }

    fn overlay_image(&mut self, id: OverlayId) -> Option<(u32, u32, Vec<u8>)> {
        // 最後に書き込んだスワップチェーン画像。submit は acquire した画像へ書いて release するので、
        // 直近に書いたのは「次に acquire される一つ前」だが、ここでは最後に使った index を覚えている。
        let sc = self.overlays.get(&id.0)?.swapchain.as_ref()?;
        let image = sc.images[sc.last?];
        let rgba = self
            .gpu
            .download(
                image,
                sc.width,
                sc.height,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            )
            .ok()?;
        Some((sc.width, sc.height, rgba))
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "invalid RGBA buffer size"
        );
        if self.lost {
            return;
        }
        if let Err(e) = self.submit(id.0, width, height, rgba) {
            eprintln!("{e}");
        }
    }
}

impl Drop for OpenXrBackend {
    fn drop(&mut self) {
        // スワップチェーンが GPU で使われている間に破棄しない。
        self.gpu.wait_idle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posef_roundtrip() {
        let p = Pose::new(Vec3::new(0.1, 1.6, -0.5), Quat::from_rotation_y(0.7));
        let back = from_posef(to_posef(p));
        assert!(back.position.abs_diff_eq(p.position, 1e-6));
        assert!(back.orientation.abs_diff_eq(p.orientation, 1e-6));
    }

    #[test]
    fn action_names_have_no_slash() {
        for (_, inputs) in BINDINGS {
            for input in *inputs {
                let name = action_name(input);
                assert!(
                    name.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn touch_buttons_are_per_hand() {
        let touch = "oculus/touch_controller";
        assert!(on_hand(touch, "menu/click", Hand::Left));
        assert!(!on_hand(touch, "menu/click", Hand::Right));
        assert!(on_hand(touch, "b/click", Hand::Right));
        assert!(!on_hand(touch, "b/click", Hand::Left));
        assert!(on_hand(touch, "x/touch", Hand::Left));
        assert!(!on_hand(touch, "x/touch", Hand::Right));
        assert!(on_hand(touch, "a/touch", Hand::Right));
        assert!(!on_hand(touch, "a/touch", Hand::Left));
        assert!(on_hand("valve/index_controller", "b/click", Hand::Left));
    }
}
