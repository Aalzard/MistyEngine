//! GPU side of the player: window surface, the engine renderer's offscreen target, and a
//! fullscreen blit of that target onto the surface. The target is rendered at the quality
//! level's Render Scale and stretched to the window; a splash image can be shown instead.

use std::sync::Arc;

use misty_engine::render::ui::UiRenderer;
use misty_engine::render::{Frame, RenderTarget, Renderer};
use misty_engine::scene::Scene;
use misty_engine::ui::UiInteraction;
use misty_engine::wgpu;
use misty_engine::wgpu::util::DeviceExt;
use winit::window::Window;

/// Copies the target's final image (gamma-encoded bytes) unchanged to a non-sRGB colour target.
const BLIT_SHADER: &str = r#"
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@fragment fn fs(i: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(image, image_sampler, i.uv).rgb, 1.0);
}
"#;

/// Format of the screenshot copy (same bytes as the surface gets).
const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Non-sRGB view format of the surface (the blit writes encoded bytes).
    view_format: wgpu::TextureFormat,
    renderer: Renderer,
    /// Canvas UI drawn over the camera image.
    ui: UiRenderer,
    target: RenderTarget,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    group: wgpu::BindGroup,
    /// One pipeline per colour format (surface, screenshot).
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// Realtime reflection probes re-captured while the game runs.
    probes: misty_engine::render::probes::realtime::RealtimeProbes,
    pub adapter_name: String,
    /// Fraction of the window size the game is rendered at.
    render_scale: f32,
    /// The blit samples the nearest texel (Quality ▸ Upscale Filter: Nearest).
    nearest: bool,
    /// Splash image (bind group, pixel size) and its background colour.
    splash: Option<(wgpu::BindGroup, [u32; 2])>,
    splash_background: wgpu::Color,
}

impl Gfx {
    pub fn new(window: Arc<Window>, vsync: bool) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let size = window.inner_size();
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("window surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no graphics adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("misty player"),
            required_features: misty_engine::render::timestamp_features(&adapter),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("graphics device: {e}"))?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the window surface isn't supported by the graphics adapter")?;
        let view_format = config.format.remove_srgb_suffix();
        config.view_formats = vec![view_format];
        config.present_mode = present_mode(vsync);
        surface.configure(&device, &config);

