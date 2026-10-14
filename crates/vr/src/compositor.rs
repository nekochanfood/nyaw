//! novr のエミュレーターの GPU コンポジタ（wgpu）。パネルを透視投影したテクスチャ付きの四角としてウィンドウに直接描く。
//! CPU の合成（novr.rs の `rasterize`）と同じ見た目（無彩色の市松模様の背景、描画順・遠い順に奥から重ねる、ストレートアルファ）。
//! 透視の割り付けと奥行きの切り取りは GPU がする。テクスチャは送られたときだけ GPU へ上げる。

use glam::{Mat4, Vec3};
use minifb::Window;

use crate::{Pose, Vec2};

/// 市松模様の 1 マス（px）と 2 色（0〜1。CPU の合成と同じ値）。
const CHECKER: f32 = 16.0;
const CHECKER_GRAY: [f32; 2] = [0x6b as f32 / 255.0, 0x8a as f32 / 255.0];

/// 1 枚のクアッドを描く材料（毎フレーム novr.rs が渡す）。
pub(crate) struct Draw<'a> {
    /// どのクアッドか（GPU 側のテクスチャの持ち主）。
    pub quad: usize,
    pub pose: Pose,
    /// 大きさ（m）。
    pub size: Vec2,
    /// 不透明度（0〜1）。
    pub alpha: f32,
    /// テクスチャを上げ直すなら (幅, 高さ, RGBA)。
    pub upload: Option<(u32, u32, &'a [u8])>,
}

/// 1 枚のクアッドの GPU 側の持ち物。
struct Slot {
    texture: wgpu::Texture,
    size: (u32, u32),
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

pub(crate) struct Gpu {
    // サーフェスはウィンドウより先に落とす（Emulator のフィールドの順でウィンドウより前に置く）。
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    quad_pipeline: wgpu::RenderPipeline,
    background_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    slots: Vec<Option<Slot>>,
}

/// 1 枚分の uniform（4 隅のクリップ座標と不透明度）。
#[repr(C)]
#[derive(Clone, Copy)]
struct QuadUniform {
    corners: [[f32; 4]; 4],
    alpha: [f32; 4],
}

const SHADER: &str = r#"
struct Quad { corners: array<vec4<f32>, 4>, alpha: vec4<f32> };
@group(0) @binding(0) var<uniform> q: Quad;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

// 4 隅は左上・右上・左下・右下の順で、三角形の帯で描く。
@vertex fn quad_vs(@builtin(vertex_index) i: u32) -> V {
    var uv = array<vec2<f32>, 4>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0));
    var o: V;
    o.pos = q.corners[i];
    o.uv = uv[i];
    return o;
}

@fragment fn quad_fs(v: V) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, v.uv);
    return vec4(c.rgb, c.a * q.alpha.x);
}

struct Checker { size: f32, a: f32, b: f32, pad: f32 };

@vertex fn bg_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[i], 0.0, 1.0);
}

@fragment fn bg_fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let k = (u32(p.x / CHECKER) + u32(p.y / CHECKER)) % 2u;
    let g = select(GRAY_A, GRAY_B, k == 1u);
    return vec4(g, g, g, 1.0);
}
"#;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    pollster::block_on(f)
}

