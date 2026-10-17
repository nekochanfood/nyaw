//! Vulkan: CPU 上の RGBA を GPU のイメージへ書き込む。OpenVR（`SetOverlayTexture`）と
//! OpenXR（スワップチェーン）で共有する。インスタンスとデバイスの作り方だけがランタイムごとに違う
//! （OpenXR はランタイム経由で作る必要がある）ので、作成は呼び出し側に任せる。

use std::ffi::{CStr, CString, c_char};

use ash::vk;

use crate::VrError;

pub(crate) fn vk_err(what: &str) -> impl FnOnce(vk::Result) -> VrError + '_ {
    move |e| VrError::Runtime(format!("{what}: {e}"))
}

/// オーバーレイの画素の形式。GPUI の読み戻しは sRGB で符号化された RGBA。
pub(crate) const FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

pub(crate) fn entry() -> Result<ash::Entry, VrError> {
    unsafe { ash::Entry::load() }.map_err(|e| VrError::Runtime(format!("Vulkan: {e}")))
}

/// スペース区切りの拡張名（OpenVR が返す形式）を分割する。
pub(crate) fn extension_names(list: &str) -> Vec<CString> {
    list.split_whitespace()
        .map(|s| CString::new(s).unwrap())
        .collect()
}

fn pointers(names: &[CString]) -> Vec<*const c_char> {
    names.iter().map(|n| n.as_ptr()).collect()
}

/// `create` に `VkInstanceCreateInfo` を渡してインスタンスを作らせる。
pub(crate) fn create_instance(
    entry: &ash::Entry,
    extensions: &[CString],
    create: impl FnOnce(&vk::InstanceCreateInfo) -> Result<vk::Instance, VrError>,
) -> Result<ash::Instance, VrError> {
    let app = vk::ApplicationInfo::default()
        .application_name(c"nyaw")
        .api_version(vk::API_VERSION_1_1);
    let ext = pointers(extensions);
    let info = vk::InstanceCreateInfo::default()
        .application_info(&app)
        .enabled_extension_names(&ext);
    let raw = create(&info)?;
    Ok(unsafe { ash::Instance::load(entry.static_fn(), raw) })
}

/// グラフィックス用のキューを 1 本持つデバイスを `create` に作らせる。
pub(crate) fn create_device(
    entry: ash::Entry,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    extensions: &[CString],
    create: impl FnOnce(&ash::Instance, &vk::DeviceCreateInfo) -> Result<vk::Device, VrError>,
) -> Result<Gpu, VrError> {
    let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
        .iter()
        .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .map(|i| i as u32);
    let Some(family) = family else {
        unsafe { instance.destroy_instance(None) };
        return Err(VrError::Runtime("Vulkan: no graphics queue".into()));
    };
    let priorities = [1.0];
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&priorities)];
    let ext = pointers(extensions);
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&ext);
    let raw = match create(&instance, &info) {
        Ok(raw) => raw,
        Err(e) => {
            unsafe { instance.destroy_instance(None) };
            return Err(e);
        }
    };
    let device = unsafe { ash::Device::load(instance.fp_v1_0(), raw) };
    Gpu::new(entry, instance, physical, device, family)
}

/// 書き込み先として自前で確保したイメージ（OpenVR 用）。
pub(crate) struct Image {
    pub image: vk::Image,
    memory: vk::DeviceMemory,
}

struct Staging {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: usize,
    mapped: *mut u8,
}

/// 転送 1 回分のコマンドバッファ・フェンス・一時バッファ。順番に使い回す。
struct Slot {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    staging: Option<Staging>,
    /// 送ったまま完了を確かめていない。
    busy: bool,
    /// 最後に送った転送の通し番号（[`Gpu::retire`]）。
    seq: u64,
}

