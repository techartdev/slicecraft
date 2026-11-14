//! The 3D view on the graphics card, through wgpu (Direct3D 12 or Vulkan on
//! Windows, Metal on macOS, Vulkan or OpenGL on Linux).
//!
//! The scene is drawn into a texture of its own, with a depth buffer and 4×
//! anti-aliasing, and egui shows that texture where the view is. Models are
//! lit meshes; every extrusion is a lit bead of its real width and layer
//! height, built on the card from one small template; travel moves are lines.
//! Geometry is uploaded once per slice or model change: scrubbing layers,
//! playing, changing the colouring or hiding a kind only changes a few
//! numbers or a draw range, so it costs nothing per move.
//!
//! Everything here is safe Rust (wgpu's API is safe). Each frame's GPU work
//! runs inside an error scope: a refused buffer or shader turns the GPU view
//! off with a note, and the CPU view takes over, instead of stopping the
//! window.

use crate::camera::Frame;
use crate::scene::{
    self, BedStyle, Coloring, KindFilter, ModelGeometry, PreviewLayer, Prism, SegKind,
};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use eframe::wgpu::{self, util::DeviceExt};
use slicecraft_doc::{ModelRole, PrintSettings};
use slicecraft_geom::Vec3;
use slicecraft_render::PathKind;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

const SAMPLES: u32 = 4;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Bytes of the shared uniform block (see `Globals` in gpu.wgsl).
const GLOBALS_BYTES: usize = 64 + 10 * 16 + 16 + 16 * 16 + 8 * 16;
/// Bytes per toolpath bead instance.
const BEAD_BYTES: usize = 36;
/// Bytes per mesh vertex: position, normal, colour.
const MESH_BYTES: usize = 28;
/// Bytes per line vertex: position, colour.
const LINE_BYTES: usize = 16;
/// Width and height of a bead are stored as fractions of this (mm).
const SIZE_SCALE: f32 = 4.0;
/// The biggest side of the view texture (px).
const MAX_SIDE: u32 = 8192;

/// What the view draws this frame.
pub struct Scene<'a> {
    pub frame: &'a Frame,
    /// The view's size in points, and points per pixel.
    pub size: egui::Vec2,
    pub pixels_per_point: f32,
    pub dark: bool,
    pub bed: (f64, f64, f64),
    pub round_bed: bool,
    pub bed_style: BedStyle,
    /// Models, when they are shown (prepare mode, or the outline in preview).
    pub models: Option<Models<'a>>,
    /// Toolpaths, in preview mode.
    pub paths: Option<Paths<'a>>,
}

pub struct Models<'a> {
    pub geometry: &'a ModelGeometry,
    /// The geometry's revision, to know when to upload it again.
    pub revision: u64,
    pub selected: &'a BTreeSet<usize>,
    pub support: &'a [Prism],
    pub cut_z: Option<f64>,
    /// Drawn see-through over the toolpaths.
    pub ghost: bool,
}

pub struct Paths<'a> {
    pub layers: &'a [PreviewLayer],
    /// Which slice the layers came from, to know when to upload again.
    pub key: usize,
    pub settings: &'a PrintSettings,
    pub first_layer: usize,
    pub last_layer: usize,
    /// Moves of the last layer to draw (stepping move by move).
    pub moves_on_last: Option<usize>,
    pub coloring: Coloring,
    pub filter: KindFilter,
    pub show_travel: bool,
}

struct Target {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
    id: egui::TextureId,
}

struct Pipelines {
    sky: wgpu::RenderPipeline,
    plate: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    ghost: wgpu::RenderPipeline,
    bead: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
}

struct MeshBuffer {
    key: u64,
    buffer: Option<wgpu::Buffer>,
    vertices: u32,
}

struct PathBuffers {
    key: usize,
    beads: Option<wgpu::Buffer>,
    /// Bead index where each layer starts; one more entry for the end.
    bead_start: Vec<u32>,
    travel: Option<wgpu::Buffer>,
    /// Travel vertex index where each layer starts; one more for the end.
    travel_start: Vec<u32>,
}

