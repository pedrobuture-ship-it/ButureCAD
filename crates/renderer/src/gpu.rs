//! wgpu backend: renders a [`Scene`] offscreen and reads the pixels back.
//!
//! The webview shows the result in a `<canvas>`; there is no per-object DOM. Frames are
//! produced on demand (no render loop here), so an idle editor costs nothing.
//!
//! Pipeline: one triangle pipeline (alpha blended, 4× MSAA). Three draws per frame share it:
//! the cached scene mesh, a per-frame world-space mesh for selection/hover/tool previews, and a
//! pixel-space overlay for the grid, box selection and the optional debug text.

use crate::grid::{grid_lines, GridLineKind, GridSettings, GridStyle};
use crate::picking::Selection;
use crate::scene::{Rgba, Scene, Shape};
use crate::tessellate::{tessellate_scene, Mesh, MeshBuilder, Vertex};
use crate::view::View;
use std::time::Instant;
use thiserror::Error;
use wgpu::util::DeviceExt;

const SAMPLES: u32 = 4;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter found: {0}")]
    NoAdapter(String),
    #[error("GPU device request failed: {0}")]
    Device(String),
    #[error("GPU readback failed: {0}")]
    Readback(String),
}

/// Colours for the non-scene parts of the canvas.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Rgba,
    pub grid_minor: Rgba,
    pub grid_major: Rgba,
    pub selection: Rgba,
    pub hover: Rgba,
    pub box_fill: Rgba,
    pub box_stroke: Rgba,
    pub debug_text: Rgba,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: Rgba::rgb(11, 14, 20),
            grid_minor: Rgba::rgb(24, 29, 40),
            grid_major: Rgba::rgb(40, 48, 66),
            selection: Rgba::rgb(255, 214, 10),
            hover: Rgba::rgb(120, 200, 255),
            box_fill: Rgba::with_alpha(80, 140, 255, 40),
            box_stroke: Rgba::rgb(80, 140, 255),
            debug_text: Rgba::rgb(120, 255, 160),
        }
    }
    pub fn light() -> Self {
        Self {
            background: Rgba::rgb(248, 249, 251),
            grid_minor: Rgba::rgb(232, 235, 240),
            grid_major: Rgba::rgb(210, 215, 225),
            selection: Rgba::rgb(214, 120, 0),
            hover: Rgba::rgb(0, 120, 220),
            box_fill: Rgba::with_alpha(0, 100, 255, 30),
            box_stroke: Rgba::rgb(0, 100, 220),
            debug_text: Rgba::rgb(0, 130, 60),
        }
    }
}

/// Everything needed to draw one frame.
pub struct FrameRequest<'a> {
    pub scene: &'a Scene,
    pub view: &'a View,
    pub grid: &'a GridSettings,
    pub theme: &'a Theme,
    pub selection: &'a Selection,
    /// Rubber-band box in screen pixels (start, end), if a box selection is in progress.
    pub box_select: Option<((f64, f64), (f64, f64))>,
    /// Transient world-space shapes (tool previews), drawn above the scene.
    pub preview: &'a [(Shape, Rgba)],
    /// Draw the FPS / statistics overlay.
    pub debug: bool,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub objects: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub mesh_rebuilt: bool,
    pub tessellate_ms: f32,
    pub gpu_ms: f32,
    pub readback_ms: f32,
    pub frame_ms: f32,
    /// Smoothed frames per second (exponential moving average of frame time).
    pub fps: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scale: [f32; 2],
    offset: [f32; 2],
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct WorldCache {
    revision: u64,
    bucket: i32,
    origin: (f64, f64),
    mesh: Option<GpuMesh>,
}

struct Targets {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
}

pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
    cache: Option<WorldCache>,
    stats: RenderStats,
    adapter_name: String,
}

fn zoom_bucket(scale: f64) -> i32 {
    (scale.ln() * 4.0).round() as i32
}

fn bucket_scale(bucket: i32) -> f64 {
    (bucket as f64 / 4.0).exp()
}

impl GpuRenderer {
    /// Creates the renderer, blocking on adapter/device setup. Fails without a usable adapter
    /// (callers and tests handle that explicitly; there is no silent CPU fallback).
    pub fn new_blocking() -> Result<Self, RenderError> {
        pollster::block_on(Self::new())
    }

