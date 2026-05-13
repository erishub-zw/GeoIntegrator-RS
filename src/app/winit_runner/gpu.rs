use crate::app::{App, GravityParams};
use bytemuck::{Pod, Zeroable};
use egui::{ClippedPrimitive, TexturesDelta};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use half::f16;
use std::sync::Arc;
use wgpu::{
    Adapter, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType, BufferDescriptor,
    BufferUsages, Color, ColorTargetState, CompositeAlphaMode, Device, DeviceDescriptor, Extent3d, Features,
    FilterMode, FragmentState, Instance, Limits, LoadOp, MultisampleState, Operations,
    PipelineCompilationOptions, PipelineLayoutDescriptor, PowerPreference, PresentMode, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, RequestAdapterOptions,
    SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Surface,
    SurfaceConfiguration, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureViewDescriptor, TextureViewDimension, VertexState,
};
use winit::window::Window;

use super::hdr_loader::HdrTextureData;

pub(super) struct GpuState {
    _instance: Instance,
    _adapter: Adapter,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    pub(super) max_texture_dimension_2d: u32,
    pipeline: RenderPipeline,
    egui_renderer: Renderer,
    sky_bind_group_layout: BindGroupLayout,
    sky_sampler: wgpu::Sampler,
    sky_texture_a: wgpu::Texture,
    sky_texture_b: wgpu::Texture,
    bind_group: BindGroup,
    camera_uniform: CameraUniform,
    camera_buffer: Buffer,
    gravity_uniform: GravityUniform,
    gravity_buffer: Buffer,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    params: [f32; 4],  // x=aspect, y=tan_half_fov, z=exposure, w=gamma
    params2: [f32; 4], // x=debug_direction_view
    position: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GravityUniform {
    params: [f32; 4],  // x=mass, y=spin, z=charge, w=horizon_radius
    params2: [f32; 4], // x=is_wormhole, y=integrator_enabled, z=debug_steps_view, w=adaptive_step
    params3: [f32; 4], // x=step_size, y=min_step_size, z=max_step_size, w=max_steps
    params4: [f32; 4], // x=escape_radius, y=adaptive_radius_scale
}

impl GpuState {
    pub(super) async fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| e.to_string())?;

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("egone-device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: PresentMode::Fifo,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let camera_uniform = CameraUniform {
            right: [1.0, 0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0, 0.0],
            forward: [0.0, 0.0, -1.0, 0.0],
            params: [16.0 / 9.0, (60.0_f32.to_radians() * 0.5).tan(), 0.0, 0.0],
            params2: [0.0, 0.0, 0.0, 0.0],
            position: [0.0, 0.0, 5.0, 0.0],
        };
        let camera_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("camera-uniform-buffer"),
            size: core::mem::size_of::<CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));
        let gravity_uniform = GravityUniform {
            params: [5.0, 0.0, 0.0, GravityParams::HORIZON_RADIUS_MIN.max(1.0)],
            params2: [0.0, 0.0, 0.0, 0.0],
            params3: [0.02, 0.002, 0.05, 128.0],
            params4: [40.0, 6.0, 0.0, 0.0],
        };
        let gravity_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("gravity-uniform-buffer"),
            size: core::mem::size_of::<GravityUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&gravity_buffer, 0, bytemuck::bytes_of(&gravity_uniform));

        let sky_bind_group_layout = create_sky_bind_group_layout(&device);
        let sky_sampler = create_sky_sampler(&device);
        let sky_texture_a = create_sky_texture_from_pixels(
            &device,
            &queue,
            &[
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(1.0).to_bits(),
            ],
            1,
            1,
        );
        let sky_texture_b = create_sky_texture_from_pixels(
            &device,
            &queue,
            &[
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(0.0).to_bits(),
                f16::from_f32(1.0).to_bits(),
            ],
            1,
            1,
        );
        let bind_group = create_sky_bind_group(
            &device,
            &sky_bind_group_layout,
            &sky_sampler,
            &camera_buffer,
            &gravity_buffer,
            &sky_texture_a,
            &sky_texture_b,
        );
        let pipeline = create_fullscreen_pipeline(&device, config.format, &sky_bind_group_layout);
        let egui_renderer = Renderer::new(&device, config.format, RendererOptions::default());
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;

        Ok(Self {
            _instance: instance,
            _adapter: adapter,
            surface,
            device,
            queue,
            config,
            max_texture_dimension_2d,
            pipeline,
            egui_renderer,
            sky_bind_group_layout,
            sky_sampler,
            sky_texture_a,
            sky_texture_b,
            bind_group,
            camera_uniform,
            camera_buffer,
            gravity_uniform,
            gravity_buffer,
        })
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn update_camera_uniform(&mut self, app: &App) {
        let right = app.camera.right();
        let up = app.camera.up();
        let forward = app.camera.forward();
        let position = app.camera.position();
        self.camera_uniform = CameraUniform {
            right: [right.x, right.y, right.z, 0.0],
            up: [up.x, up.y, up.z, 0.0],
            forward: [forward.x, forward.y, forward.z, 0.0],
            params: [
                app.camera.aspect,
                (app.camera.fov_y_radians * 0.5).tan(),
                app.tone_mapping.exposure,
                app.tone_mapping.gamma,
            ],
            params2: [if app.tone_mapping.debug_direction_view { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            position: [position.x, position.y, position.z, 0.0],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&self.camera_uniform));
    }

    fn update_gravity_uniform(&mut self, app: &App) {
        let mut gravity = app.gravity;
        gravity.sanitize();
        self.gravity_uniform = GravityUniform {
            params: [gravity.mass, gravity.spin, gravity.charge, gravity.horizon_radius],
            params2: [
                if gravity.is_wormhole { 1.0 } else { 0.0 },
                if app.integrator.enabled { 1.0 } else { 0.0 },
                if app.integrator.debug_steps_view { 1.0 } else { 0.0 },
                if app.integrator.adaptive_step { 1.0 } else { 0.0 },
            ],
            params3: [
                app.integrator.step_size,
                app.integrator.min_step_size,
                app.integrator.max_step_size,
                app.integrator.max_steps as f32,
            ],
            params4: [
                app.integrator.escape_radius,
                app.integrator.adaptive_radius_scale,
                0.0,
                0.0,
            ],
        };
        self.queue
            .write_buffer(&self.gravity_buffer, 0, bytemuck::bytes_of(&self.gravity_uniform));
    }

    pub(super) fn update_sky_texture_a(&mut self, data: HdrTextureData) {
        self.sky_texture_a = create_sky_texture_from_pixels(
            &self.device,
            &self.queue,
            &data.pixels,
            data.width,
            data.height,
        );
        self.bind_group = create_sky_bind_group(
            &self.device,
            &self.sky_bind_group_layout,
            &self.sky_sampler,
            &self.camera_buffer,
            &self.gravity_buffer,
            &self.sky_texture_a,
            &self.sky_texture_b,
        );
    }

    pub(super) fn update_sky_texture_b(&mut self, data: HdrTextureData) {
        self.sky_texture_b = create_sky_texture_from_pixels(
            &self.device,
            &self.queue,
            &data.pixels,
            data.width,
            data.height,
        );
        self.bind_group = create_sky_bind_group(
            &self.device,
            &self.sky_bind_group_layout,
            &self.sky_sampler,
            &self.camera_buffer,
            &self.gravity_buffer,
            &self.sky_texture_a,
            &self.sky_texture_b,
        );
    }

    pub(super) fn render(
        &mut self,
        app: &App,
        paint_jobs: &[ClippedPrimitive],
        textures_delta: &TexturesDelta,
        pixels_per_point: f32,
    ) {
        self.update_camera_uniform(app);
        self.update_gravity_uniform(app);

        for (id, delta) in textures_delta.set.iter().filter(|(_, d)| d.pos.is_none()) {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        for (id, delta) in textures_delta.set.iter().filter(|(_, d)| d.pos.is_some()) {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resize(self.config.width, self.config.height);
                for id in &textures_delta.free {
                    self.egui_renderer.free_texture(id);
                }
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                for id in &textures_delta.free {
                    self.egui_renderer.free_texture(id);
                }
                return;
            }
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egone-clear-pass"),
            });
        let egui_user_cmd = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            paint_jobs,
            &screen_descriptor,
        );

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("fullscreen-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
            let mut egui_pass = pass.forget_lifetime();
            self.egui_renderer
                .render(&mut egui_pass, paint_jobs, &screen_descriptor);
        }

        self.queue
            .submit(egui_user_cmd.into_iter().chain(Some(encoder.finish())));
        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        frame.present();
    }
}

