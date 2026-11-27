//! 表示サーバーの画像（DMA-BUF・共有メモリ）をこの GPU へ取り込み、オーバーレイのイメージへ RGBA で書き写す。
//! DMA-BUF は VkImage として、共有メモリはテクセルバッファとして CPU を介さずに読み、全面を覆う三角形 1 枚の描画で
//! 並びを揃える（透明度の無い形式は不透明にする）。書き先はオーバーレイのイメージの UNORM のビュー（値をそのまま写す）。

#[cfg(target_os = "linux")]
use std::os::{
    fd::{AsRawFd, FromRawFd, IntoRawFd},
    unix::fs::MetadataExt,
};
use std::{
    ffi::{CStr, CString, c_void},
    sync::Arc,
    time::{Duration, Instant},
};

use ash::{ext, khr, vk};

use super::{Gpu, Retired, SLOTS, vk_err};
use crate::{ExternalImage, ImportFormats, Memory, PixelFormat, VrError};

// SPIR-V は shaders/ の GLSL から `glslc -O --target-env=vulkan1.1 <file> -o <file>.spv` で生成する。
const VERTEX: &[u8] = include_bytes!("shaders/fullscreen.vert.spv");
const IMAGE: &[u8] = include_bytes!("shaders/image.frag.spv");
const BUFFER: &[u8] = include_bytes!("shaders/buffer.frag.spv");

/// DMA-BUF の取り込みに要る拡張（すべて揃ったときだけ有効にする）。
const DMABUF: [&CStr; 5] = [
    khr::external_memory_fd::NAME,
    ext::external_memory_dma_buf::NAME,
    ext::image_drm_format_modifier::NAME,
    khr::image_format_list::NAME,
    ext::queue_family_foreign::NAME,
];
const HOST: &CStr = ext::external_memory_host::NAME;

/// この GPU で使える取り込みの拡張。
pub(super) fn extensions(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
) -> Vec<&'static CStr> {
    let available =
        unsafe { instance.enumerate_device_extension_properties(physical) }.unwrap_or_default();
    let has = |name: &CStr| {
        available
            .iter()
            .any(|e| e.extension_name_as_c_str() == Ok(name))
    };
    let mut wanted = Vec::new();
    if DMABUF.iter().all(|n| has(n)) {
        wanted.extend(DMABUF);
    }
    if has(HOST) {
        wanted.push(HOST);
    }
    wanted
}

/// 取り込む画像を読むときの Vulkan の形式（`bgr` は B と R の並びをイメージの形式で解釈させる）。
fn vk_format(format: PixelFormat) -> vk::Format {
    if format.bgr() {
        vk::Format::B8G8R8A8_UNORM
    } else {
        vk::Format::R8G8B8A8_UNORM
    }
}

/// シェーダーへ渡す値（`shaders/*.frag` の `Params`）。
#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    opaque: u32,
    bgr: u32,
    /// 共有メモリの行あたりの画素数と、先頭の画素の位置（テクセル単位）。
    stride: u32,
    offset: u32,
}

/// 読み元の種類（パイプラインとデスクリプタの添字）。
const KIND_IMAGE: usize = 0;
const KIND_BUFFER: usize = 1;

pub(crate) struct Importer {
    pub formats: ImportFormats,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fd: Option<khr::external_memory_fd::Device>,
    host: Option<ext::external_memory_host::Device>,
    max_texels: u32,
    sampler: vk::Sampler,
    set_layouts: [vk::DescriptorSetLayout; 2],
    layouts: [vk::PipelineLayout; 2],
    pipelines: [vk::Pipeline; 2],
    render_pass: vk::RenderPass,
    pool: vk::DescriptorPool,
    /// 転送のスロットごとの、読み元の種類ごとのデスクリプタセット（スロットの完了を待ってから書き換える）。
    sets: Vec<[vk::DescriptorSet; 2]>,
}

/// GPU が読み終えるまで残す、1 回の取り込みの資源。
pub(crate) struct Transient {
    /// 使い回さない読み元（共有メモリ）か、使い回さなくなった読み元（[`Cached`]）。
    source: Option<(Source, vk::DeviceMemory)>,
    target: vk::ImageView,
    framebuffer: vk::Framebuffer,
    /// 書き込み元へ返すのは、GPU が読み終えてから。
    _image: Option<Arc<ExternalImage>>,
}

/// 取り込んだ DMA-BUF を引く値。同じバッファは同じ inode を持ち、取り込んでいる間は別のバッファに使われない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct Key {
    dev: u64,
    ino: u64,
    modifier: u64,
    offset: u32,
    stride: u32,
    width: u32,
    height: u32,
    format: PixelFormat,
}