impl Gpu {
    /// `window` に描く GPU コンポジタを作る。使える GPU が無ければエラー（呼び出し側は CPU で合成する）。
    pub(crate) fn new(window: &Window, size: (u32, u32)) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // SAFETY: サーフェスは Emulator の中でウィンドウより先に落とす（フィールドの順）。ウィンドウは Emulator が持ち続ける。
        let surface = unsafe {
            use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(
                    window.display_handle().map_err(|e| e.to_string())?.as_raw(),
                ),
                raw_window_handle: window.window_handle().map_err(|e| e.to_string())?.as_raw(),
            };
            instance.create_surface_unsafe(target)
        }
        .map_err(|e| format!("サーフェスを作れない: {e}"))?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|e| format!("GPU が無い: {e}"))?;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nyaw-emulator"),
            ..Default::default()
        }))
        .map_err(|e| format!("GPU のデバイスを作れない: {e}"))?;
        let caps = surface.get_capabilities(&adapter);
        // CPU の合成と同じく sRGB の値のまま混ぜるので、sRGB でない形式を選ぶ。
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        // 周期は poll が作るので、表示で待たない。
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let source = SHADER
            .replace("CHECKER", &format!("{CHECKER:.1}"))
            .replace("GRAY_A", &format!("{:.6}", CHECKER_GRAY[0]))
            .replace("GRAY_B", &format!("{:.6}", CHECKER_GRAY[1]));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nyaw-emulator"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let empty_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background"),
            bind_group_layouts: &[],
            ..Default::default()
        });
        let pipeline = |layout: &wgpu::PipelineLayout, vs: &str, fs: &str, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let quad_pipeline = pipeline(
            &quad_layout,
            "quad_vs",
            "quad_fs",
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let background_pipeline = pipeline(&empty_layout, "bg_vs", "bg_fs", None);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quad"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            surface,
            device,
            queue,
            config,
            quad_pipeline,
            background_pipeline,
            layout,
            sampler,
            slots: Vec::new(),
        })
    }

    /// ウィンドウの大きさが変わった。
    pub(crate) fn resize(&mut self, size: (u32, u32)) {
        if size.0 == 0 || size.1 == 0 || (self.config.width, self.config.height) == size {
            return;
        }
        (self.config.width, self.config.height) = size;
        self.surface.configure(&self.device, &self.config);
    }

    /// テクスチャを上げる（大きさが変わったら作り直す）。
    fn upload(&mut self, quad: usize, (w, h, rgba): (u32, u32, &[u8])) {
        if self.slots.len() <= quad {
            self.slots.resize_with(quad + 1, || None);
        }
        if self.slots[quad].as_ref().is_none_or(|s| s.size != (w, h)) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("quad"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // CPU の合成と同じく sRGB の値のまま混ぜる。
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad"),
                size: std::mem::size_of::<QuadUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let view = texture.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("quad"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.slots[quad] = Some(Slot {
                texture,
                size: (w, h),
                uniform,
                bind,
            });
        }
        let slot = self.slots[quad].as_ref().unwrap();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &slot.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 背景の上に `draws` を奥から手前の順に描いて表示する。`fov_y` は縦の視野角（ラジアン）。
    pub(crate) fn draw(&mut self, draws: &[Draw], fov_y: f32, near: f32) -> Result<(), String> {
        for d in draws {
            if let Some(up) = d.upload {
                self.upload(d.quad, up);
            }
        }
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        let proj = Mat4::perspective_rh(fov_y, w / h, near, 1000.0);
        let mut drawn = Vec::new();
        for d in draws {
            let Some(slot) = self.slots.get(d.quad).and_then(|s| s.as_ref()) else {
                continue;
            };
            let (ux, uy) = (d.pose.orientation * Vec3::X, d.pose.orientation * Vec3::Y);
            let c = d.pose.position;
            let corner = |sx: f32, sy: f32| {
                let p = c + ux * d.size.x / 2.0 * sx + uy * d.size.y / 2.0 * sy;
                (proj * p.extend(1.0)).to_array()
            };
            let uniform = QuadUniform {
                corners: [
                    corner(-1.0, 1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                ],
                alpha: [d.alpha, 0.0, 0.0, 0.0],
            };
            // SAFETY: QuadUniform は repr(C) の f32 だけ。
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    &uniform as *const QuadUniform as *const u8,
                    std::mem::size_of::<QuadUniform>(),
                )
            };
            self.queue.write_buffer(&slot.uniform, 0, bytes);
            drawn.push(d.quad);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                // 失われた・古くなったサーフェスは作り直して、このフレームは飛ばす。
                self.surface.configure(&self.device, &self.config);
                return Err(format!("表示先を取れない: {other:?}"));
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("emulator"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("emulator"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.background_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.quad_pipeline);
            for quad in drawn {
                let slot = self.slots[quad].as_ref().unwrap();
                pass.set_bind_group(0, &slot.bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}