fn create_fullscreen_pipeline(
    device: &Device,
    color_format: wgpu::TextureFormat,
    bind_group_layout: &BindGroupLayout,
) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("fullscreen-shader"),
        source: ShaderSource::Wgsl(include_str!("../../shaders/fullscreen.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("fullscreen-pipeline-layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("fullscreen-triangle-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: PipelineCompilationOptions::default(),
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_sky_bind_group_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("sky-bind-group-layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

fn create_sky_sampler(device: &Device) -> wgpu::Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("sky-sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    })
}

fn create_sky_texture_from_pixels(
    device: &Device,
    queue: &Queue,
    pixels: &[u16],
    width: u32,
    height: u32,
) -> wgpu::Texture {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("sky-texture-rgba16f"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba16Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(pixels),
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8 * width),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn create_sky_bind_group(
    device: &Device,
    bind_group_layout: &BindGroupLayout,
    sampler: &wgpu::Sampler,
    camera_buffer: &Buffer,
    gravity_buffer: &Buffer,
    sky_texture_a: &wgpu::Texture,
    sky_texture_b: &wgpu::Texture,
) -> BindGroup {
    let texture_a_view = sky_texture_a.create_view(&TextureViewDescriptor::default());
    let texture_b_view = sky_texture_b.create_view(&TextureViewDescriptor::default());
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("sky-bind-group"),
        layout: bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&texture_a_view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 2,
                resource: camera_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: gravity_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(&texture_b_view),
            },
        ],
    })
}