/// 使い回す DMA-BUF の取り込み。表示サーバーは少数のバッファを順に使うため、取り込みは 1 回で済む
/// （毎回取り込むと 1920×1080 で約 0.7ms かかっていた）。
pub(crate) struct Cached {
    key: Key,
    source: Source,
    memory: vk::DeviceMemory,
    used: Instant,
}

/// 使い回す取り込みの上限と、使われなくなってから手放すまでの時間。
const CACHE: usize = 16;
const UNUSED: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
enum Source {
    // 作るのは DMA-BUF の取り込み（Linux のみ）だけである。
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Image(vk::Image, vk::ImageView),
    Buffer(vk::Buffer, vk::BufferView),
}

impl Importer {
    /// 有効にした拡張 `enabled` から取り込みの手段を用意する。どちらの手段も無ければ `None`。
    pub fn new(
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        device: &ash::Device,
        enabled: &[CString],
    ) -> Result<Option<Self>, VrError> {
        let on = |name: &CStr| enabled.iter().any(|e| e.as_c_str() == name);
        let dmabuf = DMABUF.iter().all(|n| on(n));
        let host = on(HOST);
        if !dmabuf && !host {
            return Ok(None);
        }
        let mut host_props = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
        let mut props = vk::PhysicalDeviceProperties2::default();
        if host {
            props = props.push_next(&mut host_props);
        }
        unsafe { instance.get_physical_device_properties2(physical, &mut props) };
        let max_texels = props.properties.limits.max_texel_buffer_elements;
        let host_alignment =
            host.then_some(host_props.min_imported_host_pointer_alignment as usize);
        let dmabuf_formats = if dmabuf {
            PixelFormat::ALL
                .into_iter()
                .map(|f| (f, modifiers(instance, physical, vk_format(f))))
                .filter(|(_, m)| !m.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        let formats = ImportFormats {
            dmabuf: dmabuf_formats,
            host_alignment,
        };
        let mut importer = Self {
            formats,
            fd: dmabuf.then(|| khr::external_memory_fd::Device::new(instance, device)),
            host: host.then(|| ext::external_memory_host::Device::new(instance, device)),
            max_texels,
            sampler: vk::Sampler::null(),
            set_layouts: [vk::DescriptorSetLayout::null(); 2],
            layouts: [vk::PipelineLayout::null(); 2],
            pipelines: [vk::Pipeline::null(); 2],
            render_pass: vk::RenderPass::null(),
            pool: vk::DescriptorPool::null(),
            sets: Vec::new(),
        };
        if let Err(e) = importer.create(device) {
            importer.destroy(device);
            return Err(e);
        }
        Ok(Some(importer))
    }

    fn create(&mut self, device: &ash::Device) -> Result<(), VrError> {
        unsafe {
            self.sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .map_err(vk_err("vkCreateSampler"))?;
            for (kind, ty) in [
                (KIND_IMAGE, vk::DescriptorType::COMBINED_IMAGE_SAMPLER),
                (KIND_BUFFER, vk::DescriptorType::UNIFORM_TEXEL_BUFFER),
            ] {
                let binding = [vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(ty)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
                self.set_layouts[kind] = device
                    .create_descriptor_set_layout(
                        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding),
                        None,
                    )
                    .map_err(vk_err("vkCreateDescriptorSetLayout"))?;
                let push = [vk::PushConstantRange::default()
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                    .size(size_of::<Params>() as u32)];
                let sets = [self.set_layouts[kind]];
                self.layouts[kind] = device
                    .create_pipeline_layout(
                        &vk::PipelineLayoutCreateInfo::default()
                            .set_layouts(&sets)
                            .push_constant_ranges(&push),
                        None,
                    )
                    .map_err(vk_err("vkCreatePipelineLayout"))?;
            }
            let attachment = [vk::AttachmentDescription::default()
                .format(vk::Format::R8G8B8A8_UNORM)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let color = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color)];
            let dependency = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::ALL_COMMANDS)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
            self.render_pass = device
                .create_render_pass(
                    &vk::RenderPassCreateInfo::default()
                        .attachments(&attachment)
                        .subpasses(&subpass)
                        .dependencies(&dependency),
                    None,
                )
                .map_err(vk_err("vkCreateRenderPass"))?;
            let module = |code: &[u8]| {
                let words: Vec<u32> = code
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|w| u32::from_le_bytes(*w))
                    .collect();
                device
                    .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
                    .map_err(vk_err("vkCreateShaderModule"))
            };
            let vertex = module(VERTEX)?;
            let mut fragments = Vec::new();
            let result = (|| {
                for (kind, code) in [IMAGE, BUFFER].into_iter().enumerate() {
                    let fragment = module(code)?;
                    fragments.push(fragment);
                    self.pipelines[kind] = self.pipeline(device, vertex, fragment, kind)?;
                }
                Ok::<_, VrError>(())
            })();
            device.destroy_shader_module(vertex, None);
            for fragment in fragments {
                device.destroy_shader_module(fragment, None);
            }
            result?;
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(SLOTS as u32),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_TEXEL_BUFFER)
                    .descriptor_count(SLOTS as u32),
            ];
            self.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(2 * SLOTS as u32)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(vk_err("vkCreateDescriptorPool"))?;
            for _ in 0..SLOTS {
                let sets = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(self.pool)
                            .set_layouts(&self.set_layouts),
                    )
                    .map_err(vk_err("vkAllocateDescriptorSets"))?;
                self.sets.push([sets[0], sets[1]]);
            }
        }
        Ok(())
    }

    fn pipeline(
        &self,
        device: &ash::Device,
        vertex: vk::ShaderModule,
        fragment: vk::ShaderModule,
        kind: usize,
    ) -> Result<vk::Pipeline, VrError> {
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment)
                .name(c"main"),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layouts[kind])
            .render_pass(self.render_pass)];
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) }
            .map(|p| p[0])
            .map_err(|(_, e)| vk_err("vkCreateGraphicsPipelines")(e))
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            for pipeline in self.pipelines {
                device.destroy_pipeline(pipeline, None);
            }
            for layout in self.layouts {
                device.destroy_pipeline_layout(layout, None);
            }
            for layout in self.set_layouts {
                device.destroy_descriptor_set_layout(layout, None);
            }
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_sampler(self.sampler, None);
        }
    }
}

