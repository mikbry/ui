//! Host-owned GPU and frame example (#175).
//!
//! This binary plays the role of a third-party `wgpu`-based game/engine host
//! embedding mkui as its UI layer — the scenario ADR 0012
//! (`docs/architecture/0012-host-owned-gpu-embedding.md`) and
//! [`mkui_wgpu::EmbeddedRenderer`] exist for. The host (this file) owns
//! everything a real engine would: the `wgpu::Instance`, the adapter, the
//! device, the queue, the window's `Surface`, and a lit 3D pass (a rotating,
//! per-face Lambertian-shaded cube). `mkui-wgpu` owns none of that — it only
//! borrows the host's device and the host's surface format once, at startup
//! (`EmbeddedRenderer::new`), and records its one UI pass into the host's
//! own command encoder and the host's own swapchain view every frame
//! (`EmbeddedRenderer::render_into`).
//!
//! Per frame: ONE `wgpu::CommandEncoder`, carrying the host's 3D pass
//! followed by mkui's UI pass into the *same* swapchain `TextureView`, then
//! ONE `queue.submit`, then present — satisfying #175's acceptance bar: one
//! frame, one queue submission, no extra texture copy.
//!
//! The UI pass itself is a believable HUD/pause-menu scene (#255), built
//! from the components `#173`'s batch 15 shipped: an always-on health
//! `progress_bar` + score `counter_chip` + a "LIVE" `badge`/status `dot`,
//! plus a toast/banner `toast_stack` firing on a timer — and, behind
//! `Escape`, a pause overlay (`glass_panel` chrome, a keyboard-navigable
//! `MenuList`, a `toggle`/`slider_with_label`/`segmented_select` settings
//! row, a `tooltip`, and a `titled_dialog` quit confirmation stacked on
//! top). #230 profiles CPU/GPU frame cost and memory against this scene,
//! not the two-quad placeholder #175 shipped it with; this issue only
//! assembles the scene, it does not measure anything itself.
//!
//! `cargo run -p host-embed-wgpu --release` opens the window: HUD text
//! renders through the real Abel TTF face via the Slug lane (#259's
//! anti-aliased-text fix — this crate's default-on `slug` feature, see its
//! `Cargo.toml`). `cargo run -p host-embed-wgpu --release
//! --no-default-features` instead keeps every component on
//! `mkui_text::CompositeTextSystem`'s built-in bitmap face (the 5x7 cell,
//! scaled up — same as the pre-#259 standalone `BitmapTextSystem`).
//! `HEADLESS=1 cargo run -p host-embed-wgpu` exits before opening a window,
//! the same CI-smoke convention `native-window`/`native-showcase` use.
//!
//! ## Manual profiling
//!
//! This binary is #230's profiling target, not a profiler itself — run it
//! under the host platform's own GPU/CPU capture tool while it's open:
//!
//! - **macOS** — Instruments' Metal System Trace, or Xcode's GPU frame
//!   capture, attached to the running `host-embed-wgpu` process.
//! - **Windows** — PIX or RenderDoc, launching/attaching to the built
//!   `host-embed-wgpu.exe`.
//!
//! Press `Escape` to open the pause overlay (exercising the glass panel +
//! menu + settings + tooltip + dialog lanes together) and `Escape`/`Enter`
//! to navigate it — the richer, GPU-heavier frame #230 wants captured
//! alongside the base HUD's steady per-frame cost.
//!
//! ## Reading the profiling output (#263)
//!
//! Every frame prints one `frame_stats` line to stdout — no extra tooling
//! needed beyond a terminal capture — `grep frame_stats` it out of a log, or
//! parse the trailing `key=value` pairs directly:
//!
//! ```text
//! frame_stats cpu_update_ms=0.015 cpu_acquire_ms=0.003 cpu_render_ms=0.412 cpu_ui_ms=0.183 gpu_cube_ms=0.020 gpu_hud_ms=0.061 gpu_frame_ms=0.081 ui_gpu_mem_bytes=61440 host_gpu_mem_bytes=2764864
//! ```
//!
//! **GPU fields lag CPU/memory fields on the same line by one frame** (see
//! below) — a `frame_stats` line's `gpu_*` fields describe the *previous*
//! frame's GPU cost, not the frame whose `cpu_*`/`*_gpu_mem_bytes` fields
//! share that line. Close enough for a steady-state average; don't line up
//! one line's GPU number against that same line's CPU number expecting them
//! to be the same frame.
//!
//! **CPU fields — three different windows, not three parts of one sum:**
//!
//! - `cpu_update_ms` — building this frame's `Scene` (toast/tooltip state +
//!   `build_hud_scene`), no GPU work. Together with `cpu_ui_ms` below, this
//!   is the pair #230's "UI update + draw ≤1ms" budget is actually about.
//! - `cpu_acquire_ms` — `Gpu::render`'s `surface.get_current_texture()` call.
//!   Under `PresentMode::Fifo` this blocks until a drawable frees up, i.e.
//!   it mostly measures vsync wait (commonly ~16.7ms @ 60Hz), not this
//!   binary's own cost — logged separately so it never gets mistaken for
//!   CPU render work. An early version of this harness folded it into
//!   `cpu_render_ms` and every frame read close to one refresh interval
//!   regardless of actual cost; keep this field separate for exactly that
//!   reason.
//! - `cpu_render_ms` — CPU-side cost of recording and submitting this
//!   frame's commands (both render passes, the optional marker pass +
//!   query resolve/copy below) and the `queue.submit` call itself. Starts
//!   *after* `cpu_acquire_ms`'s wait and stops *before* `present` (which can
//!   itself block, e.g. on DXGI) and before the GPU-timestamp readback
//!   further down — none of those three are this binary's own CPU cost.
//! - `cpu_ui_ms` — the CPU-side subset of `cpu_render_ms` spent inside
//!   `EmbeddedRenderer::render_into` alone: tessellation, buffer uploads,
//!   command recording for mkui's HUD pass specifically. This *is* where
//!   `render_into`'s own re-tessellate/re-allocate-every-call cost (the
//!   embedded-path sibling of `Renderer::render`'s #230-flagged hotspot)
//!   shows up — this field is that hotspot made visible, not a stand-in
//!   for it.
//!
//! **GPU fields:**
//!
//! - `gpu_cube_ms` / `gpu_hud_ms` / `gpu_frame_ms` — GPU time for the host's
//!   3D pass, mkui's HUD pass, and `t0..t3` (first pass's start to second
//!   pass's end — not `gpu_cube_ms + gpu_hud_ms` summed, since the two
//!   passes aren't guaranteed back-to-back with zero gap between them),
//!   from `wgpu` timestamp queries (`wgpu::Features::TIMESTAMP_QUERY`).
//!   `gpu_cube_ms` brackets the cube pass this binary owns outright;
//!   `gpu_hud_ms` brackets mkui's HUD pass directly, via
//!   `EmbeddedRenderer::render_into_timed` (#264) — not an approximation.
//!   An earlier version of this harness used `render_into` plus a trailing
//!   no-op "marker" pass to approximate `gpu_hud_ms` indirectly; on real
//!   hardware that approximation's floor value shifted depending on which
//!   marker-pass timestamp index was used (a tell that it wasn't actually
//!   bracketing the HUD pass's own retirement), so `render_into_timed`
//!   replaced it with a real pass-level hook instead.
//!
//!   The query set is double-buffered (`GpuTiming`): one frame's timestamp
//!   writes land in one of two slots, and that slot is resolved + read back
//!   only on the *next* frame, never the same submission that wrote it —
//!   this is the one-frame lag mentioned above. Resolving a slot in the
//!   same submission that wrote it occasionally returned the *previous*
//!   frame's stale values on real Apple Silicon hardware, even after this
//!   binary's own blocking `wgpu::Device::poll` on that submission —
//!   visible as two or more consecutive lines printing byte-identical GPU
//!   fields. Deferring the resolve by a full frame fixed that empirically.
//!
//!   On an adapter that doesn't support `TIMESTAMP_QUERY`, all three print
//!   `gpu_frame_ms=unsupported` instead (no `gpu_cube_ms`/`gpu_hud_ms`
//!   fields at all) — permanent for the run, never a panic. They print
//!   `gpu_frame_ms=pending` the same way, but only transiently: the first
//!   frame (before double-buffering has anything to resolve yet) or a
//!   one-off poll/map failure. Any of the three can
//!   instead print `unreliable` — a defensive residual check
//!   (`u64::checked_sub`) for a stale/out-of-order pair slipping through
//!   despite the double-buffered resolve above; treat it as a dropped
//!   sample for that one frame, not a real negative-duration pass.
//! - `ui_gpu_mem_bytes` — mkui's own *per-frame transient* GPU allocations
//!   (not resident memory): `EmbeddedRenderer::last_frame_ui_vertex_bytes`
//!   (the triangle lane's vertex buffers) plus, under this crate's
//!   default-on `slug` feature, `EmbeddedRenderer::last_frame_slug_bytes`
//!   (the Slug lane's viewport + curve/band/index/glyph storage buffers,
//!   via `mkui_vector2d_wgpu::PreparedSlug::memory_bytes`). Every one of
//!   these buffers is freed at the end of its frame and a fresh set
//!   allocated the next — with `desired_maximum_frame_latency: 2`, two or
//!   three frames' worth can be transiently alive on the GPU at once, so
//!   actual peak VRAM pressure runs somewhat above a single frame's
//!   `ui_gpu_mem_bytes` reading. This is still the number #230's 64MB
//!   UI-memory ceiling is actually about.
//! - `host_gpu_mem_bytes` — this *host's* own static GPU allocations (cube
//!   vertex buffer, uniform buffer, depth texture) — logged separately so it
//!   never gets mistaken for UI cost; at 1080p the depth texture alone
//!   dwarfs a typical HUD's `ui_gpu_mem_bytes`. **Known gaps in both
//!   fields**: neither captures the swapchain/surface textures or any
//!   driver-level allocator overhead — measuring those precisely is out of
//!   #263's own scope.
//!
//! **Known limitation**: reading the GPU timestamps back blocks the CPU once
//! per frame (`wgpu::Device::poll` with `PollType::wait_indefinitely`), so
//! this binary's actual frame rate is lower than an uninstrumented build.
//! That's expected for a measurement harness, not a regression in the HUD's
//! rendered behavior — the cube/HUD animation is driven by wall-clock time,
//! not frame count, so it just runs at a lower FPS, never out of sync.
//! `HEADLESS=1` never reaches any of this code (see `main` below), so the
//! CI-smoke path pays none of it.