struct LineBuffer {
    key: u64,
    buffer: Option<wgpu::Buffer>,
    vertices: u32,
}

/// The GPU side of the 3D view.
pub struct Gpu {
    rs: RenderState,
    pipelines: Pipelines,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    template: wgpu::Buffer,
    indices: wgpu::Buffer,
    target: Option<Target>,
    models: Option<MeshBuffer>,
    paths: Option<PathBuffers>,
    frame_lines: Option<LineBuffer>,
}

/// Run `work` inside a validation and out-of-memory scope; the first error
/// becomes an `Err` instead of reaching wgpu's handler.
fn scoped<T>(device: &wgpu::Device, work: impl FnOnce() -> T) -> Result<T, String> {
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let out = work();
    let first = ready(validation.pop());
    let second = ready(memory.pop());
    match (first, second) {
        (Some(Some(e)), _) | (_, Some(Some(e))) => Err(e.to_string()),
        (None, _) | (_, None) => Err("the graphics card did not answer in time".into()),
        _ => Ok(out),
    }
}

/// The value of a future that is already complete (wgpu's native error
/// scopes are), without an async runtime. `None` when it is still pending.
fn ready<F: std::future::Future>(f: F) -> Option<F::Output> {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

/// An sRGB colour as linear floats, for the sRGB render target.
fn linear(c: Color32) -> [f32; 4] {
    let f = |v: u8| {
        let x = f32::from(v) / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    };
    [f(c.r()), f(c.g()), f(c.b()), f32::from(c.a()) / 255.0]
}

fn push_f32s(out: &mut Vec<u8>, v: &[f32]) {
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn push_vec3(out: &mut Vec<u8>, v: Vec3) {
    push_f32s(out, &[v.x as f32, v.y as f32, v.z as f32]);
}

fn unorm16(x: f32) -> [u8; 2] {
    let v = if x.is_finite() { (x.clamp(0.0, 1.0) * 65535.0).round() as u16 } else { 0 };
    v.to_le_bytes()
}

impl Gpu {
    /// Set the GPU view up, or say why it cannot be.
    pub fn new(rs: &RenderState) -> Result<Self, String> {
        let device = &rs.device;
        scoped(device, || Self::build(rs))
    }

    fn build(rs: &RenderState) -> Self {
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slicecraft view"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: GLOBALS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("view"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let depth = |write: bool, compare: wgpu::CompareFunction| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout<'_>],
                        topology: wgpu::PrimitiveTopology,
                        depth_state: wgpu::DepthStencilState,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(depth_state),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: MESH_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4],
        };
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: LINE_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
        };
        let template_layout = wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let bead_layout = wgpu::VertexBufferLayout {
            array_stride: BEAD_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                1 => Float32x3, 2 => Float32x3, 3 => Unorm16x2, 4 => Unorm16x2, 5 => Uint32
            ],
        };
        use wgpu::CompareFunction::{Always, Greater, GreaterEqual};
        use wgpu::PrimitiveTopology::{LineList, TriangleList};
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        let pipelines = Pipelines {
            sky: pipeline("sky", "vs_sky", "fs_sky", &[], TriangleList, depth(false, Always), None),
            plate: pipeline(
                "plate",
                "vs_plate",
                "fs_plate",
                &[],
                TriangleList,
                depth(true, Greater),
                None,
            ),
            mesh: pipeline(
                "mesh",
                "vs_mesh",
                "fs_mesh",
                std::slice::from_ref(&mesh_layout),
                TriangleList,
                depth(true, Greater),
                None,
            ),
            ghost: pipeline(
                "ghost",
                "vs_mesh",
                "fs_ghost",
                &[mesh_layout],
                TriangleList,
                depth(false, Greater),
                alpha,
            ),
            bead: pipeline(
                "beads",
                "vs_path",
                "fs_path",
                &[template_layout, bead_layout],
                TriangleList,
                depth(true, Greater),
                None,
            ),
            lines: pipeline(
                "lines",
                "vs_line",
                "fs_line",
                &[line_layout],
                LineList,
                depth(false, GreaterEqual),
                alpha,
            ),
        };
        // the bead template: two rings of four corners, and a tip at each end
        let mut tmpl = Vec::new();
        for (x, y) in [
            (0.0, 0.0),
            (0.0, 1.0),
            (0.0, 2.0),
            (0.0, 3.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (1.0, 2.0),
            (1.0, 3.0),
            (-1.0, 0.0),
            (2.0, 0.0),
        ] {
            push_f32s(&mut tmpl, &[x, y]);
        }
        let mut idx: Vec<u16> = Vec::with_capacity(48);
        for c in 0..4u16 {
            let n = (c + 1) % 4;
            // side quad between the rings
            idx.extend_from_slice(&[c, n, 4 + n, c, 4 + n, 4 + c]);
            // end fans to the tips
            idx.extend_from_slice(&[8, n, c, 9, 4 + c, 4 + n]);
        }
        let idx_bytes: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        let template = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bead template"),
            contents: &tmpl,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bead indices"),
            contents: &idx_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            rs: rs.clone(),
            pipelines,
            globals,
            globals_group,
            template,
            indices,
            target: None,
            models: None,
            paths: None,
            frame_lines: None,
        }
    }

    /// Draw `scene` and give the texture to show, or the reason it failed.
    pub fn render(&mut self, scene: &Scene<'_>) -> Result<egui::TextureId, String> {
        let device = self.rs.device.clone();
        scoped(&device, || self.render_inner(scene))?
    }

    fn render_inner(&mut self, scene: &Scene<'_>) -> Result<egui::TextureId, String> {
        let px = |v: f32| ((v * scene.pixels_per_point).round() as u32).clamp(1, MAX_SIDE);
        let size = [px(scene.size.x), px(scene.size.y)];
        self.ensure_target(size);
        self.upload_models(scene);
        self.upload_paths(scene)?;
        self.upload_frame_lines(scene);
        self.write_globals(scene);
        let Some(target) = &self.target else { return Err("no view texture".into()) };
        let device = &self.rs.device;
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("view") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_pipeline(&self.pipelines.sky);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.pipelines.plate);
            pass.draw(0..6, 0..1);
            let ghost = scene.models.as_ref().is_some_and(|m| m.ghost);
            if let (Some(m), false) = (&self.models, ghost)
                && let Some(b) = &m.buffer
                && m.vertices > 0
            {
                pass.set_pipeline(&self.pipelines.mesh);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..m.vertices, 0..1);
            }
            if let (Some(p), Some(input)) = (&self.paths, &scene.paths) {
                let (beads, travel) = draw_ranges(p, input);
                if let Some(b) = &p.beads
                    && beads.end > beads.start
                {
                    pass.set_pipeline(&self.pipelines.bead);
                    pass.set_vertex_buffer(0, self.template.slice(..));
                    pass.set_vertex_buffer(1, b.slice(..));
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..48, 0, beads);
                }
                if input.show_travel
                    && let Some(t) = &p.travel
                    && travel.end > travel.start
                {
                    pass.set_pipeline(&self.pipelines.lines);
                    pass.set_vertex_buffer(0, t.slice(..));
                    pass.draw(travel, 0..1);
                }
            }
            if let Some(l) = &self.frame_lines
                && let Some(b) = &l.buffer
                && l.vertices > 0
            {
                pass.set_pipeline(&self.pipelines.lines);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..l.vertices, 0..1);
            }
            if let (Some(m), true) = (&self.models, ghost)
                && let Some(b) = &m.buffer
                && m.vertices > 0
            {
                pass.set_pipeline(&self.pipelines.ghost);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..m.vertices, 0..1);
            }
        }
        self.rs.queue.submit(Some(encoder.finish()));
        Ok(target.id)
    }

    fn ensure_target(&mut self, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let device = &self.rs.device;
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texture = |label: &str, format, samples, usage, views: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: views,
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let view = |t: wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa = view(texture("view msaa", COLOR, SAMPLES, attach, &[]));
        let depth = view(texture("view depth", DEPTH, SAMPLES, attach, &[]));
        // drawn in sRGB (so light adds up right) and shown to egui as plain
        // RGBA, which is how egui reads its own textures: gamma-encoded bytes
        let shown = texture(
            "view",
            COLOR,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING,
            &[wgpu::TextureFormat::Rgba8Unorm],
        );
        let egui_view = shown.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let resolve = view(shown);
        let mut renderer = self.rs.renderer.write();
        let id = match self.target.take() {
            Some(old) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    &egui_view,
                    wgpu::FilterMode::Linear,
                    old.id,
                );
                old.id
            }
            None => renderer.register_native_texture(device, &egui_view, wgpu::FilterMode::Linear),
        };
        self.target = Some(Target { size, msaa, depth, resolve, id });
    }

    fn buffer(&self, label: &str, bytes: &[u8], usage: wgpu::BufferUsages) -> Option<wgpu::Buffer> {
        if bytes.is_empty() {
            return None;
        }
        Some(self.rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytes,
            usage,
        }))
    }

    fn upload_models(&mut self, scene: &Scene<'_>) {
        let Some(m) = &scene.models else {
            self.models = None;
            return;
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        m.revision.hash(&mut h);
        m.selected.hash(&mut h);
        m.support.len().hash(&mut h);
        m.support.as_ptr().hash(&mut h);
        let key = h.finish();
        if self.models.as_ref().is_some_and(|b| b.key == key) {
            return;
        }
        let bytes = mesh_bytes(m);
        let vertices = (bytes.len() / MESH_BYTES) as u32;
        let buffer = self.buffer("models", &bytes, wgpu::BufferUsages::VERTEX);
        self.models = Some(MeshBuffer { key, buffer, vertices });
    }

    fn upload_paths(&mut self, scene: &Scene<'_>) -> Result<(), String> {
        let Some(p) = &scene.paths else {
            return Ok(());
        };
        if self.paths.as_ref().is_some_and(|b| b.key == p.key) {
            return Ok(());
        }
        let built = bead_bytes(p.layers, p.settings);
        let limit = self.rs.device.limits().max_buffer_size;
        if built.beads.len() as u64 > limit || built.travel.len() as u64 > limit {
            return Err(format!(
                "this job has more moves than the graphics card takes in one buffer ({} MB)",
                limit / (1 << 20)
            ));
        }
        let beads = self.buffer("beads", &built.beads, wgpu::BufferUsages::VERTEX);
        let travel = self.buffer("travel", &built.travel, wgpu::BufferUsages::VERTEX);
        self.paths = Some(PathBuffers {
            key: p.key,
            beads,
            bead_start: built.bead_start,
            travel,
            travel_start: built.travel_start,
        });
        Ok(())
    }

    fn upload_frame_lines(&mut self, scene: &Scene<'_>) {
        let (w, d, ht) = scene.bed;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in [w, d, ht] {
            v.to_bits().hash(&mut h);
        }
        (scene.bed_style.volume, scene.bed_style.axes, scene.round_bed, scene.dark).hash(&mut h);
        let key = h.finish();
        if self.frame_lines.as_ref().is_some_and(|l| l.key == key) {
            return;
        }
        let bytes = frame_line_bytes(scene);
        let vertices = (bytes.len() / LINE_BYTES) as u32;
        let buffer = self.buffer("bed lines", &bytes, wgpu::BufferUsages::VERTEX);
        self.frame_lines = Some(LineBuffer { key, buffer, vertices });
    }

    fn write_globals(&self, scene: &Scene<'_>) {
        let bytes = globals_bytes(scene);
        self.rs.queue.write_buffer(&self.globals, 0, &bytes);
    }

    /// The graphics card and the interface wgpu talks to it through.
    pub fn describe(&self) -> String {
        let i = self.rs.adapter.get_info();
        format!("{} · {:?}", i.name, i.backend)
    }

    /// Forget the uploaded geometry (after the GPU view was turned off).
    pub fn clear(&mut self) {
        self.models = None;
        self.paths = None;
        self.frame_lines = None;
    }
}