/// `format` で取り込める DMA-BUF の修飾子（単一プレーンで、サンプリングでき、取り込めるもの）。
fn modifiers(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    format: vk::Format,
) -> Vec<u64> {
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
    let mut props = vk::FormatProperties2::default().push_next(&mut list);
    unsafe { instance.get_physical_device_format_properties2(physical, format, &mut props) };
    let mut found = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        list.drm_format_modifier_count as usize
    ];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut found);
    let mut props = vk::FormatProperties2::default().push_next(&mut list);
    unsafe { instance.get_physical_device_format_properties2(physical, format, &mut props) };
    found
        .iter()
        .filter(|m| {
            m.drm_format_modifier_plane_count == 1
                && m.drm_format_modifier_tiling_features
                    .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
                && importable(instance, physical, format, m.drm_format_modifier)
        })
        .map(|m| m.drm_format_modifier)
        .collect()
}

fn importable(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    format: vk::Format,
    modifier: u64,
) -> bool {
    let mut modifier_info = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
        .drm_format_modifier(modifier)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let mut external = vk::PhysicalDeviceExternalImageFormatInfo::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let info = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(format)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(vk::ImageUsageFlags::SAMPLED)
        .push_next(&mut external)
        .push_next(&mut modifier_info);
    let mut external_props = vk::ExternalImageFormatProperties::default();
    let mut props = vk::ImageFormatProperties2::default().push_next(&mut external_props);
    unsafe { instance.get_physical_device_image_format_properties2(physical, &info, &mut props) }
        .is_ok()
        && external_props
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
}

impl Gpu {
    /// 取り込める外部の画像（取り込みの拡張が無ければ `None`）。
    pub fn import_formats(&self) -> Option<ImportFormats> {
        self.import.as_ref().map(|i| i.formats.clone())
    }

