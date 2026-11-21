//! Vulkan: CPU 上の RGBA を GPU イメージへ書き込む。OpenVR（`SetOverlayTexture`）と
//! OpenXR（スワップチェーン）で共有する。インスタンスとデバイスの作成方法のみがランタイムごとに異なる
//! （OpenXR はランタイム経由で作成する必要がある）ため、作成は呼び出し側に委譲する。

use std::ffi::{CStr, CString, c_char};

use ash::vk;

use crate::VrError;

mod import;

pub(crate) fn vk_err(what: &str) -> impl FnOnce(vk::Result) -> VrError + '_ {
    move |e| VrError::Runtime(format!("{what}: {e}"))
}

/// オーバーレイのピクセルフォーマット。GPUI の読み戻しは sRGB エンコードされた RGBA。
pub(crate) const FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

pub(crate) fn entry() -> Result<ash::Entry, VrError> {
    unsafe { ash::Entry::load() }.map_err(|e| VrError::Runtime(format!("Vulkan: {e}")))
}

/// スペース区切りの拡張名（OpenVR が返す形式）を分割して返す。
pub(crate) fn extension_names(list: &str) -> Vec<CString> {
    list.split_whitespace()
        .map(|s| CString::new(s).unwrap())
        .collect()
}

fn pointers(names: &[CString]) -> Vec<*const c_char> {
    names.iter().map(|n| n.as_ptr()).collect()
}

/// `create` に `VkInstanceCreateInfo` を渡してインスタンスを作成させる。
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

/// グラフィックス用キューを 1 本持つデバイスを `create` に作成させる。`extensions` に加え、表示サーバーの画像の
/// 取り込み（[`Gpu::convert`]）に使う拡張を、GPU が対応していれば有効にする。
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
    let mut enabled = extensions.to_vec();
    for name in import::extensions(&instance, physical) {
        if !enabled.iter().any(|e| e.as_c_str() == name) {
            enabled.push(name.to_owned());
        }
    }
    let ext = pointers(&enabled);
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
    Gpu::new(entry, instance, physical, device, family, &enabled)
}

/// 書き込み先として自前で確保した VkImage（OpenVR 用）。
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

/// 転送 1 回分のコマンドバッファ・フェンス・ステージングバッファ。順番に再利用する。
struct Slot {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    staging: Option<Staging>,
    /// サブミット済みだが完了未確認。
    busy: bool,
    /// 最後にサブミットした転送のシーケンス番号（[`Gpu::retire`]）。
    seq: u64,
}

/// GPU へ同時にサブミット可能な転送数。書き込みは完了を待たずに返り、この数だけ後に同じスロットを再利用する際に待機する。
/// ゲームが GPU を占有している間はフェンス待機が長引き、フレームループ全体が停止していた。
/// ランタイム（SteamVR / OpenXR）は渡したキューでイメージを読むため、キュー順序により書き込み後に読まれることが保証される。
// ponytail: 1 フレームの転送がこの数を超えると同一フレームの先行転送を待機する。パネルを大量に同時更新する場合は増やす。
const SLOTS: usize = 8;

/// GPU が使い終えるまで破棄を待つ資源。
enum Retired {
    Image(Image),
    Import(import::Transient),
}

pub(crate) struct Gpu {
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pool: vk::CommandPool,
    slots: Vec<Slot>,
    next: usize,
    /// サブミット済み転送のカウント（シーケンス番号）。
    submitted: u64,
    /// 破棄予約済みの資源と破棄時点のシーケンス番号。それ以降の転送が完了すれば、ランタイムの読み出しも完了している。
    retired: Vec<(u64, Retired)>,
    /// 表示サーバーの画像の取り込み（[`Gpu::convert`]）。拡張が無ければ `None`。
    import: Option<import::Importer>,
    /// Vulkan ライブラリ。関数ポインタの参照先のため、最後に解放する。
    _entry: ash::Entry,
}

impl Gpu {
    fn new(
        entry: ash::Entry,
        instance: ash::Instance,
        physical: vk::PhysicalDevice,
        device: ash::Device,
        queue_family: u32,
        enabled: &[CString],
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
            let import = import::Importer::new(&instance, physical, &device, enabled)
                .inspect_err(|e| logger::warn!("cannot import display server images", error = e))
                .ok()
                .flatten();
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
                import,
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
            // 取り込んだ画像は UNORM のビューで値をそのまま書き写す（[`Gpu::convert`]）。
            let (flags, usage) = if self.import.is_some() {
                (
                    vk::ImageCreateFlags::MUTABLE_FORMAT,
                    usage | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                )
            } else {
                (vk::ImageCreateFlags::empty(), usage)
            };
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .flags(flags)
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

    /// 次のスロットを取得する（前回そのスロットでサブミットした転送の完了を待機する）。
    fn next_slot(&mut self) -> Result<(usize, vk::CommandBuffer), VrError> {
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
            // キューは順に完了するため、この番号までの転送（およびその後にキューイングされたランタイムの読み出し）は完了済み。
            let done = slot.seq;
            let finished: Vec<Retired> = self
                .retired
                .extract_if(.., |(at, _)| *at < done)
                .map(|(_, r)| r)
                .collect();
            for retired in finished {
                self.free(retired);
            }
        }
        Ok((i, self.slots[i].cmd))
    }

    fn free(&self, retired: Retired) {
        match retired {
            Retired::Image(image) => self.destroy_image(image),
            Retired::Import(transient) => self.destroy_transient(transient),
        }
    }

    /// 次のスロットと、`size` 以上の CPU 可視ステージングバッファとその書き込み先ポインタを取得する。
    fn slot(
        &mut self,
        size: usize,
    ) -> Result<(usize, vk::CommandBuffer, vk::Buffer, *mut u8), VrError> {
        let (i, cmd) = self.next_slot()?;
        if let Some(s) = &self.slots[i].staging
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

    /// スロット `i` に記録したコマンドをサブミットする。完了は待機しない（次にそのスロットを使用する際に待機する）。
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

    /// 使用済みイメージを破棄予約する。ランタイムがまだ読み出し中の可能性があるため、後続の転送完了後に破棄する
    /// （サイズ変更のたびに GPU 全体のアイドル待ちを行うと、ゲーム中はそれだけで数十 ms のストールが発生していた）。
    pub fn retire(&mut self, image: Image) {
        self.retired.push((self.submitted, Retired::Image(image)));
    }

    /// `rgba`（行間パディングなし）を `image` 全体へ書き込み、`layout` に遷移させる。
    /// 既存の内容は破棄する。GPU の完了は待機しない（[`SLOTS`]）。
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

    /// `image`（`layout` 状態）の全体を RGBA で読み戻し、`layout` に復帰させる。テスト用の疎通確認。
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
        // 読み戻しは結果が必要なため、ここで完了を待機する。
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
        for (_, retired) in std::mem::take(&mut self.retired) {
            self.free(retired);
        }
        if let Some(import) = self.import.take() {
            import.destroy(&self.device);
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

/// C 文字列バッファ（NUL 終端）を `String` に変換する。
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