/// The beads and travel vertices to draw for the layer range and move cursor.
fn draw_ranges(p: &PathBuffers, input: &Paths<'_>) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    let layers = p.bead_start.len().saturating_sub(1);
    if layers == 0 {
        return (0..0, 0..0);
    }
    let last = input.last_layer.min(layers - 1);
    let first = input.first_layer.min(last);
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    let mut bead_end = at(&p.bead_start, last + 1);
    let mut travel_end = at(&p.travel_start, last + 1);
    if let Some(cursor) = input.moves_on_last
        && let Some(layer) = input.layers.get(last)
    {
        // count the beads and travels among the first `cursor` moves
        let shown = layer.segs.iter().take(cursor);
        let (mut beads, mut travels) = (0u32, 0u32);
        for s in shown {
            match s.kind {
                SegKind::Travel => travels += 1,
                SegKind::Path(_) => beads += 1,
            }
        }
        bead_end = at(&p.bead_start, last) + beads;
        travel_end = at(&p.travel_start, last) + travels * 2;
    }
    (at(&p.bead_start, first)..bead_end, at(&p.travel_start, first)..travel_end)
}

/// The packed toolpaths of a slice.
pub struct Built {
    pub beads: Vec<u8>,
    pub bead_start: Vec<u32>,
    pub travel: Vec<u8>,
    pub travel_start: Vec<u32>,
}