/// 同時に GPU へ出しておける転送の数。書き込みは完了を待たずに返り、この数だけ後に同じ枠を使うときに待つ。
/// ゲームが GPU を使い切っている間は完了待ちが長く伸び、フレームループごと止まっていた。
/// ランタイム（SteamVR / OpenXR）は渡したキューでイメージを読むので、キューの順序で書き込みの後に読まれる。
// ponytail: 1 フレームの転送がこれを超えると同じフレームの前の転送を待つ。パネルを大量に同時更新するなら増やす。
const SLOTS: usize = 8;

pub(crate) struct Gpu {
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pool: vk::CommandPool,
    slots: Vec<Slot>,
    next: usize,
    /// 送った転送の数（通し番号）。
    submitted: u64,
    /// 捨てたイメージと、捨てた時点の通し番号。それより後の転送が終われば、ランタイムの読み出しも済んでいる。
    retired: Vec<(u64, Image)>,
    /// Vulkan のライブラリ。関数ポインタが指す先なので最後に解放する。
    _entry: ash::Entry,
}

impl Gpu {
    fn new(
        entry: ash::Entry,
        instance: ash::Instance,
        physical: vk::PhysicalDevice,
        device: ash::Device,
        queue_family: u32,
    ) -> Result<Self, VrError> {
        unsafe {
            let queue = device.get_device_queue(queue_family, 0);
            let mut pool = vk::CommandPool::null();
            let mut slots = Vec::new();
            let created = (|| -> Result<(), VrError> {
                pool = device
                    .create_command_pool(
                        &vk::CommandPoolCreateInfo::default()
                            .queue_family_index(queue_family)
                            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                        None,
                    )
                    .map_err(vk_err("vkCreateCommandPool"))?;
                let cmds = device
                    .allocate_command_buffers(
                        &vk::CommandBufferAllocateInfo::default()
                            .command_pool(pool)
                            .command_buffer_count(SLOTS as u32),
                    )
                    .map_err(vk_err("vkAllocateCommandBuffers"))?;
                for cmd in cmds {
                    let fence = device
                        .create_fence(&vk::FenceCreateInfo::default(), None)
                        .map_err(vk_err("vkCreateFence"))?;
                    slots.push(Slot {
                        cmd,
                        fence,
                        staging: None,
                        busy: false,
                        seq: 0,
                    });
                }
                Ok(())
            })();
            if let Err(e) = created {
                for slot in slots {
                    device.destroy_fence(slot.fence, None);
                }
                if pool != vk::CommandPool::null() {
                    device.destroy_command_pool(pool, None);
                }
                device.destroy_device(None);
                instance.destroy_instance(None);
                return Err(e);
            }
            Ok(Self {
                instance,
                physical,
                device,
                queue,
                queue_family,
                pool,
                slots,
                next: 0,
                submitted: 0,
                retired: Vec::new(),
                _entry: entry,
            })
        }
    }