    /// `image` を取り込み、`target`（[`Gpu::create_image`] の書き換え可能な形式のイメージか、可変形式の
    /// スワップチェーンのイメージ）の全体へ RGBA で書き写して `layout` にする。GPU の完了は待たず、読み終えるまで
    /// `image` を保持する。
    pub fn convert(
        &mut self,
        target: vk::Image,
        image: &Arc<ExternalImage>,
        layout: vk::ImageLayout,
    ) -> Result<(), VrError> {
        if self.import.is_none() {
            return Err(VrError::Runtime("Vulkan: no external memory import".into()));
        }
        self.collect();
        let (slot, cmd) = self.next_slot()?;
        self.forget_unused();
        let key = key(image);
        let cached = key.and_then(|k| self.cache.iter_mut().find(|c| c.key == k));
        let (source, owned) = match cached {
            Some(c) => {
                c.used = Instant::now();
                (c.source, None)
            }
            None => {
                let importer = self.import.as_ref().unwrap();
                let (source, memory) = self.import_source(importer, image)?;
                match key {
                    Some(key) => {
                        self.remember(Cached {
                            key,
                            source,
                            memory,
                            used: Instant::now(),
                        });
                        (source, None)
                    }
                    None => (source, Some((source, memory))),
                }
            }
        };
        let importer = self.import.as_ref().unwrap();
        let transient = match self.record(importer, slot, cmd, target, image, &source, layout) {
            Ok((target, framebuffer)) => Transient {
                source: owned,
                target,
                framebuffer,
                _image: Some(image.clone()),
            },
            Err(e) => {
                if let Some((source, memory)) = owned {
                    self.destroy_source(source, memory);
                }
                return Err(e);
            }
        };
        if let Err(e) = self.submit(slot) {
            self.destroy_transient(transient);
            return Err(e);
        }
        self.retired
            .push((self.submitted, Retired::Import(transient)));
        Ok(())
    }