fn kind_index(k: PathKind) -> u32 {
    PathKind::ALL.iter().position(|x| *x == k).unwrap_or(0) as u32
}

/// Pack every extrusion as a bead instance and every travel as a line.
pub fn bead_bytes(layers: &[PreviewLayer], s: &PrintSettings) -> Built {
    let (s_lo, s_hi) = scene::speed_range(layers);
    let (f_lo, f_hi) = scene::flow_range(layers);
    let norm = |v: f32, lo: f32, hi: f32| (v - lo) / (hi - lo).max(1e-3);
    let mut out = Built {
        beads: Vec::new(),
        bead_start: Vec::with_capacity(layers.len() + 1),
        travel: Vec::new(),
        travel_start: Vec::with_capacity(layers.len() + 1),
    };
    let travel_colour = scene::TRAVEL.to_array();
    for (i, l) in layers.iter().enumerate() {
        out.bead_start.push((out.beads.len() / BEAD_BYTES) as u32);
        out.travel_start.push((out.travel.len() / LINE_BYTES) as u32);
        let width = slicecraft_io::gcode::layer_line_width(s, i) as f32;
        let height = l.height as f32;
        let layer = (i as u32).min(0x00ff_ffff);
        for seg in &l.segs {
            match seg.kind {
                SegKind::Travel => {
                    for p in [seg.a, seg.b] {
                        push_vec3(&mut out.travel, p);
                        out.travel.extend_from_slice(&travel_colour);
                    }
                }
                SegKind::Path(kind) => {
                    push_vec3(&mut out.beads, seg.a);
                    push_vec3(&mut out.beads, seg.b);
                    out.beads.extend_from_slice(&unorm16(width / SIZE_SCALE));
                    out.beads.extend_from_slice(&unorm16(height / SIZE_SCALE));
                    out.beads.extend_from_slice(&unorm16(norm(seg.speed, s_lo, s_hi)));
                    out.beads.extend_from_slice(&unorm16(norm(seg.flow, f_lo, f_hi)));
                    let info =
                        (kind_index(kind) & 15) | (u32::from(seg.tool) & 15) << 4 | layer << 8;
                    out.beads.extend_from_slice(&info.to_le_bytes());
                }
            }
        }
    }
    out.bead_start.push((out.beads.len() / BEAD_BYTES) as u32);
    out.travel_start.push((out.travel.len() / LINE_BYTES) as u32);
    out
}

