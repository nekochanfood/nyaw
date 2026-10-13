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
        .ok_or_else(|| VrError::Runtime("Vulkan: グラフィックス用のキューが無い".into()))?
        as u32;
    let priorities = [1.0];
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&priorities)];
    let ext = pointers(extensions);
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&ext);
    let raw = create(&instance, &info)?;
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

pub(crate) struct Gpu {
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    staging: Option<Staging>,
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
            let pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(queue_family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(vk_err("vkCreateCommandPool"))?;
            let cmd = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .command_buffer_count(1),
                )
                .map_err(vk_err("vkAllocateCommandBuffers"))?[0];
            let fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(vk_err("vkCreateFence"))?;
            Ok(Self {
                instance,
                physical,
                device,
                queue,
                queue_family,
                pool,
                cmd,
                fence,
                staging: None,
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
            .ok_or_else(|| VrError::Runtime("Vulkan: 適したメモリの種類が無い".into()))
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
            self.device
                .bind_image_memory(image, memory, 0)
                .map_err(vk_err("vkBindImageMemory"))?;
            Ok(Image { image, memory })
        }
    }

    pub fn destroy_image(&self, image: Image) {
        unsafe {
            self.device.destroy_image(image.image, None);
            self.device.free_memory(image.memory, None);
        }
    }

    /// 書き込みに使う、CPU から見える一時バッファを `size` 以上にする。
    fn staging(&mut self, size: usize) -> Result<*mut u8, VrError> {
        if let Some(s) = &self.staging
            && s.size >= size
        {
            return Ok(s.mapped);
        }
        self.drop_staging();
        unsafe {
            let buffer = self
                .device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size as u64)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC),
                    None,
                )
                .map_err(vk_err("vkCreateBuffer"))?;
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
            self.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(vk_err("vkBindBufferMemory"))?;
            let mapped = self
                .device
                .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .map_err(vk_err("vkMapMemory"))? as *mut u8;
            self.staging = Some(Staging {
                buffer,
                memory,
                size,
                mapped,
            });
            Ok(mapped)
        }
    }

    fn drop_staging(&mut self) {
        if let Some(s) = self.staging.take() {
            unsafe {
                self.device.destroy_buffer(s.buffer, None);
                self.device.free_memory(s.memory, None);
            }
        }
    }

    /// `rgba`（行間に詰め物なし）を `image` 全体へ書き込み、`layout` に遷移させる。
    /// 以前の中身は捨てる。
    // ponytail: 書き込みのたびに GPU の完了を待つ（1 枚あたり数百 µs）。
    // パネルが増えて重くなったら、待たずにフェンスを次のフレームで確認する。
    pub fn upload(
        &mut self,
        image: vk::Image,
        width: u32,
        height: u32,
        rgba: &[u8],
        layout: vk::ImageLayout,
    ) -> Result<(), VrError> {
        let staging = self.staging(rgba.len())?;
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
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(vk_err("vkBeginCommandBuffer"))?;
            d.cmd_pipeline_barrier(
                self.cmd,
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
                self.cmd,
                self.staging.as_ref().unwrap().buffer,
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
                self.cmd,
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
            d.end_command_buffer(self.cmd)
                .map_err(vk_err("vkEndCommandBuffer"))?;
            let cmds = [self.cmd];
            d.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                self.fence,
            )
            .map_err(vk_err("vkQueueSubmit"))?;
            d.wait_for_fences(&[self.fence], true, u64::MAX)
                .map_err(vk_err("vkWaitForFences"))?;
            d.reset_fences(&[self.fence])
                .map_err(vk_err("vkResetFences"))?;
        }
        Ok(())
    }

    pub fn wait_idle(&self) {
        let _ = unsafe { self.device.device_wait_idle() };
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        self.wait_idle();
        self.drop_staging();
        unsafe {
            self.device.destroy_fence(self.fence, None);
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