    /// 取り込みを使い回せるようにする。上限を超えたら最も古いものを手放す。
    fn remember(&mut self, cached: Cached) {
        if self.cache.len() >= CACHE
            && let Some(oldest) = self
                .cache
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| c.used)
                .map(|(i, _)| i)
        {
            let old = self.cache.swap_remove(oldest);
            self.retire_cached(old);
        }
        self.cache.push(cached);
    }

    /// しばらく使われていない取り込み（止まった配信や、交渉し直して捨てられたバッファ）を手放す。
    fn forget_unused(&mut self) {
        let now = Instant::now();
        let unused: Vec<Cached> = self
            .cache
            .extract_if(.., |c| now.duration_since(c.used) > UNUSED)
            .collect();
        for cached in unused {
            self.retire_cached(cached);
        }
    }

    /// 送信中のコマンドが読み終えてから破棄する。
    fn retire_cached(&mut self, cached: Cached) {
        let transient = Transient {
            source: Some((cached.source, cached.memory)),
            target: vk::ImageView::null(),
            framebuffer: vk::Framebuffer::null(),
            _image: None,
        };
        self.retired
            .push((self.submitted, Retired::Import(transient)));
    }

    /// 破棄時にすべての取り込みを手放す（GPU は待機済み）。
    pub(super) fn forget_all(&mut self) {
        for cached in std::mem::take(&mut self.cache) {
            self.destroy_source(cached.source, cached.memory);
        }
    }

    fn import_source(
        &self,
        importer: &Importer,
        image: &ExternalImage,
    ) -> Result<(Source, vk::DeviceMemory), VrError> {
        match &image.memory {
            #[cfg(target_os = "linux")]
            Memory::DmaBuf {
                fd,
                modifier,
                offset,
                stride,
            } => {
                let ext = importer
                    .fd
                    .as_ref()
                    .ok_or_else(|| VrError::Runtime("Vulkan: no DMA-BUF import".into()))?;
                self.import_dmabuf(ext, image, fd.as_raw_fd(), *modifier, *offset, *stride)
            }
            Memory::Host {
                ptr,
                len,
                offset,
                stride,
            } => {
                let ext = importer
                    .host
                    .as_ref()
                    .ok_or_else(|| VrError::Runtime("Vulkan: no host memory import".into()))?;
                let texels = *len / 4;
                if texels > importer.max_texels as usize || offset % 4 != 0 || stride % 4 != 0 {
                    return Err(VrError::Runtime(
                        "Vulkan: shared memory image is too large or misaligned".into(),
                    ));
                }
                self.import_host(ext, image.format, *ptr, *len)
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn import_dmabuf(
        &self,
        ext: &khr::external_memory_fd::Device,
        image: &ExternalImage,
        fd: i32,
        modifier: u64,
        offset: u32,
        stride: u32,
    ) -> Result<(Source, vk::DeviceMemory), VrError> {
        let d = &self.device;
        let format = vk_format(image.format);
        let planes = [vk::SubresourceLayout::default()
            .offset(offset.into())
            .row_pitch(stride.into())];
        let mut explicit = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(modifier)
            .plane_layouts(&planes);
        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: image.width,
                height: image.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .push_next(&mut external)
            .push_next(&mut explicit);
        let vk_image = unsafe { d.create_image(&info, None) }.map_err(vk_err("vkCreateImage"))?;
        let memory = (|| unsafe {
            let mut fd_props = vk::MemoryFdPropertiesKHR::default();
            ext.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                fd,
                &mut fd_props,
            )
            .map_err(vk_err("vkGetMemoryFdPropertiesKHR"))?;
            let req = d.get_image_memory_requirements(vk_image);
            let bits = req.memory_type_bits & fd_props.memory_type_bits;
            let index = self.memory_type(bits, vk::MemoryPropertyFlags::empty())?;
            // 取り込みに成功すると Vulkan が fd を所有するので、複製を渡す。
            let owned = std::os::fd::BorrowedFd::borrow_raw(fd)
                .try_clone_to_owned()
                .map_err(|e| VrError::Runtime(format!("dup DMA-BUF: {e}")))?;
            let raw = owned.into_raw_fd();
            let mut import = vk::ImportMemoryFdInfoKHR::default()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .fd(raw);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(vk_image);
            let allocated = d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(index)
                    .push_next(&mut import)
                    .push_next(&mut dedicated),
                None,
            );
            let memory = match allocated {
                Ok(memory) => memory,
                Err(e) => {
                    drop(std::os::fd::OwnedFd::from_raw_fd(raw));
                    return Err(vk_err("vkAllocateMemory (DMA-BUF)")(e));
                }
            };
            if let Err(e) = d.bind_image_memory(vk_image, memory, 0) {
                d.free_memory(memory, None);
                return Err(vk_err("vkBindImageMemory")(e));
            }
            Ok(memory)
        })();
        let memory = match memory {
            Ok(memory) => memory,
            Err(e) => {
                unsafe { d.destroy_image(vk_image, None) };
                return Err(e);
            }
        };
        let view = unsafe {
            d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(vk_image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )
        };
        match view {
            Ok(view) => Ok((Source::Image(vk_image, view), memory)),
            Err(e) => {
                unsafe {
                    d.destroy_image(vk_image, None);
                    d.free_memory(memory, None);
                }
                Err(vk_err("vkCreateImageView")(e))
            }
        }
    }

    fn import_host(
        &self,
        ext: &ext::external_memory_host::Device,
        format: PixelFormat,
        ptr: *const u8,
        len: usize,
    ) -> Result<(Source, vk::DeviceMemory), VrError> {
        let d = &self.device;
        let mut external = vk::ExternalMemoryBufferCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT);
        let buffer = unsafe {
            d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(len as u64)
                    .usage(vk::BufferUsageFlags::UNIFORM_TEXEL_BUFFER)
                    .push_next(&mut external),
                None,
            )
        }
        .map_err(vk_err("vkCreateBuffer"))?;
        let memory = (|| unsafe {
            let mut host_props = vk::MemoryHostPointerPropertiesEXT::default();
            (ext.fp().get_memory_host_pointer_properties_ext)(
                d.handle(),
                vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT,
                ptr as *const c_void,
                &mut host_props,
            )
            .result()
            .map_err(vk_err("vkGetMemoryHostPointerPropertiesEXT"))?;
            let req = d.get_buffer_memory_requirements(buffer);
            let bits = req.memory_type_bits & host_props.memory_type_bits;
            let index = self.memory_type(bits, vk::MemoryPropertyFlags::empty())?;
            let mut import = vk::ImportMemoryHostPointerInfoEXT::default()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT)
                .host_pointer(ptr as *mut c_void);
            let memory = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(len as u64)
                        .memory_type_index(index)
                        .push_next(&mut import),
                    None,
                )
                .map_err(vk_err("vkAllocateMemory (host)"))?;
            if let Err(e) = d.bind_buffer_memory(buffer, memory, 0) {
                d.free_memory(memory, None);
                return Err(vk_err("vkBindBufferMemory")(e));
            }
            Ok(memory)
        })();
        let memory = match memory {
            Ok(memory) => memory,
            Err(e) => {
                unsafe { d.destroy_buffer(buffer, None) };
                return Err(e);
            }
        };
        // B と R はシェーダーで入れ替える（B8G8R8A8 のテクセルバッファは対応が必須でない）。
        let _ = format;
        let view = unsafe {
            d.create_buffer_view(
                &vk::BufferViewCreateInfo::default()
                    .buffer(buffer)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .range(vk::WHOLE_SIZE),
                None,
            )
        };
        match view {
            Ok(view) => Ok((Source::Buffer(buffer, view), memory)),
            Err(e) => {
                unsafe {
                    d.destroy_buffer(buffer, None);
                    d.free_memory(memory, None);
                }
                Err(vk_err("vkCreateBufferView")(e))
            }
        }
    }

    /// 書き写すコマンドを `cmd` に記録する。書き先のビューとフレームバッファを返す。
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        importer: &Importer,
        slot: usize,
        cmd: vk::CommandBuffer,
        target: vk::Image,
        image: &ExternalImage,
        source: &Source,
        layout: vk::ImageLayout,
    ) -> Result<(vk::ImageView, vk::Framebuffer), VrError> {
        let d = &self.device;
        let (width, height) = (image.width, image.height);
        let view = unsafe {
            d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(target)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(color_range()),
                None,
            )
        }
        .map_err(vk_err("vkCreateImageView"))?;
        let attachments = [view];
        let framebuffer = unsafe {
            d.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(importer.render_pass)
                    .attachments(&attachments)
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )
        };
        let framebuffer = match framebuffer {
            Ok(f) => f,
            Err(e) => {
                unsafe { d.destroy_image_view(view, None) };
                return Err(vk_err("vkCreateFramebuffer")(e));
            }
        };
        let recorded = (|| unsafe {
            let (kind, params) = match (source, &image.memory) {
                (Source::Image(_, src), _) => {
                    let info = [vk::DescriptorImageInfo::default()
                        .sampler(importer.sampler)
                        .image_view(*src)
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                    let write = [vk::WriteDescriptorSet::default()
                        .dst_set(importer.sets[slot][KIND_IMAGE])
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&info)];
                    d.update_descriptor_sets(&write, &[]);
                    let params = Params {
                        opaque: image.format.opaque().into(),
                        bgr: 0,
                        stride: 0,
                        offset: 0,
                    };
                    (KIND_IMAGE, params)
                }
                (Source::Buffer(_, src), Memory::Host { offset, stride, .. }) => {
                    let views = [*src];
                    let write = [vk::WriteDescriptorSet::default()
                        .dst_set(importer.sets[slot][KIND_BUFFER])
                        .descriptor_type(vk::DescriptorType::UNIFORM_TEXEL_BUFFER)
                        .texel_buffer_view(&views)];
                    d.update_descriptor_sets(&write, &[]);
                    let params = Params {
                        opaque: image.format.opaque().into(),
                        bgr: image.format.bgr().into(),
                        stride: stride / 4,
                        offset: (*offset / 4) as u32,
                    };
                    (KIND_BUFFER, params)
                }
                // 共有メモリ以外の置き場所（DMA-BUF）は Linux にしかない。
                #[cfg(target_os = "linux")]
                (Source::Buffer(..), _) => unreachable!("buffers come from host memory"),
            };
            d.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(vk_err("vkBeginCommandBuffer"))?;
            // DMA-BUF は表示サーバー（外部のキュー）から受け取る。
            let foreign = |src_image: vk::Image, acquire: bool| {
                let (from, to) = if acquire {
                    (vk::QUEUE_FAMILY_FOREIGN_EXT, self.queue_family)
                } else {
                    (self.queue_family, vk::QUEUE_FAMILY_FOREIGN_EXT)
                };
                let (old, new) = if acquire {
                    (
                        vk::ImageLayout::GENERAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    )
                } else {
                    (
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::ImageLayout::GENERAL,
                    )
                };
                vk::ImageMemoryBarrier::default()
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(if acquire {
                        vk::AccessFlags::empty()
                    } else {
                        vk::AccessFlags::SHADER_READ
                    })
                    .dst_access_mask(if acquire {
                        vk::AccessFlags::SHADER_READ
                    } else {
                        vk::AccessFlags::empty()
                    })
                    .src_queue_family_index(from)
                    .dst_queue_family_index(to)
                    .image(src_image)
                    .subresource_range(color_range())
            };
            if let Source::Image(src_image, _) = source {
                d.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[foreign(*src_image, true)],
                );
            }
            d.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(importer.render_pass)
                    .framebuffer(framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: vk::Extent2D { width, height },
                    }),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                importer.pipelines[kind],
            );
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                importer.layouts[kind],
                0,
                &[importer.sets[slot][kind]],
                &[],
            );
            let bytes = std::slice::from_raw_parts(
                &params as *const Params as *const u8,
                size_of::<Params>(),
            );
            d.cmd_push_constants(
                cmd,
                importer.layouts[kind],
                vk::ShaderStageFlags::FRAGMENT,
                0,
                bytes,
            );
            d.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: width as f32,
                    height: height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            d.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: vk::Extent2D { width, height },
                }],
            );
            d.cmd_draw(cmd, 3, 1, 0, 0);
            d.cmd_end_render_pass(cmd);
            let mut barriers = vec![
                vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .new_layout(layout)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(target)
                    .subresource_range(color_range()),
            ];
            if let Source::Image(src_image, _) = source {
                barriers.push(foreign(*src_image, false));
            }
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
            d.end_command_buffer(cmd)
                .map_err(vk_err("vkEndCommandBuffer"))
        })();
        match recorded {
            Ok(()) => Ok((view, framebuffer)),
            Err(e) => {
                unsafe {
                    d.destroy_framebuffer(framebuffer, None);
                    d.destroy_image_view(view, None);
                }
                Err(e)
            }
        }
    }

    fn destroy_source(&self, source: Source, memory: vk::DeviceMemory) {
        let d = &self.device;
        unsafe {
            match source {
                Source::Image(image, view) => {
                    d.destroy_image_view(view, None);
                    d.destroy_image(image, None);
                }
                Source::Buffer(buffer, view) => {
                    d.destroy_buffer_view(view, None);
                    d.destroy_buffer(buffer, None);
                }
            }
            d.free_memory(memory, None);
        }
    }

    pub(super) fn destroy_transient(&self, t: Transient) {
        unsafe {
            self.device.destroy_framebuffer(t.framebuffer, None);
            self.device.destroy_image_view(t.target, None);
        }
        if let Some((source, memory)) = t.source {
            self.destroy_source(source, memory);
        }
    }
}