/// Model triangles (and the support preview) with their colours.
fn mesh_bytes(m: &Models<'_>) -> Vec<u8> {
    let total: usize = m.geometry.objects.iter().map(|o| o.triangles.len()).sum();
    let mut out = Vec::with_capacity((total + m.support.len() * 24) * 3 * MESH_BYTES);
    let tri = |out: &mut Vec<u8>, v: [Vec3; 3], n: Vec3, c: [u8; 4]| {
        for p in v {
            push_vec3(out, p);
            push_vec3(out, n);
            out.extend_from_slice(&c);
        }
    };
    for (oi, o) in m.geometry.objects.iter().enumerate() {
        let base = if o.outside_bed {
            scene::MODEL_OUTSIDE
        } else if m.selected.contains(&oi) {
            scene::MODEL_SELECTED
        } else {
            match o.role {
                ModelRole::Print => scene::MODEL,
                ModelRole::Support => scene::MODEL_SUPPORT,
                ModelRole::Cutter => scene::MODEL_CUTTER,
            }
        };
        // a model left out of the slice is drawn greyed
        let c = if o.visible { base } else { lerp(base, Color32::from_gray(90), 0.65) };
        let c = c.to_array();
        for (v, n) in &o.triangles {
            let n = n.unwrap_or_else(|| {
                (v[1] - v[0]).cross(v[2] - v[0]).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0))
            });
            tri(&mut out, *v, n, c);
        }
    }
    let support = scene::kind_color(PathKind::Support).to_array();
    for prism in m.support {
        let k = prism.outline.len();
        for i in 0..k {
            let (Some(a), Some(b)) = (prism.outline.get(i), prism.outline.get((i + 1) % k)) else {
                continue;
            };
            let Some(n) = Vec3::new(b.y - a.y, a.x - b.x, 0.0).normalized() else { continue };
            let q = [
                Vec3::new(a.x, a.y, prism.z0),
                Vec3::new(b.x, b.y, prism.z0),
                Vec3::new(b.x, b.y, prism.z1),
                Vec3::new(a.x, a.y, prism.z1),
            ];
            tri(&mut out, [q[0], q[1], q[2]], n, support);
            tri(&mut out, [q[0], q[2], q[3]], n, support);
        }
        // the top: a fan, right for the convex pillars
        if let Some((first, rest)) = prism.outline.split_first() {
            let up = Vec3::new(0.0, 0.0, 1.0);
            for w in rest.windows(2) {
                if let [b, c] = w {
                    let p = |q: &slicecraft_geom::Point2| Vec3::new(q.x, q.y, prism.z1);
                    tri(&mut out, [p(first), p(b), p(c)], up, support);
                }
            }
        }
    }
    out
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// The bed's edge, the build volume and the axes, as lines.
fn frame_line_bytes(scene: &Scene<'_>) -> Vec<u8> {
    let (w, d, h) = scene.bed;
    let mut out = Vec::new();
    let mut line = |a: Vec3, b: Vec3, c: Color32| {
        for p in [a, b] {
            push_vec3(&mut out, p);
            out.extend_from_slice(&c.to_array());
        }
    };
    let edge = if scene.dark {
        Color32::from_rgb(0x8a, 0x90, 0x9a)
    } else {
        Color32::from_rgb(0x7d, 0x86, 0x92)
    };
    let volume = Color32::from_rgba_unmultiplied(edge.r(), edge.g(), edge.b(), 110);
    if scene.round_bed {
        let r = w / 2.0;
        let n = 128;
        for k in 0..n {
            let a0 = f64::from(k) / f64::from(n) * std::f64::consts::TAU;
            let a1 = f64::from(k + 1) / f64::from(n) * std::f64::consts::TAU;
            line(
                Vec3::new(r + r * a0.cos(), r + r * a0.sin(), 0.0),
                Vec3::new(r + r * a1.cos(), r + r * a1.sin(), 0.0),
                edge,
            );
        }
    } else {
        let c = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(w, 0.0, 0.0),
            Vec3::new(w, d, 0.0),
            Vec3::new(0.0, d, 0.0),
        ];
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            line(a, b, edge);
            if scene.bed_style.volume {
                let up = Vec3::new(0.0, 0.0, h);
                line(a + up, b + up, volume);
                line(a, a + up, volume);
            }
        }
    }
    if scene.bed_style.axes {
        let len = (w.min(d) * 0.12).clamp(10.0, 40.0);
        for (k, dir) in
            [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)]
                .into_iter()
                .enumerate()
        {
            let colour = crate::theme::AXES.get(k).copied().unwrap_or(Color32::GRAY);
            line(Vec3::new(0.0, 0.0, 0.05), dir.scale(len) + Vec3::new(0.0, 0.0, 0.05), colour);
        }
    }
    out
}

