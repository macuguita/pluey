#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
use std::sync::Arc;

use anyhow::Result;
use wgpu::PipelineCompilationOptions;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, event_loop::OwnedDisplayHandle, window::Window};

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, 1.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [-1.0, -1.0],
        uv: [0.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-1.0, 1.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, -1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [1.0, 1.0],
        uv: [1.0, 0.0],
    },
];

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scale: [f32; 2],
    offset: [f32; 2],
}

/// Scale that renders the image at its native pixel size:
/// 1 image pixel = 1 screen pixel, regardless of window size.
fn native_scale(
    window_width: u32,
    window_height: u32,
    img_width: u32,
    img_height: u32,
) -> [f32; 2] {
    [
        img_width as f32 / window_width as f32,
        img_height as f32 / window_height as f32,
    ]
}

pub struct Renderer {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    max_texture_dim: u32,
    base_scale: [f32; 2],
    zoom: f32,
    offset: [f32; 2],
    dragging: bool,
    last_cursor: Option<(f64, f64)>,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    image: Option<LoadedImage>,
}

struct LoadedImage {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl Renderer {
    #[allow(clippy::too_many_lines)]
    pub async fn new(
        _display: OwnedDisplayHandle,
        window: Arc<Window>,
        image_path: Option<&str>,
    ) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;

        // use the adapter's real limits instead of the conservative
        // downlevel defaults (which cap textures at 2048px)
        let adapter_limits = adapter.limits();
        let max_texture_dim = adapter_limits.max_texture_dimension_2d;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await?;

        let clamped_width = size.width.max(1).min(max_texture_dim);
        let clamped_height = size.height.max(1).min(max_texture_dim);

        let config = surface
            .get_default_config(&adapter, clamped_width, clamped_height)
            .unwrap();

        surface.configure(&device, &config);

        // --- Load image and upload to a texture ---
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let image = match image_path {
            Some(path) => Some(Self::load_image_texture(
                &device,
                &queue,
                &texture_bind_group_layout,
                path,
            )?),
            None => None,
        };

        // --- Uniform buffer holding the aspect-fit scale ---
        let base_scale = native_scale(
            clamped_width,
            clamped_height,
            image.as_ref().map_or(0, |img| img.width),
            image.as_ref().map_or(0, |img| img.height),
        );
        let zoom = 1.0;
        let offset = [0.0, 0.0];
        let uniforms = Uniforms {
            scale: [base_scale[0] * zoom, base_scale[1] * zoom],
            offset,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // --- Vertex buffer ---
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // --- Shader + pipeline ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Image Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[
                Some(&texture_bind_group_layout),
                Some(&uniform_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::desc())],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            instance,
            window,
            surface,
            device,
            queue,
            config,
            size: PhysicalSize::new(clamped_width, clamped_height),
            render_pipeline,
            vertex_buffer,
            uniform_buffer,
            uniform_bind_group,
            max_texture_dim,
            base_scale,
            zoom,
            offset,
            dragging: false,
            last_cursor: None,
            texture_bind_group_layout,
            image,
        })
    }

    pub fn apply_loaded_image(&mut self, data: &crate::LoadedImageData) {
        self.image = Some(Self::upload_rgba_to_texture(
            &self.device,
            &self.queue,
            &self.texture_bind_group_layout,
            &data.rgba,
            data.width,
            data.height,
        ));

        self.base_scale = native_scale(self.size.width, self.size.height, data.width, data.height);
        self.zoom = 1.0;
        self.offset = [0.0, 0.0];
        self.update_uniforms();
    }

    fn upload_rgba_to_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> LoadedImage {
        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Image Texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Diffuse Bind Group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        LoadedImage {
            bind_group,
            width,
            height,
        }
    }

    fn load_image_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        image_path: &str,
    ) -> Result<LoadedImage> {
        let img = image::open(image_path)?.to_rgba8();
        let (width, height) = img.dimensions();

        Ok(Self::upload_rgba_to_texture(
            device,
            queue,
            bind_group_layout,
            &img,
            width,
            height,
        ))
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        let clamped_width = new_size.width.min(self.max_texture_dim);
        let clamped_height = new_size.height.min(self.max_texture_dim);

        self.size = PhysicalSize::new(clamped_width, clamped_height);
        self.config.width = clamped_width;
        self.config.height = clamped_height;
        self.surface.configure(&self.device, &self.config);

        if let Some(image) = &self.image {
            self.base_scale =
                native_scale(clamped_width, clamped_height, image.width, image.height);
        }
        self.update_uniforms();
    }

    fn update_uniforms(&self) {
        let uniforms = Uniforms {
            scale: [
                self.base_scale[0] * self.zoom,
                self.base_scale[1] * self.zoom,
            ],
            offset: self.offset,
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    fn cursor_to_ndc(&self, pos: (f64, f64)) -> [f32; 2] {
        let x = (pos.0 / f64::from(self.size.width)) * 2.0 - 1.0;
        let y = 1.0 - (pos.1 / f64::from(self.size.height)) * 2.0;
        [x as f32, y as f32]
    }

    pub fn zoom(&mut self, scroll_delta: f32, cursor_pos: (f64, f64)) {
        let old_zoom = self.zoom;
        let factor = 1.0 + scroll_delta * 0.1;
        let new_zoom = (old_zoom * factor).clamp(0.05, 40.0);

        let cursor_ndc = self.cursor_to_ndc(cursor_pos);
        let old_scale = [self.base_scale[0] * old_zoom, self.base_scale[1] * old_zoom];
        let new_scale = [self.base_scale[0] * new_zoom, self.base_scale[1] * new_zoom];

        // keep image-space point under cursor stationary
        for i in 0..2 {
            let image_local = (cursor_ndc[i] - self.offset[i]) / old_scale[i];
            self.offset[i] = cursor_ndc[i] - image_local * new_scale[i];
        }

        self.zoom = new_zoom;
        self.update_uniforms();
    }

    pub fn start_drag(&mut self, pos: (f64, f64)) {
        self.dragging = true;
        self.last_cursor = Some(pos);
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.last_cursor = None;
    }

    pub fn cursor_moved(&mut self, pos: (f64, f64)) -> bool {
        if !self.dragging {
            self.last_cursor = Some(pos);
            return false;
        }
        let last = self.last_cursor.unwrap_or(pos);
        let dx = (pos.0 - last.0) / f64::from(self.size.width) * 2.0;
        let dy = -(pos.1 - last.1) / f64::from(self.size.height) * 2.0;
        self.offset[0] += dx as f32;
        self.offset[1] += dy as f32;
        self.last_cursor = Some(pos);
        self.update_uniforms();
        true
    }

    pub fn render(&mut self) -> Result<()> {
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                drop(texture);
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                unreachable!("No error scope registered, so validation errors will panic")
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(self.window.clone())?;
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0356,
                            g: 0.0356,
                            b: 0.0494,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(image) = &self.image {
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_bind_group(0, &image.bind_group, &[]);
                render_pass.set_bind_group(1, &self.uniform_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(surface_texture);

        Ok(())
    }

    pub fn window(&self) -> &Window {
        &self.window
    }
}