        let renderer = Renderer::with_queue(&device, &queue);
        let ui = UiRenderer::new(&device, &queue);
        let target = renderer.create_view_target(&device, config.width, config.height);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty blit layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let sampler = blit_sampler(&device, false);
        let group = blit_group(&device, &layout, &sampler, &target);
        let mut gfx = Self {
            adapter_name: format!(
                "{} ({:?})",
                adapter.get_info().name,
                adapter.get_info().backend
            ),
            surface,
            device,
            queue,
            config,
            view_format,
            renderer,
            ui,
            target,
            layout,
            sampler,
            group,
            pipelines: Vec::new(),
            probes: Default::default(),
            render_scale: 1.0,
            nearest: false,
            splash: None,
            splash_background: wgpu::Color::BLACK,
        };
        gfx.pipeline(view_format);
        gfx.pipeline(CAPTURE_FORMAT);
        Ok(gfx)
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        if let Some(i) = self.pipelines.iter().position(|(f, _)| *f == format) {
            return &self.pipelines[i].1;
        }
        let device = &self.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty blit"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("misty blit"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((format, pipeline));
        &self.pipelines.last().expect("just pushed").1
    }

    /// New window size: reconfigure the surface and recreate the offscreen target.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimised
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.recreate_target();
    }

    fn recreate_target(&mut self) {
        let [w, h] = self.target_size();
        // Quality ▸ Upscaling: the scene renders at a share of this and is rebuilt temporally.
        self.target = self.renderer.create_view_target(&self.device, w, h);
        self.group = blit_group(&self.device, &self.layout, &self.sampler, &self.target);
    }

    /// Pixel size the game renders at (window size × Render Scale).
    pub fn target_size(&self) -> [u32; 2] {
        let s = self.render_scale;
        [self.config.width, self.config.height].map(|v| ((v as f32 * s).round() as u32).max(1))
    }

    pub fn render_scale(&self) -> f32 {
        self.render_scale
    }

    /// Use a quality level: shadow map size, effects, render scale.
    pub fn set_quality(&mut self, quality: &misty_engine::quality::QualityLevel) {
        self.renderer.set_quality(&self.device, quality);
        // Upscale Filter: Nearest keeps hard pixels when the image is stretched.
        let nearest = quality.upscale_filter.nearest();
        if nearest != self.nearest {
            self.nearest = nearest;
            self.sampler = blit_sampler(&self.device, nearest);
            self.group = blit_group(&self.device, &self.layout, &self.sampler, &self.target);
        }
        if (quality.render_scale - self.render_scale).abs() > 1e-4 {
            self.render_scale = quality.render_scale;
            self.recreate_target();
        } else {
            let [w, h] = self.target_size();
            if !self.renderer.view_target_fits(&self.target, w, h) {
                self.recreate_target();
            }
        }
    }

    /// Show `image` (sRGB bytes; transparency blended over `background`) until cleared.
    pub fn set_splash(
        &mut self,
        image: Option<misty_engine::image::RgbaImage>,
        background: [f32; 3],
    ) {
        let bg = background.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
        self.splash_background = wgpu::Color {
            r: f64::from(bg[0]) / 255.0,
            g: f64::from(bg[1]) / 255.0,
            b: f64::from(bg[2]) / 255.0,
            a: 1.0,
        };
        self.splash = image.map(|mut img| {
            for p in img.pixels_mut() {
                let a = f32::from(p[3]) / 255.0;
                for c in 0..3 {
                    p[c] = (f32::from(p[c]) * a + bg[c] * (1.0 - a)).round() as u8;
                }
                p[3] = 255;
            }
            let size = [img.width(), img.height()];
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("misty splash"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                img.as_raw(),
            );
            let view = texture.create_view(&Default::default());
            (
                view_group(&self.device, &self.layout, &self.sampler, &view),
                size,
            )
        });
    }

    /// Draw the splash screen: the image fitted inside the window over its background.
    pub fn draw_splash(&mut self) {
        let Some(texture) = self.acquire() else {
            return;
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let format = self.view_format;
        self.pipeline(format);
        let pipeline = &self
            .pipelines
            .iter()
            .find(|(f, _)| *f == format)
            .expect("created")
            .1;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty splash"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.splash_background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((group, [iw, ih])) = &self.splash {
                let (w, h) = (self.config.width as f32, self.config.height as f32);
                // Fit inside 80% of the window, never upscaled past 1:1 by more than 2x.
                let s = (0.8 * w / *iw as f32).min(0.8 * h / *ih as f32).min(2.0);
                let (vw, vh) = (*iw as f32 * s, *ih as f32 * s);
                pass.set_viewport((w - vw) / 2.0, (h - vh) / 2.0, vw, vh, 0.0, 1.0);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
    }

    /// The next surface image (`None` when the surface needs another try next frame).
    fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None, // occluded / timeout: try next frame
        }
    }

    /// GPU resources the renderer holds (profiler capture).
    pub fn memory(&self) -> misty_engine::render::RenderMemory {
        self.renderer.memory()
    }

    /// Texture problems found while drawing the UI.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.ui.take_warnings()
    }

    /// One frame of Realtime reflection probe captures (before the view gathers its probes).
    pub fn update_probes(
        &mut self,
        scene: &Scene,
        materials: &dyn misty_engine::render_scene::MaterialSource,
    ) {
        let (device, queue) = (&self.device, &self.queue);
        self.probes
            .update(&mut self.renderer, device, queue, scene, materials);
    }

    /// Render `frame` (or black when no camera renders) with `scene`'s UI on top, and present it.
    pub fn draw(
        &mut self,
        frame: Option<&Frame>,
        effects: &[misty_engine::vfx::EffectDraw],
        scene: &Scene,
        ui: Option<&UiInteraction>,
    ) {
        if let Some(frame) = frame {
            let (device, queue) = (&self.device, &self.queue);
            self.renderer
                .render_with_effects(device, queue, &self.target, frame, effects);
            misty_engine::profiler::set_render_stats(self.renderer.stats());
            self.ui
                .render(&self.device, &self.queue, &self.target, scene, ui);
        }
        let Some(texture) = self.acquire() else {
            return;
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        self.blit(&view, self.view_format, frame.is_some());
        self.queue.present(texture);
    }

    fn blit(&mut self, view: &wgpu::TextureView, format: wgpu::TextureFormat, image: bool) {
        self.pipeline(format);
        let pipeline = &self
            .pipelines
            .iter()
            .find(|(f, _)| *f == format)
            .expect("created")
            .1;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("misty blit"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if image {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Read the last rendered image back as tightly packed RGBA rows.
    pub fn capture(&mut self) -> (u32, u32, Vec<u8>) {
        let (width, height) = self.target.output_size();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        self.blit(&view, CAPTURE_FORMAT, true);

        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty capture"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let Ok(data) = slice.get_mapped_range() else {
            return (width, height, Vec::new());
        };
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            pixels.extend_from_slice(&data[start..start + width as usize * 4]);
        }
        (width, height, pixels)
    }
}

fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    }
}

/// Sampler that stretches the game image to the window.
fn blit_sampler(device: &wgpu::Device, nearest: bool) -> wgpu::Sampler {
    let filter = if nearest {
        wgpu::FilterMode::Nearest
    } else {
        wgpu::FilterMode::Linear
    };
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("misty blit sampler"),
        mag_filter: filter,
        min_filter: filter,
        ..Default::default()
    })
}

fn blit_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    target: &RenderTarget,
) -> wgpu::BindGroup {
    view_group(device, layout, sampler, target.view())
}

fn view_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty blit group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