fn globals_bytes(scene: &Scene<'_>) -> Vec<u8> {
    let p = crate::theme::palette(scene.dark);
    let mut out = Vec::with_capacity(GLOBALS_BYTES);
    let f = scene.frame;
    push_f32s(&mut out, &f.clip_matrix(f64::from(scene.size.x), f64::from(scene.size.y)));
    let eye = f.eye();
    push_f32s(&mut out, &[eye.x as f32, eye.y as f32, eye.z as f32, 1.0]);
    let l = f.light();
    push_f32s(&mut out, &[l.x as f32, l.y as f32, l.z as f32, 0.0]);
    push_f32s(&mut out, &linear(p.sky));
    push_f32s(&mut out, &linear(p.ground));
    let (w, d, _) = scene.bed;
    let grid = if scene.bed_style.grid { 1.0 } else { 0.0 };
    let round = if scene.round_bed { 1.0 } else { 0.0 };
    push_f32s(&mut out, &[w as f32, d as f32, grid, round]);
    let [plate, minor, major] = if scene.dark {
        [
            Color32::from_rgb(0x2b, 0x2e, 0x33),
            Color32::from_rgb(0x3a, 0x3e, 0x45),
            Color32::from_rgb(0x55, 0x5b, 0x65),
        ]
    } else {
        [
            Color32::from_rgb(0xf4, 0xf5, 0xf7),
            Color32::from_rgb(0xdc, 0xe0, 0xe6),
            Color32::from_rgb(0xbd, 0xc4, 0xce),
        ]
    };
    for c in [plate, minor, major] {
        push_f32s(&mut out, &linear(c));
    }
    let cut = scene.models.as_ref().and_then(|m| m.cut_z).map_or(1.0e9, |z| z as f32);
    let (mode, last, mask, zr) = match &scene.paths {
        Some(pp) => {
            let mode = match pp.coloring {
                Coloring::Feature => 0.0,
                Coloring::Speed => 1.0,
                Coloring::Height => 2.0,
                Coloring::Tool => 3.0,
                Coloring::Flow => 4.0,
            };
            let mut mask = 0u32;
            for (i, k) in PathKind::ALL.iter().enumerate().take(16) {
                if pp.filter.shows(*k) {
                    mask |= 1 << i;
                }
            }
            let top = pp.layers.last().map_or(1.0, |l| l.z) as f32;
            (mode, pp.last_layer as f32, mask, (0.0f32, top))
        }
        None => (0.0, 0.0, 0, (0.0, 1.0)),
    };
    push_f32s(&mut out, &[cut, mode, 0.16, last]);
    push_f32s(&mut out, &[zr.0, zr.1, 0.0, 0.0]);
    for v in [mask, 0, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for i in 0..16 {
        let c = PathKind::ALL.get(i).map_or(Color32::GRAY, |k| scene::kind_color(*k));
        push_f32s(&mut out, &linear(c));
    }
    for t in 0..8u8 {
        push_f32s(&mut out, &linear(scene::tool_color(t)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::PreviewSeg;

    fn seg(kind: SegKind, x: f64, z: f64) -> PreviewSeg {
        PreviewSeg {
            a: Vec3::new(x, 0.0, z),
            b: Vec3::new(x + 1.0, 0.0, z),
            kind,
            speed: 40.0,
            tool: 1,
            flow: 2.0,
        }
    }

    fn layers() -> Vec<PreviewLayer> {
        let wall = SegKind::Path(PathKind::OuterPerimeter);
        (0..3)
            .map(|i| {
                let z = 0.2 * f64::from(i + 1);
                PreviewLayer {
                    z,
                    height: 0.2,
                    segs: vec![seg(wall, 0.0, z), seg(SegKind::Travel, 1.0, z), seg(wall, 2.0, z)],
                    retractions: Vec::new(),
                }
            })
            .collect()
    }

    #[test]
    fn beads_pack_by_layer() {
        let b = bead_bytes(&layers(), &PrintSettings::default());
        assert_eq!(b.beads.len(), 6 * BEAD_BYTES);
        assert_eq!(b.travel.len(), 3 * 2 * LINE_BYTES);
        assert_eq!(b.bead_start, vec![0, 2, 4, 6]);
        assert_eq!(b.travel_start, vec![0, 2, 4, 6]);
        // the third bead: layer 1, tool 1, outer perimeter
        let info_at = 2 * BEAD_BYTES + 32;
        let info = u32::from_le_bytes([
            b.beads[info_at],
            b.beads[info_at + 1],
            b.beads[info_at + 2],
            b.beads[info_at + 3],
        ]);
        assert_eq!(info >> 8, 1);
        assert_eq!((info >> 4) & 15, 1);
        assert_eq!(info & 15, kind_index(PathKind::OuterPerimeter));
    }

    #[test]
    fn layer_range_and_move_cursor_pick_the_draw_range() {
        let ls = layers();
        let b = bead_bytes(&ls, &PrintSettings::default());
        let p = PathBuffers {
            key: 0,
            beads: None,
            bead_start: b.bead_start,
            travel: None,
            travel_start: b.travel_start,
        };
        let s = PrintSettings::default();
        let mut input = Paths {
            layers: &ls,
            key: 0,
            settings: &s,
            first_layer: 1,
            last_layer: 2,
            moves_on_last: None,
            coloring: Coloring::Feature,
            filter: KindFilter::default(),
            show_travel: true,
        };
        assert_eq!(draw_ranges(&p, &input), (2..6, 2..6));
        // two moves into the last layer: one bead, one travel
        input.moves_on_last = Some(2);
        assert_eq!(draw_ranges(&p, &input), (2..5, 2..6));
        input.last_layer = 99;
        input.moves_on_last = None;
        assert_eq!(draw_ranges(&p, &input), (2..6, 2..6));
    }

    #[test]
    fn the_uniform_block_has_the_size_the_shader_expects() {
        let mut cam = crate::camera::OrbitCamera::for_bed(220.0, 220.0, 250.0);
        cam.ortho = false;
        let f =
            cam.frame(crate::camera::Viewport { left: 0.0, top: 0.0, width: 800.0, height: 600.0 });
        let scene = Scene {
            frame: &f,
            size: egui::vec2(800.0, 600.0),
            pixels_per_point: 1.0,
            dark: true,
            bed: (220.0, 220.0, 250.0),
            round_bed: false,
            bed_style: BedStyle::default(),
            models: None,
            paths: None,
        };
        assert_eq!(globals_bytes(&scene).len(), GLOBALS_BYTES);
        assert!(!frame_line_bytes(&scene).is_empty());
    }

    #[test]
    fn colours_become_linear() {
        assert_eq!(linear(Color32::BLACK), [0.0, 0.0, 0.0, 1.0]);
        let w = linear(Color32::WHITE);
        assert!((w[0] - 1.0).abs() < 1e-6);
        let mid = linear(Color32::from_gray(128));
        assert!((mid[0] - 0.2158).abs() < 1e-3);
    }

    #[test]
    fn the_shader_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("gpu.wgsl"));
        assert!(
            module.is_ok(),
            "{:?}",
            module.as_ref().err().map(|e| e.emit_to_string(include_str!("gpu.wgsl")))
        );
        let Ok(module) = module else { return };
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module);
        assert!(info.is_ok(), "{info:?}");
        // the uniform block is as big in the shader as the bytes written
        let mut layouter = naga::proc::Layouter::default();
        assert!(layouter.update(module.to_ctx()).is_ok());
        let globals = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Globals"));
        assert!(globals.is_some());
        if let Some((handle, _)) = globals {
            assert_eq!(layouter[handle].size as usize, GLOBALS_BYTES);
        }
        for entry in [
            "vs_sky", "fs_sky", "vs_plate", "fs_plate", "vs_mesh", "fs_mesh", "fs_ghost",
            "vs_path", "fs_path", "vs_line", "fs_line",
        ] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "{entry} missing");
        }
    }

    #[test]
    fn a_complete_future_is_ready() {
        assert_eq!(ready(async { 5 }), Some(5));
    }
}