    fn memory_type(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> Result<u32, VrError> {
        let props = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical)
        };
        (0..props.memory_type_count)
            .find(|&i| {
                bits & (1 << i) != 0
                    && props.memory_types[i as usize]
                        .property_flags
                        .contains(flags)
            })
            .ok_or_else(|| VrError::Runtime("Vulkan: no suitable memory type".into()))
    }

    pub fn create_image(
        &self,
        width: u32,
        height: u32,
        usage: vk::ImageUsageFlags,
    ) -> Result<Image, VrError> {
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(FORMAT)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage),
                    None,
                )
                .map_err(vk_err("vkCreateImage"))?;
            let result = (|| {
                let req = self.device.get_image_memory_requirements(image);
                let memory = self
                    .device
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default()
                            .allocation_size(req.size)
                            .memory_type_index(self.memory_type(
                                req.memory_type_bits,
                                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                            )?),
                        None,
                    )
                    .map_err(vk_err("vkAllocateMemory"))?;
                if let Err(e) = self.device.bind_image_memory(image, memory, 0) {
                    self.device.free_memory(memory, None);
                    return Err(vk_err("vkBindImageMemory")(e));
                }
                Ok(Image { image, memory })
            })();
            if result.is_err() {
                self.device.destroy_image(image, None);
            }
            result
        }
    }

    pub fn destroy_image(&self, image: Image) {
        unsafe {
            self.device.destroy_image(image.image, None);
            self.device.free_memory(image.memory, None);
        }
    }

    /// 次の枠を取る（前にその枠で送った転送の完了を待つ）。CPU から見える一時バッファは `size` 以上にする。
    /// 枠の番号・コマンドバッファ・一時バッファとその書き込み先を返す。
    fn slot(
        &mut self,
        size: usize,
    ) -> Result<(usize, vk::CommandBuffer, vk::Buffer, *mut u8), VrError> {
        let i = self.next;
        self.next = (i + 1) % SLOTS;
        let slot = &mut self.slots[i];
        if slot.busy {
            unsafe {
                self.device
                    .wait_for_fences(&[slot.fence], true, u64::MAX)
                    .map_err(vk_err("vkWaitForFences"))?;
                self.device
                    .reset_fences(&[slot.fence])
                    .map_err(vk_err("vkResetFences"))?;
            }
            slot.busy = false;
            // キューは順に終わるので、この番号までの転送（とその後ろに積まれたランタイムの読み出し）は済んでいる。
            let done = slot.seq;
            for (_, image) in self.retired.extract_if(.., |(at, _)| *at < done) {
                unsafe {
                    self.device.destroy_image(image.image, None);
                    self.device.free_memory(image.memory, None);
                }
            }
        }
        let slot = &mut self.slots[i];
        let cmd = slot.cmd;
        if let Some(s) = &slot.staging
            && s.size >= size
        {
            return Ok((i, cmd, s.buffer, s.mapped));
        }
        if let Some(s) = self.slots[i].staging.take() {
            self.drop_staging(s);
        }
        unsafe {
            let buffer = self
                .device
                .create_buffer(
                    &vk::BufferCreateInfo::default().size(size as u64).usage(
                        vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST,
                    ),
                    None,
                )
                .map_err(vk_err("vkCreateBuffer"))?;
            let result = (|| {
                let req = self.device.get_buffer_memory_requirements(buffer);
                let memory = self
                    .device
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default()
                            .allocation_size(req.size)
                            .memory_type_index(self.memory_type(
                                req.memory_type_bits,
                                vk::MemoryPropertyFlags::HOST_VISIBLE
                                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                            )?),
                        None,
                    )
                    .map_err(vk_err("vkAllocateMemory"))?;
                Ok::<_, VrError>(memory)
            })();
            let memory = match result {
                Ok(memory) => memory,
                Err(e) => {
                    self.device.destroy_buffer(buffer, None);
                    return Err(e);
                }
            };
            let result = self
                .device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(vk_err("vkBindBufferMemory"))
                .and_then(|_| {
                    self.device
                        .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                        .map_err(vk_err("vkMapMemory"))
                });
            let mapped = match result {
                Ok(mapped) => mapped as *mut u8,
                Err(e) => {
                    self.device.destroy_buffer(buffer, None);
                    self.device.free_memory(memory, None);
                    return Err(e);
                }
            };
            self.slots[i].staging = Some(Staging {
                buffer,
                memory,
                size,
                mapped,
            });
            Ok((i, cmd, buffer, mapped))
        }
    }

    fn drop_staging(&self, s: Staging) {
        unsafe {
            self.device.destroy_buffer(s.buffer, None);
            self.device.free_memory(s.memory, None);
        }
    }

    /// 枠 `i` に記録したコマンドを送る。完了は待たない（次にその枠を使うときに待つ）。
    fn submit(&mut self, i: usize) -> Result<(), VrError> {
        let slot = &mut self.slots[i];
        let cmds = [slot.cmd];
        unsafe {
            self.device
                .queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&cmds)],
                    slot.fence,
                )
                .map_err(vk_err("vkQueueSubmit"))?;
        }
        self.submitted += 1;
        (slot.busy, slot.seq) = (true, self.submitted);
        Ok(())
    }

    /// 使い終えたイメージを捨てる。ランタイムがまだ読んでいるかもしれないので、後の転送が終わってから破棄する
    /// （大きさが変わるたびに GPU 全体の完了を待つと、ゲーム中はそれだけで数十 ms 止まっていた）。
    pub fn retire(&mut self, image: Image) {
        self.retired.push((self.submitted, image));
    }

    /// `rgba`（行間に詰め物なし）を `image` 全体へ書き込み、`layout` に遷移させる。
    /// 以前の中身は捨てる。GPU の完了は待たない（[`SLOTS`]）。
    pub fn upload(
        &mut self,
        image: vk::Image,
        width: u32,
        height: u32,
        rgba: &[u8],
        layout: vk::ImageLayout,
    ) -> Result<(), VrError> {
        let (slot, cmd, buffer, staging) = self.slot(rgba.len())?;
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), staging, rgba.len());
            let d = &self.device;
            let range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1);
            let barrier = |old, new, src, dst| {
                vk::ImageMemoryBarrier::default()
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
            };
            d.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(vk_err("vkBeginCommandBuffer"))?;
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                )],
            );
            d.cmd_copy_buffer_to_image(
                cmd,
                buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })],
            );
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    layout,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::MEMORY_READ,
                )],
            );
            d.end_command_buffer(cmd)
                .map_err(vk_err("vkEndCommandBuffer"))?;
        }
        self.submit(slot)
    }

    /// `image`（`layout` の状態）の全体を RGBA で読み戻し、`layout` に戻す。テスト用の疎通確認。
    pub fn download(
        &mut self,
        image: vk::Image,
        width: u32,
        height: u32,
        layout: vk::ImageLayout,
    ) -> Result<Vec<u8>, VrError> {
        let len = (width * height * 4) as usize;
        let (slot, cmd, buffer, staging) = self.slot(len)?;
        unsafe {
            let d = &self.device;
            let range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1);
            let barrier = |old, new, src, dst| {
                vk::ImageMemoryBarrier::default()
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
            };
            let transition = |from, to, src, dst| {
                d.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(from, to, src, dst)],
                )
            };
            d.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(vk_err("vkBeginCommandBuffer"))?;
            transition(
                layout,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::AccessFlags::MEMORY_WRITE,
                vk::AccessFlags::TRANSFER_READ,
            );
            d.cmd_copy_image_to_buffer(
                cmd,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })],
            );
            transition(
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                layout,
                vk::AccessFlags::TRANSFER_READ,
                vk::AccessFlags::MEMORY_READ,
            );
            d.end_command_buffer(cmd)
                .map_err(vk_err("vkEndCommandBuffer"))?;
        }
        self.submit(slot)?;
        // 読み戻しは結果が要るのでここで待つ。
        self.wait_idle();
        Ok(unsafe { std::slice::from_raw_parts(staging, len) }.to_vec())
    }

    pub fn wait_idle(&self) {
        let _ = unsafe { self.device.device_wait_idle() };
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        self.wait_idle();
        for (_, image) in std::mem::take(&mut self.retired) {
            self.destroy_image(image);
        }
        for slot in std::mem::take(&mut self.slots) {
            if let Some(s) = slot.staging {
                self.drop_staging(s);
            }
            unsafe { self.device.destroy_fence(slot.fence, None) };
        }
        unsafe {
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// C 文字列のバッファ（NUL 終端）を `String` にする。
pub(crate) fn c_buf_to_string(buf: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_space_separated_extensions() {
        let v = extension_names("VK_KHR_a  VK_KHR_b\n");
        assert_eq!(v, [c"VK_KHR_a".to_owned(), c"VK_KHR_b".to_owned()]);
    }
}