    pub async fn new() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| RenderError::Device(e.to_string()))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("buturecad-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("triangles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            bind_layout,
            targets: None,
            cache: None,
            stats: RenderStats::default(),
            adapter_name,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    fn ensure_targets(&mut self, width: u32, height: u32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height)
        {
            return;
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |label, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let msaa = make("msaa", SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = make(
            "resolve",
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.targets = Some(Targets {
            width,
            height,
            msaa_view: msaa.create_view(&Default::default()),
            resolve_view: resolve.create_view(&Default::default()),
            resolve,
            readback,
            padded_row,
        });
    }

    fn upload(&self, mesh: &Mesh) -> Option<GpuMesh> {
        if mesh.indices.is_empty() {
            return None;
        }
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Some(GpuMesh {
            vertices,
            indices,
            index_count: mesh.indices.len() as u32,
        })
    }

    fn uniform_bind(&self, u: Uniforms) -> wgpu::BindGroup {
        let buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("uniforms"),
                contents: bytemuck::bytes_of(&u),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform-bind"),
            layout: &self.bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            }],
        })
    }

    /// Renders one frame and returns its pixels.
    pub fn render(&mut self, req: &FrameRequest<'_>) -> Result<Frame, RenderError> {
        let t_frame = Instant::now();
        let (w, h) = (
            req.view.width.round().max(1.0) as u32,
            req.view.height.round().max(1.0) as u32,
        );
        self.ensure_targets(w, h);

        // 1. Cached scene mesh (rebuilt only when the scene or the zoom bucket changes).
        let bucket = zoom_bucket(req.view.scale);
        let stale = self
            .cache
            .as_ref()
            .is_none_or(|c| c.revision != req.scene.revision() || c.bucket != bucket);
        let t_tess = Instant::now();
        let mut rebuilt = false;
        if stale {
            let origin = req
                .scene
                .bounds()
                .map(|b| (b.center().x as f64, b.center().y as f64))
                .unwrap_or((0.0, 0.0));
            let mesh = tessellate_scene(req.scene, bucket_scale(bucket), origin);
            self.stats.vertices = mesh.vertices.len();
            self.stats.triangles = mesh.triangle_count();
            self.cache = Some(WorldCache {
                revision: req.scene.revision(),
                bucket,
                origin,
                mesh: self.upload(&mesh),
            });
            rebuilt = true;
        }

        // 2. Per-frame meshes (small): highlights/previews in world space, grid/box/debug in pixels.
        let centre = req.view.center;
        let mut dynamic = MeshBuilder::new(centre, req.view.scale);
        let push_highlight = |b: &mut MeshBuilder, id, colour| {
            if let Some(item) = req.scene.get(id) {
                let drawn = req
                    .scene
                    .layer(item.layer)
                    .is_some_and(|l| l.visible && !l.hit_only);
                if drawn {
                    b.shape(&item.shape, colour);
                }
            }
        };
        for id in req.selection.selected() {
            push_highlight(&mut dynamic, id, req.theme.selection);
        }
        if let Some(h) = req.selection.hover.filter(|h| !req.selection.contains(*h)) {
            push_highlight(&mut dynamic, h, req.theme.hover);
        }
        for (shape, colour) in req.preview {
            dynamic.shape(shape, *colour);
        }
        let dynamic = dynamic.finish();

        let mut overlay = MeshBuilder::new((0.0, 0.0), 1.0);
        let hpx = req.view.height;
        // Pixel space here is y-up (flip screen y) so the shared shader formula applies.
        for line in grid_lines(req.view, req.grid) {
            let colour = if line.kind == GridLineKind::Major {
                req.theme.grid_major
            } else {
                req.theme.grid_minor
            };
            match (req.grid.style, line.vertical) {
                (GridStyle::Lines, true) => {
                    overlay.rect(line.position - 0.5, 0.0, line.position + 0.5, hpx, colour)
                }
                (GridStyle::Lines, false) => overlay.rect(
                    0.0,
                    hpx - line.position - 0.5,
                    req.view.width,
                    hpx - line.position + 0.5,
                    colour,
                ),
                (GridStyle::Dots, _) => {}
            }
        }
        if req.grid.style == GridStyle::Dots && req.grid.visible {
            let step = crate::grid::adaptive_spacing(req.grid.spacing, req.view.scale);
            let vis = req.view.visible_world();
            let (mut x, y0) = (
                vis.min.x.div_euclid(step) * step,
                vis.min.y.div_euclid(step) * step,
            );
            while x <= vis.max.x {
                let mut y = y0;
                while y <= vis.max.y {
                    let (sx, sy) = req
                        .view
                        .world_to_screen(buturecad_geometry::Point::new(x, y));
                    overlay.rect(
                        sx - 0.75,
                        hpx - sy - 0.75,
                        sx + 0.75,
                        hpx - sy + 0.75,
                        req.theme.grid_major,
                    );
                    y += step;
                }
                x += step;
            }
        }
        if let Some(((x0, y0), (x1, y1))) = req.box_select {
            let (ya, yb) = (hpx - y0, hpx - y1);
            overlay.rect(
                x0.min(x1),
                ya.min(yb),
                x0.max(x1),
                ya.max(yb),
                req.theme.box_fill,
            );
            let (lx, rx, by, ty) = (x0.min(x1), x0.max(x1), ya.min(yb), ya.max(yb));
            for (a, b) in [
                ((lx, by), (rx, by)),
                ((rx, by), (rx, ty)),
                ((rx, ty), (lx, ty)),
                ((lx, ty), (lx, by)),
            ] {
                overlay.line(a, b, 1.0, req.theme.box_stroke);
            }
        }
        if req.debug {
            let s = self.stats;
            let lines = [
                format!("{:.0} FPS  {:.1} MS", s.fps, s.frame_ms),
                format!("OBJ {}  TRI {}", req.scene.len(), s.triangles),
                format!("TESS {:.1} MS  GPU {:.1} MS", s.tessellate_ms, s.gpu_ms),
                format!(
                    "READ {:.1} MS  {}",
                    s.readback_ms,
                    self.adapter_name.to_uppercase()
                ),
            ];
            for (i, l) in lines.iter().enumerate() {
                let y = hpx - 18.0 - i as f64 * 12.0;
                overlay.text(
                    buturecad_geometry::Point::new(8, y as i64),
                    7,
                    &l.chars().take(60).collect::<String>(),
                    req.theme.debug_text,
                );
            }
        }
        let overlay = overlay.finish();
        self.stats.tessellate_ms = t_tess.elapsed().as_secs_f32() * 1000.0;

        // 3. Record and submit.
        let t_gpu = Instant::now();
        let dynamic_gpu = self.upload(&dynamic);
        let overlay_gpu = self.upload(&overlay);
        let (wf, hf) = (w as f32, h as f32);
        let s = req.view.scale as f32;
        let world_uniform = |origin: (f64, f64)| {
            let cam = ((centre.0 - origin.0) as f32, (centre.1 - origin.1) as f32);
            let scale = [2.0 * s / wf, 2.0 * s / hf];
            Uniforms {
                scale,
                offset: [-cam.0 * scale[0], -cam.1 * scale[1]],
            }
        };
        let origin = self.cache.as_ref().map_or((0.0, 0.0), |c| c.origin);
        let world_bind = self.uniform_bind(world_uniform(origin));
        let dynamic_bind = self.uniform_bind(world_uniform(centre));
        let pixel_bind = self.uniform_bind(Uniforms {
            scale: [2.0 / wf, 2.0 / hf],
            offset: [-1.0, -1.0],
        });

        let Some(targets) = self.targets.as_ref() else {
            return Err(RenderError::Readback("render targets missing".into()));
        };
        let bg = req.theme.background.0.map(|c| c as f64 / 255.0);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&targets.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0],
                            g: bg[1],
                            b: bg[2],
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            let draw = |pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, bind: &wgpu::BindGroup| {
                pass.set_bind_group(0, bind, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            };
            // Grid first (under everything), then the scene, highlights/previews, box and debug text.
            if let Some(m) = overlay_gpu.as_ref() {
                draw(&mut pass, m, &pixel_bind);
            }
            if let Some(m) = self.cache.as_ref().and_then(|c| c.mesh.as_ref()) {
                draw(&mut pass, m, &world_bind);
            }
            if let Some(m) = dynamic_gpu.as_ref() {
                draw(&mut pass, m, &dynamic_bind);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        self.stats.gpu_ms = t_gpu.elapsed().as_secs_f32() * 1000.0;

        // 4. Read back.
        let t_read = Instant::now();
        let slice = targets.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        {
            let data = slice
                .get_mapped_range()
                .map_err(|e| RenderError::Readback(e.to_string()))?;
            for row in 0..h as usize {
                let start = row * targets.padded_row as usize;
                rgba.extend_from_slice(&data[start..start + w as usize * 4]);
            }
        }
        targets.readback.unmap();
        self.stats.readback_ms = t_read.elapsed().as_secs_f32() * 1000.0;

        let frame_ms = t_frame.elapsed().as_secs_f32() * 1000.0;
        self.stats.objects = req.scene.len();
        self.stats.mesh_rebuilt = rebuilt;
        self.stats.frame_ms = frame_ms;
        let inst_fps = 1000.0 / frame_ms.max(0.01);
        self.stats.fps = if self.stats.fps == 0.0 {
            inst_fps
        } else {
            self.stats.fps * 0.9 + inst_fps * 0.1
        };
        Ok(Frame {
            width: w,
            height: h,
            rgba,
        })
    }
}