mod mat4;

use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

use mkui_core::headless::{KeyboardInteractable, MenuList};
use mkui_text::CompositeTextSystem;
use mkui_wgpu::components::{
    badge, counter_chip, dot, glass_panel, label, progress_bar, segmented_select,
    slider_with_label, titled_dialog, toast_stack, toggle, tooltip, tooltip_rect, TOOLTIP_GAP_PX,
};
use mkui_wgpu::{
    BadgeSize, BadgeVariant, Color, CornerRadii, DotAnimation, DotSize, DotVariant,
    EmbeddedColorSpace, EmbeddedRenderer, GlassMode, IconId, PanelLayout, Point, Quad, Rect, Scene,
    Size, TextVariant, ToastStack, ToastVariant, TooltipPlacement, TooltipState, WgpuTheme,
};

/// Abel (Google Fonts, SIL Open Font License 1.1) — embedded for this example
/// only, never the library (#259). Only referenced under `slug`; an
/// off-feature build never decodes it.
#[cfg(feature = "slug")]
const ABEL_TTF: &[u8] =
    include_bytes!("../../../crates/mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CubeVertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl CubeVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    mvp: [[f32; 4]; 4],
}

/// Fixed light direction for the host's per-face Lambertian shading
/// (`normalize([0.4, 0.8, 0.5])`, baked here to avoid a sqrt at startup).
const LIGHT_DIR: [f32; 3] = [0.379_008_25, 0.758_016_5, 0.473_760_3];

/// GPU timestamp query slots the frame-timing harness uses (#263): index 0/1
/// = start/end of the host's 3D pass; index 2/3 = start/end of mkui's HUD
/// pass, via `EmbeddedRenderer::render_into_timed` (#264) — no marker-pass
/// workaround needed, since that method brackets its own pass directly.
const TIMESTAMP_QUERY_COUNT: u32 = 4;

/// `(normal, base_color, 4 corners)` for one cube face.
type Face = ([f32; 3], [f32; 3], [[f32; 3]; 4]);

/// One entry per face of the cube. Each face is flat-shaded (one normal,
/// baked into the vertex color below) — a cube has no use for interpolated
/// per-vertex normals.
const FACES: [Face; 6] = [
    (
        [1.0, 0.0, 0.0],
        [0.85, 0.25, 0.25],
        [
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
        ],
    ),
    (
        [-1.0, 0.0, 0.0],
        [0.25, 0.85, 0.25],
        [
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, -0.5],
        ],
    ),
    (
        [0.0, 1.0, 0.0],
        [0.25, 0.25, 0.85],
        [
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        ],
    ),
    (
        [0.0, -1.0, 0.0],
        [0.85, 0.85, 0.25],
        [
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
        ],
    ),
    (
        [0.0, 0.0, 1.0],
        [0.85, 0.25, 0.85],
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
    ),
    (
        [0.0, 0.0, -1.0],
        [0.25, 0.85, 0.85],
        [
            [0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
        ],
    ),
];

/// Build the host's demo mesh: a unit cube, one baked Lambertian color per
/// face. Pure, no GPU — covered by the plain unit tests below.
fn build_cube_vertices() -> Vec<CubeVertex> {
    let mut vertices = Vec::with_capacity(FACES.len() * 6);
    for (normal, base_color, corners) in FACES {
        let diffuse =
            (normal[0] * LIGHT_DIR[0] + normal[1] * LIGHT_DIR[1] + normal[2] * LIGHT_DIR[2])
                .max(0.0);
        let factor = 0.3 + 0.7 * diffuse;
        let color = [
            base_color[0] * factor,
            base_color[1] * factor,
            base_color[2] * factor,
        ];
        for [a, b, c] in [[0usize, 1, 2], [0, 2, 3]] {
            for idx in [a, b, c] {
                vertices.push(CubeVertex {
                    position: corners[idx],
                    color,
                });
            }
        }
    }
    vertices
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("host-embed-wgpu Depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// `Depth32Float` is 4 bytes/texel, single sample, single mip, single layer
/// — the one per-resize host allocation the `gpu_mem_bytes` estimate (#263)
/// needs to recompute; the cube vertex buffer and uniform buffer are a fixed
/// size for the binary's lifetime.
fn depth_texture_bytes(width: u32, height: u32) -> u64 {
    u64::from(width.max(1)) * u64::from(height.max(1)) * 4
}

/// Convert a physical-pixel window size into the logical-pixel viewport the
/// HUD `Scene` carries — mirrors `mkui_wgpu::app`'s own
/// `logical_viewport_from_physical_size` (the host, not mkui, owns resize
/// here, but the unit convention is the same).
fn logical_viewport(size: winit::dpi::PhysicalSize<u32>, scale_factor: f64) -> Size {
    let scale = (scale_factor as f32).max(f32::EPSILON);
    Size::new(size.width as f32 / scale, size.height as f32 / scale)
}

/// GPU timestamp-query resources (#263), created only when the adapter
/// reports `wgpu::Features::TIMESTAMP_QUERY` support — absent (and GPU
/// timing degrades to logging `gpu_frame_ms=unsupported`) otherwise, per the
/// issue's "degrade gracefully... instead of panicking" scope bullet.
///
/// Double-buffered (two of everything, indexed by slot 0/1): a frame's
/// timestamp writes land in one slot, and that slot's resolve + readback is
/// deferred to the *next* frame's encoder/submission — deliberately never
/// the same submission that wrote it. On real Apple Silicon hardware,
/// resolving a slot in the same submission that wrote it occasionally
/// returned the *previous* frame's stale values even after this binary's
/// own blocking `Device::poll` on that submission (observed while building
/// this harness: two or more consecutive frames printing byte-identical GPU
/// fields). Giving the driver a full extra frame before resolving fixed it
/// empirically — see the module doc's "Reading the profiling output"
/// section for the user-visible effect (a one-frame lag).
struct GpuTiming {
    query_sets: [wgpu::QuerySet; 2],
    resolve_buffers: [wgpu::Buffer; 2],
    readback_buffers: [wgpu::Buffer; 2],
    /// The slot state machine itself — pulled out as [`SlotRing`] so its
    /// sequence is unit-testable without a GPU (#263 review).
    slots: SlotRing,
}

/// The double-buffered slot state machine `GpuTiming` needs (#263 review) —
/// its own type purely so [`advance`](Self::advance)'s sequence is
/// unit-testable without a GPU. See `GpuTiming`'s doc for why
/// double-buffering exists at all.
struct SlotRing {
    /// Slot the *next* [`advance`](Self::advance) call writes into.
    next_write: usize,
    /// Slot a prior call wrote, now ready to resolve + read — `None` only
    /// before the first [`advance`](Self::advance) call (nothing written
    /// yet). A surface-acquire failure in `Gpu::render` returns before
    /// `advance` is ever called for that frame, so a skipped frame leaves
    /// this exactly as it was.
    pending: Option<usize>,
}

impl SlotRing {
    fn new() -> Self {
        Self {
            next_write: 0,
            pending: None,
        }
    }

    /// Advance one frame: returns `(write_slot, resolve_slot)` — the slot
    /// this frame should write its timestamps into, and the slot (if any)
    /// a PRIOR call wrote that's now ready to resolve + read. Call exactly
    /// once per frame that actually submits (never for a skipped frame).
    fn advance(&mut self) -> (usize, Option<usize>) {
        let write_slot = self.next_write;
        let resolve_slot = self.pending;
        self.next_write = 1 - write_slot;
        self.pending = Some(write_slot);
        (write_slot, resolve_slot)
    }
}

/// Build one `GpuTiming` slot's three GPU resources — called twice (once
/// per slot) by `Gpu::new`, never per-frame.
fn make_gpu_timing_slot(device: &wgpu::Device) -> (wgpu::QuerySet, wgpu::Buffer, wgpu::Buffer) {
    let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("host-embed-wgpu GPU Timing Queries"),
        ty: wgpu::QueryType::Timestamp,
        count: TIMESTAMP_QUERY_COUNT,
    });
    let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("host-embed-wgpu GPU Timing Resolve"),
        size: u64::from(TIMESTAMP_QUERY_COUNT) * 8,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("host-embed-wgpu GPU Timing Readback"),
        size: u64::from(TIMESTAMP_QUERY_COUNT) * 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (query_set, resolve_buffer, readback_buffer)
}