/// DMA-BUF の画像を引く値（共有メモリは使い回さない。写像が外れた後に取り込みを残せないため）。
#[cfg(not(target_os = "linux"))]
fn key(_: &ExternalImage) -> Option<Key> {
    None
}

/// DMA-BUF の画像を引く値（共有メモリは使い回さない。写像が外れた後に取り込みを残せないため）。
#[cfg(target_os = "linux")]
fn key(image: &ExternalImage) -> Option<Key> {
    let Memory::DmaBuf {
        fd,
        modifier,
        offset,
        stride,
    } = &image.memory
    else {
        return None;
    };
    // fd は借りるだけで閉じない。
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd.as_raw_fd()) });
    let meta = file.metadata().ok()?;
    Some(Key {
        dev: meta.dev(),
        ino: meta.ino(),
        modifier: *modifier,
        offset: *offset,
        stride: *stride,
        width: image.width,
        height: image.height,
        format: image.format,
    })
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

#[cfg(test)]
mod tests {
    //! ランタイムを使わず、この GPU だけで取り込みと書き写しを検証する（`cargo test -p vr -- --ignored import`）。
    use super::*;
    use crate::gpu::{self, Image};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn gpu() -> Gpu {
        let entry = gpu::entry().expect("Vulkan loader");
        let instance = gpu::create_instance(&entry, &[], |info| {
            unsafe { entry.create_instance(info, None) }
                .map(|i| i.handle())
                .map_err(vk_err("vkCreateInstance"))
        })
        .unwrap();
        let physical = unsafe { instance.enumerate_physical_devices() }.unwrap()[0];
        gpu::create_device(entry, instance, physical, &[], |instance, info| {
            unsafe { instance.create_device(physical, info, None) }
                .map(|d| d.handle())
                .map_err(vk_err("vkCreateDevice"))
        })
        .unwrap()
    }

    /// 4×2 の画素（B, G, R, 未使用）と、書き写した結果の RGBA。
    fn pattern(stride: usize) -> (Vec<u8>, Vec<u8>) {
        let mut bgrx = vec![0u8; stride * 2];
        let mut rgba = Vec::new();
        for y in 0..2 {
            for x in 0..4 {
                let (r, g, b) = (x as u8 * 60, y as u8 * 200, 7 + x as u8);
                bgrx[y * stride + x * 4..][..4].copy_from_slice(&[b, g, r, 0]);
                rgba.extend([r, g, b, 255]);
            }
        }
        (bgrx, rgba)
    }

    fn target(gpu: &Gpu) -> Image {
        gpu.create_image(
            4,
            2,
            vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED,
        )
        .unwrap()
    }

    #[test]
    #[ignore = "needs a Vulkan GPU"]
    fn shared_memory_is_drawn_as_rgba_and_returned_after_the_gpu_reads_it() {
        let mut gpu = gpu();
        let Some(align) = gpu.import_formats().and_then(|f| f.host_alignment) else {
            eprintln!("this GPU cannot import host memory");
            return;
        };
        let layout = std::alloc::Layout::from_size_align(align, align).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        // 先頭から 1 行ぶん空けて置き、行の途中で終わる幅（行あたり 5 画素）にする。
        let (bgrx, rgba) = pattern(20);
        unsafe { std::ptr::copy_nonoverlapping(bgrx.as_ptr(), ptr.add(20), bgrx.len()) };
        let released = Arc::new(AtomicBool::new(false));
        let flag = released.clone();
        let image = Arc::new(ExternalImage::new(
            4,
            2,
            PixelFormat::Bgrx,
            Memory::Host {
                ptr,
                len: align,
                offset: 20,
                stride: 20,
            },
            move || flag.store(true, Ordering::Release),
        ));
        let target = target(&gpu);
        gpu.convert(target.image, &image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        drop(image);
        let read = gpu
            .download(target.image, 4, 2, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        assert_eq!(read, rgba);
        gpu.destroy_image(target);
        drop(gpu);
        assert!(
            released.load(Ordering::Acquire),
            "returned once the GPU is done"
        );
        unsafe { std::alloc::dealloc(ptr, layout) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "needs a Vulkan GPU"]
    fn dmabuf_is_imported_and_drawn_as_rgba() {
        let mut gpu = gpu();
        let Some(formats) = gpu.import_formats() else {
            eprintln!("this GPU cannot import DMA-BUF");
            return;
        };
        let modifiers = formats.modifiers(PixelFormat::Bgrx).to_vec();
        if modifiers.is_empty() {
            eprintln!("this GPU cannot import DMA-BUF");
            return;
        }
        // 表示サーバーの代わりに、この GPU で DMA-BUF を書き出す。
        let d = gpu.device.clone();
        let fd_ext = khr::external_memory_fd::Device::new(&gpu.instance, &d);
        let mut list =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let source = unsafe {
            d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .extent(vk::Extent3D {
                        width: 4,
                        height: 2,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                    .push_next(&mut external)
                    .push_next(&mut list),
                None,
            )
        }
        .unwrap();
        let req = unsafe { d.get_image_memory_requirements(source) };
        let mut export = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(source);
        let memory = unsafe {
            d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(
                        gpu.memory_type(
                            req.memory_type_bits,
                            vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        )
                        .unwrap(),
                    )
                    .push_next(&mut export)
                    .push_next(&mut dedicated),
                None,
            )
        }
        .unwrap();
        unsafe { d.bind_image_memory(source, memory, 0) }.unwrap();
        let modifier_ext = ext::image_drm_format_modifier::Device::new(&gpu.instance, &d);
        let mut props = vk::ImageDrmFormatModifierPropertiesEXT::default();
        unsafe { modifier_ext.get_image_drm_format_modifier_properties(source, &mut props) }
            .unwrap();
        let plane = unsafe {
            d.get_image_subresource_layout(
                source,
                vk::ImageSubresource::default()
                    .aspect_mask(vk::ImageAspectFlags::MEMORY_PLANE_0_EXT),
            )
        };
        let (bgrx, rgba) = pattern(16);
        gpu.upload(source, 4, 2, &bgrx, vk::ImageLayout::GENERAL)
            .unwrap();
        gpu.wait_idle();
        let fd = unsafe {
            fd_ext.get_memory_fd(
                &vk::MemoryGetFdInfoKHR::default()
                    .memory(memory)
                    .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
            )
        }
        .unwrap();
        let image = Arc::new(ExternalImage::new(
            4,
            2,
            PixelFormat::Bgrx,
            Memory::DmaBuf {
                fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
                modifier: props.drm_format_modifier,
                offset: plane.offset as u32,
                stride: plane.row_pitch as u32,
            },
            || {},
        ));
        let target = target(&gpu);
        gpu.convert(target.image, &image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        let read = gpu
            .download(target.image, 4, 2, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        assert_eq!(read, rgba, "modifier {:#x}", props.drm_format_modifier);
        // 表示サーバーが同じバッファに次の画面を書いても、取り込みは使い回して新しい内容を読む。
        let (mut bgrx, _) = pattern(16);
        bgrx.iter_mut().for_each(|v| *v = 255 - *v);
        gpu.upload(source, 4, 2, &bgrx, vk::ImageLayout::GENERAL)
            .unwrap();
        gpu.convert(target.image, &image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        let read = gpu
            .download(target.image, 4, 2, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .unwrap();
        assert_eq!(&read[..4], [255, 255, 255 - 7, 255]);
        assert_eq!(gpu.cache.len(), 1, "imported once");
        gpu.wait_idle();
        gpu.destroy_image(target);
        unsafe {
            d.destroy_image(source, None);
            d.free_memory(memory, None);
        }
    }
}