/// GPU-side timestamp deltas for one (prior, one-frame-lagged — see
/// `GpuTiming`'s doc) frame, converted to milliseconds via
/// `Queue::get_timestamp_period`. See the module doc's "Reading the
/// profiling output" section for what `hud_ms` approximates and why, and
/// for why each delta is `Option`, not a raw `f64` — `checked_sub` rather
/// than a plain subtraction is kept as a defensive residual safety net
/// against a stale/out-of-order pair slipping through despite the
/// double-buffered resolve `GpuTiming` now does; a plain subtraction would
/// silently wrap a negative delta into a bogus multi-trillion-millisecond
/// value instead of `None`.
struct GpuFrameTimings {
    cube_ms: Option<f64>,
    hud_ms: Option<f64>,
    total_ms: Option<f64>,
}

/// GPU timing status for one frame (#263) — three states, not just
/// "supported or not". See the module doc's "Reading the profiling output"
/// section for when each applies and how each prints.
enum GpuTimingStatus {
    /// The adapter doesn't support `wgpu::Features::TIMESTAMP_QUERY` — no
    /// `GpuTiming` was ever created; permanent for the run.
    Unsupported,
    /// `GpuTiming` exists, but no resolved sample is available for this
    /// frame yet — the first frame only (double-buffering lags GPU timing
    /// by one frame, so frame 0 has nothing to resolve), or a one-off
    /// poll/map failure.
    Pending,
    /// A resolved `GpuFrameTimings` triple for some prior frame.
    Timings(GpuFrameTimings),
}

/// One frame's CPU/GPU timing + GPU-memory estimate (#263), returned by
/// `Gpu::render` for `HostApp` to log. `cpu_acquire_ms`, `cpu_render_ms`,
/// and `cpu_ui_ms` are three different windows, not three parts of one sum
/// — see the module doc's "Reading the profiling output" section for
/// exactly what each brackets.
struct FrameStats {
    cpu_acquire_ms: f64,
    cpu_render_ms: f64,
    cpu_ui_ms: f64,
    gpu: GpuTimingStatus,
    ui_gpu_mem_bytes: u64,
    host_gpu_mem_bytes: u64,
}

/// Everything the host owns: its own instance-derived device/queue/surface,
/// its own lit 3D pipeline, and the one `EmbeddedRenderer` borrowing that
/// device + the surface's format for mkui's HUD pass.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    cube_pipeline: wgpu::RenderPipeline,
    cube_vertex_buffer: wgpu::Buffer,
    cube_vertex_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    embedded: EmbeddedRenderer,
    /// `None` on an adapter without `wgpu::Features::TIMESTAMP_QUERY`.
    gpu_timing: Option<GpuTiming>,
    timestamp_period_ns: f32,
    /// Sum of this host's own *static* GPU allocations (cube vertex buffer +
    /// uniform buffer + depth texture) — recomputed on resize, since the
    /// depth texture is the only one of the three whose size depends on the
    /// window. Added to `EmbeddedRenderer::last_frame_ui_vertex_bytes` each
    /// frame for the `gpu_mem_bytes` figure `FrameStats` carries.
    static_gpu_memory_bytes: u64,
}

impl Gpu {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("host-embed-wgpu: create_surface failed");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("host-embed-wgpu: request_adapter failed");

        // #263: only request `TIMESTAMP_QUERY` when the adapter actually
        // advertises it — requesting an unsupported feature would fail
        // `request_device` outright, and the issue's own scope requires
        // degrading gracefully (a logged "unsupported" marker) rather than
        // panicking on an adapter that lacks it.
        let gpu_timing_supported = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let required_features = if gpu_timing_supported {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("host-embed-wgpu Device"),
                required_features,
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("host-embed-wgpu: request_device failed");
        let timestamp_period_ns = queue.get_timestamp_period();
        let gpu_timing = gpu_timing_supported.then(|| {
            let (query_set_0, resolve_buffer_0, readback_buffer_0) = make_gpu_timing_slot(&device);
            let (query_set_1, resolve_buffer_1, readback_buffer_1) = make_gpu_timing_slot(&device);
            GpuTiming {
                query_sets: [query_set_0, query_set_1],
                resolve_buffers: [resolve_buffer_0, resolve_buffer_1],
                readback_buffers: [readback_buffer_0, readback_buffer_1],
                slots: SlotRing::new(),
            }
        });

        let capabilities = surface.get_capabilities(&adapter);
        let default_format = capabilities
            .formats
            .first()
            .copied()
            .expect("host-embed-wgpu: surface advertised no supported formats");
        // Prefer an sRGB surface format — the common host configuration, and
        // the one `EmbeddedRenderer` draws through its physically-linear
        // fragment entry point for (see ADR 0012 §"Color space").
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(default_format);
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .or_else(|| capabilities.present_modes.first().copied())
            .expect("host-embed-wgpu: surface advertised no present modes");
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .expect("host-embed-wgpu: surface advertised no alpha modes");

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, width, height);

        let cube_vertices = build_cube_vertices();
        let cube_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("host-embed-wgpu Cube Vertices"),
            contents: bytemuck::cast_slice(&cube_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("host-embed-wgpu Cube Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("host-embed-wgpu Cube Bind Group Layout"),
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
            label: Some("host-embed-wgpu Cube Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("host-embed-wgpu Cube Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cube.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("host-embed-wgpu Cube Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("host-embed-wgpu Cube Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_cube"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(CubeVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_cube"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // The one borrow: mkui never sees the instance, the adapter, or the
        // surface above — only the device it already negotiated and the
        // format it already picked for its own frame target (#175). `format`
        // was chosen above to prefer an sRGB surface format, so this host
        // declares `Linear` explicitly — never infer this from `format`
        // alone; see `EmbeddedColorSpace`'s docs for why a format-based
        // heuristic is unsound for e.g. a linear HDR intermediate.
        let embedded = EmbeddedRenderer::new(&device, format, EmbeddedColorSpace::Linear);

        let static_gpu_memory_bytes = (cube_vertices.len() * std::mem::size_of::<CubeVertex>())
            as u64
            + std::mem::size_of::<Uniforms>() as u64
            + depth_texture_bytes(width, height);

        Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            cube_pipeline,
            cube_vertex_buffer,
            cube_vertex_count: cube_vertices.len() as u32,
            uniform_buffer,
            uniform_bind_group,
            embedded,
            gpu_timing,
            timestamp_period_ns,
            static_gpu_memory_bytes,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
        self.static_gpu_memory_bytes = u64::from(self.cube_vertex_count)
            * std::mem::size_of::<CubeVertex>() as u64
            + std::mem::size_of::<Uniforms>() as u64
            + depth_texture_bytes(width, height);
    }

    /// Record and submit exactly one frame: the host's lit 3D pass, then
    /// mkui's HUD pass into the same encoder and the same swapchain view,
    /// then one `queue.submit` and present (#175's acceptance bar). Returns
    /// `None` when no frame was actually drawn this call (surface
    /// timeout/occlusion, or an outdated/lost surface being reconfigured) —
    /// `HostApp` skips logging in that case, same as it always skipped
    /// drawing. See the module doc's "Reading the profiling output" section
    /// (#263) for `FrameStats`' field semantics.
    fn render(
        &mut self,
        elapsed_secs: f32,
        hud_scene: &Scene,
        text_system: &CompositeTextSystem,
    ) -> Option<FrameStats> {
        // #263: timed separately from `cpu_render_ms` below — under
        // `PresentMode::Fifo` this acquire blocks until a drawable frees up,
        // i.e. it mostly measures vsync wait, not this binary's own CPU
        // cost. Folding it into `cpu_render_ms` would make every frame read
        // close to one refresh interval (~16.7 ms @ 60 Hz) regardless of how
        // cheap the actual frame was — exactly the mismeasurement an early
        // version of this harness shipped with.
        let acquire_start = Instant::now();
        let (frame, reconfigure_after_present) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return None,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return None;
            }
        };
        let cpu_acquire_ms = acquire_start.elapsed().as_secs_f64() * 1000.0;
        let render_start = Instant::now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let model = mat4::rotation_y(elapsed_secs * 0.6);
        let eye = mat4::look_at_rh([2.2, 1.8, 2.6], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let aspect = self.config.width as f32 / (self.config.height.max(1) as f32);
        let proj = mat4::perspective(60f32.to_radians(), aspect, 0.1, 100.0);
        let mvp = mat4::mul(proj, mat4::mul(eye, model));
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[Uniforms { mvp }]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("host-embed-wgpu Frame Encoder"),
            });

        // #263: double-buffered query-set slot selection — see `GpuTiming`'s
        // doc for why. This frame's timestamp writes land in `write_slot`;
        // `resolve_slot`, if present, is the slot a PRIOR frame wrote,
        // resolved+read below in THIS frame's submission — deliberately a
        // different one from the submission that wrote it. Placed after the
        // surface-acquire early-return points above, so a skipped frame
        // never advances the slot toggle or marks an unwritten slot pending.
        let (write_slot, resolve_slot) = match &mut self.gpu_timing {
            Some(timing) => {
                let (write_slot, resolve_slot) = timing.slots.advance();
                (Some(write_slot), resolve_slot)
            }
            None => (None, None),
        };

        // Pass 1: the host's own lit 3D scene, writing into the swapchain
        // view. Timestamp indices 0/1 (#263) bracket just this pass, into
        // `write_slot`'s query set, when the adapter supports
        // `TIMESTAMP_QUERY`.
        {
            let cube_timestamp_writes = write_slot.map(|slot| wgpu::RenderPassTimestampWrites {
                query_set: &self.gpu_timing.as_ref().unwrap().query_sets[slot],
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("host-embed-wgpu 3D Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.06,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: cube_timestamp_writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.cube_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, self.cube_vertex_buffer.slice(..));
            pass.draw(0..self.cube_vertex_count, 0..1);
        }

        // Pass 2: mkui's HUD, recorded into the SAME encoder and the SAME
        // view, after the host's 3D scene and before present — the
        // embedding contract #175 exists for. `render_into_timed` never
        // clears (`wgpu::LoadOp::Load`), never submits, and never presents.
        // Timestamp indices 2/3 (#264) bracket this pass directly — no
        // marker-pass workaround needed, unlike the cube pass's own
        // indices 0/1 bracketing a pass this binary owns outright.
        // `cpu_ui_ms` (#263) is the CPU-side cost of exactly this call —
        // tessellation, buffer uploads, command recording — the number
        // #230's "UI update + draw ≤1ms" budget is actually about (combined
        // with `cpu_update_ms`'s scene-build cost from the caller).
        let hud_timestamp_writes = write_slot.map(|slot| wgpu::RenderPassTimestampWrites {
            query_set: &self.gpu_timing.as_ref().unwrap().query_sets[slot],
            beginning_of_pass_write_index: Some(2),
            end_of_pass_write_index: Some(3),
        });
        let ui_start = Instant::now();
        self.embedded.render_into_timed(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            (self.config.width, self.config.height),
            hud_scene,
            text_system,
            hud_timestamp_writes,
        );
        let cpu_ui_ms = ui_start.elapsed().as_secs_f64() * 1000.0;
        let ui_vertex_bytes = self.embedded.last_frame_ui_vertex_bytes() as u64;
        #[cfg(feature = "slug")]
        let ui_gpu_mem_bytes = ui_vertex_bytes + self.embedded.last_frame_slug_bytes() as u64;
        #[cfg(not(feature = "slug"))]
        let ui_gpu_mem_bytes = ui_vertex_bytes;

        // Resolve + read back `resolve_slot` — the slot a PRIOR frame
        // wrote, not this frame's `write_slot` — into THIS frame's
        // submission. See `GpuTiming`'s doc for why that deliberate
        // one-frame separation between writing a slot and resolving it
        // matters on real hardware.
        if let (Some(timing), Some(slot)) = (&self.gpu_timing, resolve_slot) {
            encoder.resolve_query_set(
                &timing.query_sets[slot],
                0..TIMESTAMP_QUERY_COUNT,
                &timing.resolve_buffers[slot],
                0,
            );
            encoder.copy_buffer_to_buffer(
                &timing.resolve_buffers[slot],
                0,
                &timing.readback_buffers[slot],
                0,
                u64::from(TIMESTAMP_QUERY_COUNT) * 8,
            );
        }

        // ONE submission, carrying both render passes (plus the optional
        // timing marker pass + query resolve above). `cpu_render_ms` stops
        // here, *before* `present` — present can itself block (e.g. on
        // DXGI) the same way the surface acquire above can, and that's not
        // this binary's own CPU cost either.
        self.queue.submit(Some(encoder.finish()));
        let cpu_render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
        drop(view);
        self.queue.present(frame);
        if reconfigure_after_present {
            self.surface.configure(&self.device, &self.config);
        }

        let gpu = match (&self.gpu_timing, resolve_slot) {
            (None, _) => GpuTimingStatus::Unsupported,
            (Some(_), None) => GpuTimingStatus::Pending,
            (Some(_), Some(slot)) => self
                .read_gpu_timestamps(slot)
                .map_or(GpuTimingStatus::Pending, GpuTimingStatus::Timings),
        };

        Some(FrameStats {
            cpu_acquire_ms,
            cpu_render_ms,
            cpu_ui_ms,
            gpu,
            ui_gpu_mem_bytes,
            host_gpu_mem_bytes: self.static_gpu_memory_bytes,
        })
    }

    /// Block until the GPU finishes the frame carrying `slot`'s resolve +
    /// copy (submitted by this same `render` call, for a slot a PRIOR frame
    /// wrote — see `GpuTiming`'s doc), then read back its three
    /// timestamp-query results and convert them to millisecond deltas.
    /// `None` when `self.gpu_timing` is `None` (defensive — every call site
    /// already checks this) or on a one-off poll/map failure; either way
    /// the caller logs `GpuTimingStatus::Pending` for this frame, not a
    /// panic. See the module doc's "Reading the profiling output" section
    /// for the known limitation this blocking sync introduces: every CPU
    /// timing field above is already captured before this call runs, so
    /// none of them are affected by it.
    fn read_gpu_timestamps(&self, slot: usize) -> Option<GpuFrameTimings> {
        let timing = self.gpu_timing.as_ref()?;
        let slice = timing.readback_buffers[slot].slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let timestamps: &[u64] = bytemuck::cast_slice(&mapped);
        let (t0, t1, t2, t3) = (timestamps[0], timestamps[1], timestamps[2], timestamps[3]);
        drop(mapped);
        timing.readback_buffers[slot].unmap();
        Some(gpu_frame_timings(t0, t1, t2, t3, self.timestamp_period_ns))
    }
}

/// Pure core of [`Gpu::read_gpu_timestamps`]: converts four raw GPU tick
/// values — cube-pass begin/end (`t0`/`t1`), HUD-pass begin/end
/// (`t2`/`t3`), each pass bracketed directly (#264) — into millisecond
/// deltas. Split out from the GPU-buffer-mapping code above so it's
/// testable without a GPU. `total_ms` is `t0..t3` (first begin to last
/// end), not `cube_ms + hud_ms` summed — the two passes aren't guaranteed
/// back-to-back with no gap, so don't expect them to sum exactly. See
/// `GpuFrameTimings`' doc for why an out-of-order pair becomes `None`
/// rather than a wrapped value.
fn gpu_frame_timings(t0: u64, t1: u64, t2: u64, t3: u64, period_ns: f32) -> GpuFrameTimings {
    let period_ns = f64::from(period_ns);
    let delta_ms = |from: u64, to: u64| {
        to.checked_sub(from)
            .map(|ticks| ticks as f64 * period_ns / 1_000_000.0)
    };
    GpuFrameTimings {
        cube_ms: delta_ms(t0, t1),
        hud_ms: delta_ms(t2, t3),
        total_ms: delta_ms(t0, t3),
    }
}

/// Render one GPU timing field as `{value:.3}`, or `unreliable` for the
/// out-of-order-timestamp-pair case `GpuFrameTimings`' doc describes.
fn fmt_gpu_ms(value: Option<f64>) -> String {
    match value {
        Some(ms) => format!("{ms:.3}"),
        None => "unreliable".to_string(),
    }
}

/// Build one `frame_stats` line (#263) — see the module doc's "Reading the
/// profiling output" section for the format and what each field means. Pure
/// (no I/O), so the `key=value` contract is testable without a GPU or a
/// captured stdout; [`log_frame_stats`] just prints what this returns.
fn format_frame_stats(cpu_update_ms: f64, stats: &FrameStats) -> String {
    match &stats.gpu {
        GpuTimingStatus::Timings(gpu) => format!(
            "frame_stats cpu_update_ms={cpu_update_ms:.3} cpu_acquire_ms={:.3} cpu_render_ms={:.3} cpu_ui_ms={:.3} gpu_cube_ms={} gpu_hud_ms={} gpu_frame_ms={} ui_gpu_mem_bytes={} host_gpu_mem_bytes={}",
            stats.cpu_acquire_ms,
            stats.cpu_render_ms,
            stats.cpu_ui_ms,
            fmt_gpu_ms(gpu.cube_ms),
            fmt_gpu_ms(gpu.hud_ms),
            fmt_gpu_ms(gpu.total_ms),
            stats.ui_gpu_mem_bytes,
            stats.host_gpu_mem_bytes
        ),
        GpuTimingStatus::Pending => format!(
            "frame_stats cpu_update_ms={cpu_update_ms:.3} cpu_acquire_ms={:.3} cpu_render_ms={:.3} cpu_ui_ms={:.3} gpu_frame_ms=pending ui_gpu_mem_bytes={} host_gpu_mem_bytes={}",
            stats.cpu_acquire_ms,
            stats.cpu_render_ms,
            stats.cpu_ui_ms,
            stats.ui_gpu_mem_bytes,
            stats.host_gpu_mem_bytes
        ),
        GpuTimingStatus::Unsupported => format!(
            "frame_stats cpu_update_ms={cpu_update_ms:.3} cpu_acquire_ms={:.3} cpu_render_ms={:.3} cpu_ui_ms={:.3} gpu_frame_ms=unsupported ui_gpu_mem_bytes={} host_gpu_mem_bytes={}",
            stats.cpu_acquire_ms,
            stats.cpu_render_ms,
            stats.cpu_ui_ms,
            stats.ui_gpu_mem_bytes,
            stats.host_gpu_mem_bytes
        ),
    }
}

/// Print one `frame_stats` line (#263) — see [`format_frame_stats`].
fn log_frame_stats(cpu_update_ms: f64, stats: &FrameStats) {
    println!("{}", format_frame_stats(cpu_update_ms, stats));
}

/// Whether the pause/settings overlay is hidden, open, or showing its
/// nested "quit to desktop?" confirmation. `QuitConfirm` stacks a second
/// [`titled_dialog`] *on top of* the already-drawn overlay — stacking falls
/// out of call order, same as [`mkui_wgpu::components::dialog`]'s own doc
/// comment describes — rather than replacing it, so Escape from
/// `QuitConfirm` can cleanly fall back to `Open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayState {
    Closed,
    Open,
    QuitConfirm,
}

/// Fresh pause menu, pre-selecting "Resume" (`MenuList::builder`'s default
/// first-item selection) — rebuilt every time the overlay transitions from
/// `Closed` to `Open` so a reopened pause menu never remembers a stale
/// selection from the last time it was open.
fn build_pause_menu() -> MenuList {
    MenuList::builder().items(["Resume", "Quit"]).build()
}

/// Every piece of per-frame HUD state the host owns across frames — the
/// renderer is immediate-mode and retains no component tree of its own
/// (see `counter_chip`'s module doc), so whatever the scene should show
/// next frame has to live here, not in the `Scene` built from it.
struct HudState {
    overlay: OverlayState,
    menu: MenuList,
    /// Toggled by `M` while the overlay is open — the one genuinely
    /// keyboard-interactive settings control; `volume`/`quality` below are
    /// rendered but held static, satisfying the issue's "at least one"
    /// toggle/slider/segmented-select bar without inventing extra input
    /// plumbing the issue doesn't ask for.
    mute: bool,
    volume: f32,
    quality: usize,
    tooltip: TooltipState,
    toasts: ToastStack,
}

impl HudState {
    fn new() -> Self {
        Self {
            overlay: OverlayState::Closed,
            menu: build_pause_menu(),
            mute: false,
            volume: 0.65,
            quality: 1,
            tooltip: TooltipState::new(),
            toasts: ToastStack::new(),
        }
    }
}

/// Toast/banner firing cadence (#255's "firing periodically" acceptance
/// bullet) and the small rotation of messages it cycles through.
const TOAST_INTERVAL_SECS: f32 = 4.0;
const TOAST_MESSAGES: [(ToastVariant, &str); 3] = [
    (ToastVariant::Info, "Checkpoint saved"),
    (ToastVariant::Success, "Objective complete"),
    (ToastVariant::Warning, "Low ammo"),
];

/// Push the next toast once `elapsed_secs` has advanced `TOAST_INTERVAL_SECS`
/// past `*last_toast_at`, cycling through `TOAST_MESSAGES`. Pure state
/// mutation, no `Scene` — kept separate from rendering so the firing
/// cadence is testable without a GPU.
fn maybe_fire_toast(
    toasts: &mut ToastStack,
    last_toast_at: &mut f32,
    cycle: &mut usize,
    elapsed_secs: f32,
) {
    if elapsed_secs - *last_toast_at < TOAST_INTERVAL_SECS {
        return;
    }
    *last_toast_at = elapsed_secs;
    let (variant, message) = TOAST_MESSAGES[*cycle % TOAST_MESSAGES.len()];
    *cycle += 1;
    toasts.push(variant, message);
}

/// The always-on-screen base HUD: health `progress_bar`, score
/// `counter_chip`, a "LIVE" `badge` + status `dot`, and the periodically
/// firing `toast_stack` — present every frame regardless of the pause
/// overlay, so #230 always has a believable minimum scene to profile.
fn draw_base_hud(
    scene: &mut Scene,
    viewport: Size,
    elapsed_secs: f32,
    hud: &HudState,
    theme: &WgpuTheme,
) {
    // Health bar — a slow bob so the base HUD isn't perfectly static under
    // profiling, same spirit as the host's own rotating cube.
    let health_fraction = 0.55 + 0.1 * (elapsed_secs * 0.5).sin();
    let bar_rect = Rect::new(Point::new(24.0, 24.0), Size::new(220.0, 18.0));
    progress_bar(scene, bar_rect, health_fraction, &theme.progress_bar());

    let chip_rect = Rect::new(
        Point::new(bar_rect.width_end() + 12.0, 20.0),
        Size::new(96.0, 26.0),
    );
    counter_chip(
        scene,
        chip_rect,
        IconId(1),
        2480,
        "Score",
        false,
        false,
        &theme.counter_chip(),
    );

    let dot_point = Point::new(viewport.width - 96.0, 36.0);
    dot(
        scene,
        dot_point,
        DotVariant::Ok,
        DotSize::Sm,
        true,
        DotAnimation::Pulse,
        theme,
    );
    let status_badge_rect = Rect::new(
        Point::new(viewport.width - 80.0, 24.0),
        Size::new(56.0, 24.0),
    );
    badge(
        scene,
        status_badge_rect,
        "LIVE",
        BadgeVariant::Secondary,
        BadgeSize::Sm,
        theme,
    );

    let toast_region = Rect::new(
        Point::new(
            (viewport.width - 260.0).max(0.0),
            (viewport.height - 200.0).max(0.0),
        ),
        Size::new(240.0, 180.0),
    );
    let mut layout = PanelLayout::<()>::default();
    toast_stack(scene, &mut layout, toast_region, &hud.toasts, theme, |_| ());
}

/// The pause menu's rows: a highlight behind [`MenuList::is_selected`]'s
/// current row, then each item's label — [`MenuList`] itself carries no
/// wgpu rendering (#255 only assembles existing batch-15 render
/// components; `MenuList` isn't one), so this is the thin per-row paint
/// loop a host wires up itself.
fn render_menu(scene: &mut Scene, rect: Rect, menu: &MenuList, theme: &WgpuTheme) {
    const ROW_HEIGHT: f32 = 36.0;
    const ROW_GAP: f32 = 8.0;

    for (index, item) in menu.items().iter().enumerate() {
        let row_rect = Rect::new(
            Point::new(
                rect.origin.x,
                rect.origin.y + index as f32 * (ROW_HEIGHT + ROW_GAP),
            ),
            Size::new(rect.size.width, ROW_HEIGHT),
        );
        if menu.is_selected(index) {
            scene.quad(Quad {
                rect: row_rect,
                fill: Color::rgba(0.4, 0.6, 0.9, 0.35),
                corner_radii: CornerRadii::all(8.0),
                stroke: None,
            });
        }
        let label_style = theme.body_style();
        let label_rect = Rect::new(
            Point::new(
                row_rect.origin.x + 12.0,
                row_rect.origin.y + (row_rect.size.height - label_style.line_height_px) * 0.5,
            ),
            Size::new(
                (row_rect.size.width - 24.0).max(0.0),
                label_style.line_height_px,
            ),
        );
        label(scene, label_rect, item.label(), label_style);
    }
}

/// The settings column: an audio status `dot` + mute `toggle`, a volume
/// `slider_with_label`, and a quality `segmented_select` + `badge` — plus a
/// keyboard-focus-driven `tooltip` anchored above the quality control.
fn render_settings(scene: &mut Scene, rect: Rect, hud: &HudState, theme: &WgpuTheme) {
    let mut y = rect.origin.y;

    let dot_point = Point::new(rect.origin.x + 6.0, y + 10.0);
    dot(
        scene,
        dot_point,
        if hud.mute {
            DotVariant::Warn
        } else {
            DotVariant::Ok
        },
        DotSize::Md,
        false,
        DotAnimation::None,
        theme,
    );
    label(
        scene,
        Rect::new(Point::new(rect.origin.x + 20.0, y), Size::new(80.0, 20.0)),
        "Audio (M)",
        theme.body_style(),
    );
    let toggle_rect = Rect::new(
        Point::new(rect.width_end() - 44.0, y - 2.0),
        Size::new(44.0, 24.0),
    );
    toggle(scene, toggle_rect, !hud.mute, &theme.toggle());
    y += 20.0 + 16.0;

    label(
        scene,
        Rect::new(
            Point::new(rect.origin.x, y),
            Size::new(rect.size.width, 18.0),
        ),
        "Volume",
        theme.text_style(TextVariant::Muted),
    );
    y += 20.0;
    let slider_rect = Rect::new(
        Point::new(rect.origin.x, y),
        Size::new(rect.size.width, 8.0),
    );
    let volume_label = format!("{:.0}%", hud.volume * 100.0);
    slider_with_label(
        scene,
        slider_rect,
        hud.volume,
        Some(&volume_label),
        &theme.slider(),
    );
    y += 36.0 + 16.0;

    label(
        scene,
        Rect::new(
            Point::new(rect.origin.x, y),
            Size::new(rect.size.width, 18.0),
        ),
        "Quality",
        theme.text_style(TextVariant::Muted),
    );
    y += 20.0;
    const QUALITY_LABELS: [&str; 3] = ["Low", "Medium", "High"];
    let select_rect = Rect::new(
        Point::new(rect.origin.x, y),
        Size::new((rect.size.width - 64.0).max(0.0), 28.0),
    );
    let mut layout = PanelLayout::<()>::default();
    segmented_select(
        scene,
        &mut layout,
        select_rect,
        &QUALITY_LABELS,
        hud.quality,
        &theme.segmented_select(),
        |_| (),
    );
    let quality_badge_rect = Rect::new(
        Point::new(select_rect.width_end() + 8.0, y + 2.0),
        Size::new(56.0, 24.0),
    );
    badge(
        scene,
        quality_badge_rect,
        QUALITY_LABELS[hud.quality].to_uppercase(),
        BadgeVariant::Secondary,
        BadgeSize::Sm,
        theme,
    );

    // Tooltip last, per its own doc comment, so the bubble always draws
    // over everything else this frame — visible on keyboard *focus*
    // (`TooltipState::set_focused`, driven by the overlay being open),
    // not hover; this host has no pointer at all.
    if hud.tooltip.is_visible() {
        let bubble_size = Size::new(220.0, 36.0);
        let bubble_rect = tooltip_rect(
            select_rect,
            bubble_size,
            TooltipPlacement::Top,
            TOOLTIP_GAP_PX,
            scene.viewport,
        );
        tooltip(scene, bubble_rect, "Lower quality reduces GPU load.", theme);
    }
}

/// The pause/settings overlay's outer chrome (a `glass_panel`) plus its
/// menu + settings columns, and — only in [`OverlayState::QuitConfirm`] —
/// a second `titled_dialog` stacked on top asking to confirm quitting.
///
/// `GlassMode::Solid`, not `Blur`: [`EmbeddedRenderer::render_into`]
/// doesn't wire up the `Primitive::Backdrop` blur lane yet (#175's
/// `render/embedded.rs` module doc — tracked as its own #187 follow-up), so
/// `Blur` would paint a `Color::TRANSPARENT`-filled panel here, i.e. an
/// outline with no visible chrome at all. `Solid` is the documented
/// fallback for exactly this case — "no blur cost," by construction.
fn draw_pause_overlay(scene: &mut Scene, viewport: Size, hud: &HudState, theme: &WgpuTheme) {
    let panel_rect = Rect::new(
        Point::new(
            (viewport.width - 560.0).max(0.0) * 0.5,
            (viewport.height - 420.0).max(0.0) * 0.5,
        ),
        Size::new(
            560.0_f32.min(viewport.width),
            420.0_f32.min(viewport.height),
        ),
    );
    let content = glass_panel(
        scene,
        panel_rect,
        theme.glass_panel_style(),
        GlassMode::Solid,
    );

    let title_style = theme.title_style();
    label(
        scene,
        Rect::new(
            content.origin,
            Size::new(content.size.width, title_style.line_height_px),
        ),
        "Paused",
        title_style,
    );
    let body = Rect::new(
        Point::new(
            content.origin.x,
            content.origin.y + title_style.line_height_px + 12.0,
        ),
        Size::new(
            content.size.width,
            (content.size.height - title_style.line_height_px - 12.0).max(0.0),
        ),
    );
    const MENU_WIDTH: f32 = 160.0;
    const COLUMN_GAP: f32 = 24.0;
    let menu_rect = Rect::new(body.origin, Size::new(MENU_WIDTH, body.size.height));
    let settings_rect = Rect::new(
        Point::new(body.origin.x + MENU_WIDTH + COLUMN_GAP, body.origin.y),
        Size::new(
            (body.size.width - MENU_WIDTH - COLUMN_GAP).max(0.0),
            body.size.height,
        ),
    );

    render_menu(scene, menu_rect, &hud.menu, theme);
    render_settings(scene, settings_rect, hud, theme);

    if hud.overlay == OverlayState::QuitConfirm {
        let confirm_rect = Rect::new(
            Point::new(viewport.width * 0.5 - 160.0, viewport.height * 0.5 - 70.0),
            Size::new(320.0, 140.0),
        );
        let confirm_content = titled_dialog(
            scene,
            confirm_rect,
            "Quit to desktop?",
            theme.dialog(),
            theme.title_style(),
        );
        label(
            scene,
            confirm_content,
            "Enter to confirm, Esc to cancel",
            theme.text_style(TextVariant::Muted),
        );
    }
}

/// Build this frame's full HUD scene: the always-on base HUD, plus the
/// pause overlay (and its nested quit-confirm dialog) whenever
/// `hud.overlay` isn't [`OverlayState::Closed`]. Rebuilt fresh every frame
/// — like every other `Scene` producer in this crate, nothing here is
/// retained between frames; `hud` (owned by [`HostApp`] across frames) is
/// what makes two consecutive calls differ.
fn build_hud_scene(viewport: Size, elapsed_secs: f32, hud: &HudState, theme: &WgpuTheme) -> Scene {
    let mut scene = Scene::new(viewport);
    draw_base_hud(&mut scene, viewport, elapsed_secs, hud, theme);
    if hud.overlay != OverlayState::Closed {
        draw_pause_overlay(&mut scene, viewport, hud, theme);
    }
    scene
}

struct HostApp {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    viewport: Size,
    theme: WgpuTheme,
    hud: HudState,
    // Real anti-aliased TTF text via the Slug lane (#259), gated on this
    // crate's own `slug` feature (default ON — see its `Cargo.toml`): a
    // composite registry seeded with the built-in bitmap face plus the Abel
    // SFNT face decoded from `ABEL_TTF`, routed through
    // `EmbeddedRenderer::set_slug_font` in `resumed` below. Built with
    // `--no-default-features`, Abel is never registered at all and every
    // component stays on `CompositeTextSystem`'s own built-in bitmap face —
    // byte-identical to the pre-#259 standalone `BitmapTextSystem` path.
    text_system: CompositeTextSystem,
    #[cfg(feature = "slug")]
    abel_font_id: mkui_text::FontId,
    start: Instant,
    last_toast_at: f32,
    toast_cycle: usize,
}

impl HostApp {
    fn new() -> Self {
        #[cfg_attr(not(feature = "slug"), allow(unused_mut))]
        let mut text_system = CompositeTextSystem::new();
        #[cfg(feature = "slug")]
        let abel_font_id = text_system
            .register_sfnt_face(
                std::sync::Arc::from(ABEL_TTF.to_vec().into_boxed_slice()),
                0,
            )
            .expect("Abel registers");
        Self {
            window: None,
            gpu: None,
            viewport: Size::new(960.0, 720.0),
            theme: WgpuTheme::default(),
            hud: HudState::new(),
            text_system,
            #[cfg(feature = "slug")]
            abel_font_id,
            start: Instant::now(),
            // Negative, not zero: the first `RedrawRequested` (elapsed ≈ 0)
            // must already satisfy `elapsed - last_toast_at >=
            // TOAST_INTERVAL_SECS`, so the first toast fires promptly
            // rather than only after one full interval of silence.
            last_toast_at: -TOAST_INTERVAL_SECS,
            toast_cycle: 0,
        }
    }

    /// Host-owned keyboard routing (ADR 0012: the host, not mkui, owns
    /// input here) — no backend maps winit's `Key`/`PhysicalKey` into
    /// [`mkui_core::input::Key`] yet, so this talks to [`MenuList`] through
    /// the same literal key-string vocabulary its own tests use
    /// (`"ArrowUp"`/`"ArrowDown"`), rather than inventing that mapping
    /// layer for a single example (out of this issue's scope — "only
    /// assembling what batch 15 already shipped").
    fn handle_key(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        match self.hud.overlay {
            OverlayState::Closed => {
                if matches!(&event.logical_key, WinitKey::Named(NamedKey::Escape)) {
                    self.hud.menu = build_pause_menu();
                    self.hud.overlay = OverlayState::Open;
                }
            }
            OverlayState::Open => match &event.logical_key {
                WinitKey::Named(NamedKey::Escape) => self.hud.overlay = OverlayState::Closed,
                WinitKey::Named(NamedKey::ArrowUp) => self.hud.menu.handle_key_down("ArrowUp"),
                WinitKey::Named(NamedKey::ArrowDown) => self.hud.menu.handle_key_down("ArrowDown"),
                WinitKey::Named(NamedKey::Enter) => {
                    let selected = self
                        .hud
                        .menu
                        .selected_item()
                        .map(|item| item.label().to_string());
                    match selected.as_deref() {
                        Some("Resume") => self.hud.overlay = OverlayState::Closed,
                        Some("Quit") => self.hud.overlay = OverlayState::QuitConfirm,
                        _ => {}
                    }
                }
                _ => {
                    if event.physical_key == PhysicalKey::Code(KeyCode::KeyM) {
                        self.hud.mute = !self.hud.mute;
                    }
                }
            },
            OverlayState::QuitConfirm => match &event.logical_key {
                WinitKey::Named(NamedKey::Enter) => event_loop.exit(),
                WinitKey::Named(NamedKey::Escape) => self.hud.overlay = OverlayState::Open,
                _ => {}
            },
        }
    }
}

impl ApplicationHandler for HostApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("host-embed-wgpu — host-owned 3D + mkui HUD (#175)")
                .with_inner_size(winit::dpi::LogicalSize::new(960.0, 720.0)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("host-embed-wgpu: failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        #[cfg_attr(not(feature = "slug"), allow(unused_mut))]
        let mut gpu = pollster::block_on(Gpu::new(window.clone()));
        // Route HUD component text through the real Abel Slug face instead
        // of `EmbeddedRenderer`'s default bitmap-only lane (#259).
        #[cfg(feature = "slug")]
        gpu.embedded.set_slug_font(Some(self.abel_font_id));
        self.gpu = Some(gpu);
        self.window = Some(window.clone());
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window_matches = self
            .window
            .as_ref()
            .is_some_and(|window| window.id() == window_id);
        if !window_matches {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
                let scale = self
                    .window
                    .as_ref()
                    .map(|window| window.scale_factor())
                    .unwrap_or(1.0);
                self.viewport = logical_viewport(size, scale);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    self.handle_key(&event, event_loop);
                }
            }
            WindowEvent::RedrawRequested => {
                let elapsed = self.start.elapsed().as_secs_f32();
                // #263: `cpu_update_ms` times only the pure-CPU scene-build
                // work below — no GPU involvement — stopped before
                // `Gpu::render` starts its own `cpu_render_ms` clock.
                let update_start = Instant::now();
                maybe_fire_toast(
                    &mut self.hud.toasts,
                    &mut self.last_toast_at,
                    &mut self.toast_cycle,
                    elapsed,
                );
                self.hud
                    .tooltip
                    .set_focused(self.hud.overlay == OverlayState::Open);
                let scene = build_hud_scene(self.viewport, elapsed, &self.hud, &self.theme);
                let cpu_update_ms = update_start.elapsed().as_secs_f64() * 1000.0;
                if let Some(gpu) = self.gpu.as_mut() {
                    if let Some(stats) = gpu.render(elapsed, &scene, &self.text_system) {
                        log_frame_stats(cpu_update_ms, &stats);
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CI smoke (no display server): build the HUD scene once, then return
    // *before* `EventLoop::new()` — not inside `resumed`. On a displayless
    // Linux host (no X11, no Wayland) `EventLoop::new()` itself fails, so
    // `resumed` never runs to reach a headless check placed there; the
    // check has to happen before the event loop is constructed at all.
    if std::env::var_os("HEADLESS").is_some() {
        let _ = build_hud_scene(
            Size::new(960.0, 720.0),
            0.0,
            &HudState::new(),
            &WgpuTheme::default(),
        );
        return Ok(());
    }

    let event_loop = EventLoop::new()?;
    let mut app = HostApp::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_wgpu::Primitive;

    #[test]
    fn build_cube_vertices_emits_six_faces_of_two_triangles() {
        let vertices = build_cube_vertices();
        assert_eq!(vertices.len(), 36, "6 faces x 2 triangles x 3 vertices");
    }

    #[test]
    fn build_cube_vertices_shades_the_lit_face_brighter_than_its_opposite() {
        // The +Y face (index 2 -> vertices[12]) points mostly toward the
        // fixed light; its opposite, -Y (index 3 -> vertices[18]), points
        // away from it and must come out darker despite a lighter base
        // color — proving the per-face Lambertian term, not just the base
        // color, drives the baked vertex color.
        let vertices = build_cube_vertices();
        let top: f32 = vertices[12].color.iter().sum();
        let bottom: f32 = vertices[18].color.iter().sum();
        assert!(
            top > bottom,
            "lit +Y face ({top}) must be brighter than its opposite -Y face ({bottom})"
        );
    }

    #[test]
    fn slot_ring_alternates_write_slots_and_lags_the_resolve_slot_by_one() {
        // The exact sequence #263's review asked to see pinned down: no
        // resolve target until a slot has actually been written once, then
        // alternating write slots with the resolve slot always one step
        // behind (never the slot just written this same call).
        let mut ring = SlotRing::new();
        assert_eq!(
            ring.advance(),
            (0, None),
            "first call: nothing to resolve yet"
        );
        assert_eq!(
            ring.advance(),
            (1, Some(0)),
            "second call: write slot 1, resolve the slot the first call wrote"
        );
        assert_eq!(
            ring.advance(),
            (0, Some(1)),
            "third call: write slot 0 again, resolve the slot the second call wrote"
        );
        assert_eq!(ring.advance(), (1, Some(0)));
    }

    #[test]
    fn depth_texture_bytes_is_four_bytes_per_texel() {
        assert_eq!(depth_texture_bytes(960, 720), 960 * 720 * 4);
    }

    #[test]
    fn depth_texture_bytes_floors_zero_dimensions_to_one_pixel() {
        // `.max(1)` inside the helper, mirroring `create_depth_view`'s own
        // zero-size guard — a 0-sized texture is invalid, not 0 bytes.
        assert_eq!(depth_texture_bytes(0, 0), 4);
    }

    #[test]
    fn gpu_frame_timings_converts_ticks_to_milliseconds_via_the_period() {
        // 1,000,000 ticks at a 1ns period is 1ms; period scaling must apply
        // per delta, not just to the final total. `hud_ms` is `t2..t3`
        // (the HUD pass's own begin/end, bracketed directly since #264),
        // independent of `t1`; `total_ms` is `t0..t3`, not `cube_ms +
        // hud_ms` summed.
        let timings = gpu_frame_timings(0, 1_000_000, 2_000_000, 4_000_000, 1.0);
        assert_eq!(timings.cube_ms, Some(1.0));
        assert_eq!(timings.hud_ms, Some(2.0));
        assert_eq!(timings.total_ms, Some(4.0));
    }

    #[test]
    fn gpu_frame_timings_reports_unreliable_for_an_out_of_order_pair() {
        // The regression this guards: a `to < from` pair (observed on real
        // hardware — see the module doc) must become `None`, never a
        // `u64::wrapping_sub` artifact that reads as a bogus
        // multi-trillion-millisecond value. Period `1_000_000.0` ns/tick
        // makes each tick exactly 1ms, so the expected deltas below are
        // round numbers.
        let timings = gpu_frame_timings(10, 5, 5, 20, 1_000_000.0);
        assert_eq!(
            timings.cube_ms, None,
            "an out-of-order (t1 < t0) pair must not produce a wrapped value"
        );
        assert_eq!(
            timings.hud_ms,
            Some(15.0),
            "a well-ordered pair elsewhere in the same frame stays unaffected"
        );
        assert_eq!(
            timings.total_ms,
            Some(10.0),
            "t0..t3 is independently well-ordered even though t0..t1 is not"
        );
    }

    fn sample_frame_stats(gpu: GpuTimingStatus) -> FrameStats {
        FrameStats {
            cpu_acquire_ms: 0.001,
            cpu_render_ms: 0.412,
            cpu_ui_ms: 0.183,
            gpu,
            ui_gpu_mem_bytes: 61_440,
            host_gpu_mem_bytes: 2_764_864,
        }
    }

    #[test]
    fn format_frame_stats_is_grep_and_parse_friendly_when_gpu_timing_is_supported() {
        let stats = sample_frame_stats(GpuTimingStatus::Timings(GpuFrameTimings {
            cube_ms: Some(0.02),
            hud_ms: Some(0.061),
            total_ms: Some(0.081),
        }));
        let line = format_frame_stats(0.015, &stats);

        assert!(line.starts_with("frame_stats "), "{line}");
        for key in [
            "cpu_update_ms=",
            "cpu_acquire_ms=",
            "cpu_render_ms=",
            "cpu_ui_ms=",
            "gpu_cube_ms=",
            "gpu_hud_ms=",
            "gpu_frame_ms=",
            "ui_gpu_mem_bytes=",
            "host_gpu_mem_bytes=",
        ] {
            assert!(line.contains(key), "{line} must contain {key}");
        }
        assert!(
            !line.contains("unsupported") && !line.contains("pending"),
            "{line}"
        );
    }

    #[test]
    fn format_frame_stats_marks_gpu_timing_unsupported_without_cube_or_hud_fields() {
        let stats = sample_frame_stats(GpuTimingStatus::Unsupported);
        let line = format_frame_stats(0.015, &stats);

        assert!(line.contains("gpu_frame_ms=unsupported"), "{line}");
        assert!(
            !line.contains("gpu_cube_ms=") && !line.contains("gpu_hud_ms="),
            "an unsupported adapter must not print per-pass GPU fields at all: {line}"
        );
        for key in [
            "cpu_update_ms=",
            "cpu_acquire_ms=",
            "cpu_render_ms=",
            "cpu_ui_ms=",
            "ui_gpu_mem_bytes=",
            "host_gpu_mem_bytes=",
        ] {
            assert!(line.contains(key), "{line} must contain {key}");
        }
    }

    #[test]
    fn format_frame_stats_marks_gpu_timing_pending_without_cube_or_hud_fields() {
        // Distinct from `Unsupported` (#263 review): the first frame only,
        // before double-buffering has anything to resolve yet — not a
        // permanent "this adapter can't do it" state.
        let stats = sample_frame_stats(GpuTimingStatus::Pending);
        let line = format_frame_stats(0.015, &stats);

        assert!(line.contains("gpu_frame_ms=pending"), "{line}");
        assert!(
            !line.contains("gpu_cube_ms=") && !line.contains("gpu_hud_ms="),
            "a pending sample must not print per-pass GPU fields at all: {line}"
        );
    }

    #[test]
    fn format_frame_stats_surfaces_unreliable_gpu_deltas_by_name() {
        let stats = sample_frame_stats(GpuTimingStatus::Timings(GpuFrameTimings {
            cube_ms: None,
            hud_ms: Some(0.061),
            total_ms: None,
        }));
        let line = format_frame_stats(0.015, &stats);

        assert!(line.contains("gpu_cube_ms=unreliable"), "{line}");
        assert!(line.contains("gpu_hud_ms=0.061"), "{line}");
        assert!(line.contains("gpu_frame_ms=unreliable"), "{line}");
    }

    fn viewport() -> Size {
        Size::new(960.0, 720.0)
    }

    fn has_text(scene: &Scene, content: &str) -> bool {
        scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Text(t) if t.content == content))
    }

    #[test]
    fn build_hud_scene_always_paints_the_base_hud_without_a_pause_overlay() {
        let theme = WgpuTheme::default();
        let hud = HudState::new();
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(
            !scene.primitives.is_empty(),
            "base HUD must paint something"
        );
        assert!(
            !has_text(&scene, "Paused"),
            "pause overlay must stay hidden until Escape is pressed"
        );
    }

    #[test]
    fn build_hud_scene_shows_the_pause_overlay_once_open() {
        let theme = WgpuTheme::default();
        let mut hud = HudState::new();
        hud.overlay = OverlayState::Open;
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(has_text(&scene, "Paused"));
        assert!(has_text(&scene, "Resume"));
        assert!(has_text(&scene, "Quit"));
    }

    #[test]
    fn build_hud_scene_stacks_the_quit_confirm_dialog_over_the_pause_menu() {
        let theme = WgpuTheme::default();
        let mut hud = HudState::new();
        hud.overlay = OverlayState::QuitConfirm;
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(has_text(&scene, "Quit to desktop?"));
        let backdrop_quads = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.dialog().backdrop))
            .count();
        assert_eq!(
            backdrop_quads, 1,
            "only the stacked quit-confirm dialog paints a full-viewport dialog backdrop"
        );
    }

    #[test]
    fn build_pause_menu_preselects_resume() {
        let menu = build_pause_menu();
        assert_eq!(
            menu.selected_item().map(|item| item.label()),
            Some("Resume")
        );
    }

    #[test]
    fn maybe_fire_toast_pushes_one_toast_per_interval_and_cycles_messages() {
        let mut toasts = ToastStack::new();
        let mut last_toast_at = -TOAST_INTERVAL_SECS;
        let mut cycle = 0usize;

        maybe_fire_toast(&mut toasts, &mut last_toast_at, &mut cycle, 0.0);
        assert_eq!(
            toasts.active().len(),
            1,
            "first tick fires promptly at startup"
        );

        maybe_fire_toast(&mut toasts, &mut last_toast_at, &mut cycle, 1.0);
        assert_eq!(
            toasts.active().len(),
            1,
            "no new toast before the interval elapses"
        );

        maybe_fire_toast(
            &mut toasts,
            &mut last_toast_at,
            &mut cycle,
            TOAST_INTERVAL_SECS + 1.1,
        );
        assert_eq!(
            toasts.active().len(),
            2,
            "a new toast fires once the interval elapses"
        );
    }

    #[test]
    fn logical_viewport_divides_by_scale_factor() {
        let viewport = logical_viewport(winit::dpi::PhysicalSize::new(1920, 1440), 2.0);
        assert_eq!(viewport.width, 960.0);
        assert_eq!(viewport.height, 720.0);
    }

    // #259: a CPU-only proof that the HUD's own text actually reaches the
    // Slug lane once Abel is registered — the real-window visual check
    // acceptance bullet 1 asks for is deferred to #230 (see the PR body); this
    // is the evidence this PR ships in its place. Exercises exactly the same
    // `expand_slug_text` seam `EmbeddedRenderer::render_into` drives at
    // runtime (`resumed`'s `set_slug_font` call above), just without a GPU.
    #[cfg(feature = "slug")]
    #[test]
    fn hud_text_routes_through_the_slug_lane_once_abel_is_registered() {
        use mkui_vector2d::{SlugBlobCache, SlugConfig};
        use mkui_wgpu::slug_text::expand_slug_text;

        let mut text_system = CompositeTextSystem::new();
        let abel_font_id = text_system
            .register_sfnt_face(
                std::sync::Arc::from(ABEL_TTF.to_vec().into_boxed_slice()),
                0,
            )
            .expect("Abel registers");
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let theme = WgpuTheme::default();

        for overlay in [
            OverlayState::Closed,
            OverlayState::Open,
            OverlayState::QuitConfirm,
        ] {
            let mut hud = HudState::new();
            hud.overlay = overlay;
            let mut last_toast_at = -TOAST_INTERVAL_SECS;
            let mut cycle = 0usize;
            maybe_fire_toast(&mut hud.toasts, &mut last_toast_at, &mut cycle, 0.0);
            // Mirrors `resumed`'s own `RedrawRequested` handler exactly
            // (`set_focused(overlay == Open)`) — the tooltip is focus-driven,
            // not drawn unconditionally, so a test that skips this never
            // exercises `components::tooltip`'s text at all.
            hud.tooltip.set_focused(overlay == OverlayState::Open);

            let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
            let text_count = scene
                .primitives
                .iter()
                .filter(|p| matches!(p, Primitive::Text(_)))
                .count();
            assert!(text_count > 0, "{overlay:?}'s scene must contain text");

            // Named labels, not just a count — proves specific tight-rect
            // content is actually present before asserting it all survives
            // Slug routing below. "LIVE" (56x24 badge), "2480" (the score
            // `counter_chip`'s count label — its own "Score" tooltip string
            // never paints here since this call site passes
            // hovered=focused=false), and the first toast string are always
            // present (base HUD); "Audio (M)" (80x20) + the default-quality
            // "MEDIUM" badge only once the pause overlay is open; the
            // tooltip only when `Open` specifically (see above).
            for label in ["LIVE", "2480", "Checkpoint saved"] {
                assert!(
                    has_text(&scene, label),
                    "{overlay:?}'s scene must contain the \"{label}\" label"
                );
            }
            if overlay != OverlayState::Closed {
                for label in ["Audio (M)", "MEDIUM"] {
                    assert!(
                        has_text(&scene, label),
                        "{overlay:?}'s scene must contain the \"{label}\" label"
                    );
                }
            }
            if overlay == OverlayState::Open {
                assert!(
                    has_text(&scene, "Lower quality reduces GPU load."),
                    "{overlay:?}'s scene must contain the focus-driven tooltip text"
                );
            }

            let expanded =
                expand_slug_text(&scene.primitives, abel_font_id, &text_system, &mut cache);
            let slug_glyphs = expanded
                .iter()
                .filter(|p| matches!(p, Primitive::SlugGlyph(_)))
                .count();
            assert!(
                slug_glyphs > 0,
                "{overlay:?}'s HUD scene must route at least one glyph through the Slug \
                 lane once Abel is registered"
            );

            // Every tight-rect label ("LIVE"/quality badges, "Audio (M)", the
            // menu rows, the toast) must actually route through Slug, not
            // silently stay on `expand_text_primitive`'s whole-primitive
            // bitmap-lane bail (its two guards: no Slug run at all for this
            // face, or a line still too wide even after ellipsizing) — #259's
            // acceptance bar is "throughout the HUD", not "somewhere in the
            // HUD". A single whitespace-only leftover is NOT that bug: it's
            // `place_slug_run`'s documented *per-glyph* fallback (a space
            // glyph's own outline has no drawable curves, so just that one
            // character's bitmap_fallback degrades — functionally invisible,
            // since the bitmap space glyph is itself blank) — distinct from a
            // whole label bailing, which would carry the label's full
            // original (non-whitespace) content.
            let bailed_labels: Vec<&str> = expanded
                .iter()
                .filter_map(|p| match p {
                    Primitive::Text(t) if !t.content.trim().is_empty() => Some(t.content.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                bailed_labels.is_empty(),
                "{overlay:?}'s HUD scene bailed these labels to the bitmap lane entirely: \
                 {bailed_labels:?} — every one of {text_count} must route through Slug"
            );
        }
    }
}
