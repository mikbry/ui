#![forbid(unsafe_code)]
//! # mkui-vector2d-wgpu — native WGPU adapter for the Slug glyph lane
//!
//! This crate owns the **GPU half** of mkui's vector text lane (#66). It packs
//! the backend-neutral [`SlugGlyph`] blobs produced by `mkui-vector2d` (#65)
//! into WGPU storage buffers, owns the Slug WGSL coverage pipeline, and records
//! a Slug draw into a caller-supplied render pass.
//!
//! ## Ownership ADR (#64)
//!
//! ```text
//! mkui-vector2d        ->  mkui-text             (CPU paths; no GPU)
//! mkui-vector2d-wgpu   ->  mkui-vector2d + wgpu  (this crate; GPU packing)
//! mkui-wgpu            ->  mkui-vector2d-wgpu    (consumes the adapter)
//! ```
//!
//! - This crate **never depends on `mkui-wgpu`** — it is consumed by it, not
//!   the other way around.
//! - This crate **owns no window, surface, adapter selection, or frame
//!   lifecycle**. [`SlugAdapter::new`] is initialised from a borrowed
//!   `&wgpu::Device` plus the target color format; [`SlugAdapter::prepare`]
//!   borrows `&wgpu::Device` + `&wgpu::Queue`. Surface negotiation, the render
//!   pass, clear/load/store, and present all stay in `mkui-wgpu`.
//! - Browser WebGPU is out of scope (#66) — native WGPU only.
//!
//! ## What it owns
//!
//! - [`SlugAdapter`] — the render pipeline, bind-group layout, and WGSL.
//! - [`PlacedSlugGlyph`] — a pre-encoded glyph blob plus its on-screen placement
//!   (pen origin, pixels-per-font-unit scale, fill colour).
//! - [`PlacedVectorPath`] / [`PlacedStroke`] — an arbitrary `mkui-vector2d`
//!   [`VectorPath`] fill, or a [`Stroke`]d centreline, placed on screen and
//!   encoded to the *same* coverage records at [`SlugAdapter::prepare_paths`]
//!   time (Sprint 8 §3.1 Wave 1, #138).
//! - [`PreparedSlug`] — the per-frame GPU buffers + bind group, ready to draw.
//!
//! ## Pipeline
//!
//! One dilated quad is emitted per glyph. The fragment shader maps each pixel
//! back to font-unit space, selects the horizontal band, and accumulates a
//! single-horizontal-ray anti-aliased coverage over that band's curves (the
//! `mkui-vector2d` band membership/ordering is consumed verbatim, never
//! recomputed). Output is straight-alpha and composited with standard alpha
//! blending so a Slug draw is load/store compatible with the UI/bitmap lanes in
//! one render pass.
//!
//! ## VectorPath + Stroke reuse the glyph pipeline unchanged
//!
//! The WGSL shader is a *general* quadratic-curve coverage rasterizer, so
//! rendering an arbitrary [`VectorPath`] or a [`Stroke`] needs **no shader
//! change** — only extra CPU prepare-time work. A fill is encoded with
//! `mkui-vector2d`'s `encode_vector_path` (cubics subdivided); a stroke is first
//! lowered to a fillable outline with `stroke_to_fill` and then encoded the same
//! way. Both produce a [`SlugGlyph`] that flows through the identical
//! pack/upload/draw path as a glyph blob, so the glyph lane is byte-identical
//! and the two new lanes share every downstream GPU record.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

pub use mkui_vector2d::{
    BandRange, Bounds, DashPattern, FillRule, GlyphBounds, LineCap, LineJoin, PathCommand,
    SlugCurve, SlugGlyph, Stroke, Vec2, VectorPath,
};

use mkui_vector2d::{encode_vector_path, stroke_to_fill, SlugConfig};

/// Half a *physical* pixel, expressed in font units via a glyph's own
/// placement scale and the frame's logical→physical pixel ratio (#157 Phase 2
/// step 4: bounded 2D dilation). Added to every edge of a glyph's *font-unit*
/// bounds, before pixel projection, so the anti-aliasing footprint at the ink
/// boundary is never clipped by the quad.
///
/// `scale_px_per_unit` and every other coordinate this crate packs
/// (`origin_px`, `viewport_px`) are in the caller's own pixel space, which in
/// mkui-wgpu's real windowed renderer is *logical* pixels — the physical
/// surface can carry more device pixels per logical pixel on a HiDPI display
/// (`device_pixel_ratio > 1.0`). Dilating by a flat `0.5` in that logical
/// space would shrink to less than half a *physical* pixel at 1x and grow
/// past it at 2x/3x, exactly the asymmetry Codex's original Sprint 8 review
/// flagged (`app.rs`'s logical→physical gap). Dividing by `device_pixel_ratio`
/// too corrects for that: `0.5 / (device_pixel_ratio * scale)` font units,
/// scaled back by `scale` at projection time, is exactly half a *physical*
/// pixel regardless of the window's DPI. Callers with no logical/physical
/// split (an offscreen render whose `viewport_px` already equals the
/// physical target, e.g. this crate's own tests) pass `1.0`.
///
/// mkui's screen transform is otherwise a uniform, axis-aligned scale +
/// translate — no rotation, skew, or perspective. The reference adapter's
/// vertex shader (`slug_dilate`, "A Decade of Slug" § Dynamic Dilation)
/// derives the equivalent half-pixel expansion from the render's full
/// Jacobian so it stays correct under an arbitrary transform; for the
/// transform mkui actually ships, that general computation collapses to this
/// closed form. Full per-render MVP/viewport-derived dynamic dilation is
/// deliberately out of scope for this mission (mkui doesn't ship
/// perspective/transform text yet).
fn half_pixel_dilation_units(scale_px_per_unit: f32, device_pixel_ratio: f32) -> f32 {
    0.5 / (device_pixel_ratio.max(f32::EPSILON) * scale_px_per_unit.max(f32::EPSILON))
}

/// #157 Phase 3 (Codex 8-step-plan step 6): below this cap-height, small UI
/// text gets its baseline snapped to the physical pixel grid. Lives here
/// (alongside [`half_pixel_dilation_units`]) rather than in the text-layout
/// caller because, like dilation, the snap needs the frame's *fresh*
/// `device_pixel_ratio` — Codex round 1 of this phase's PR review correctly
/// rejected an earlier revision that baked a caller-supplied DPR into the
/// baseline at scene-construction time (`place_slug_run`), before the real
/// per-frame DPR is known and without re-snapping on `ScaleFactorChanged`.
/// Doing it in [`pack`] instead means every frame re-derives the snap from
/// [`PlacedSlugGlyph::origin_px`] (kept unsnapped by the caller) and the
/// frame's own `device_pixel_ratio`, so a DPI change self-corrects with no
/// separate invalidation path.
const SMALL_TEXT_CAP_HEIGHT_PX: f32 = 16.0;

/// Round `value_px` to the nearest physical pixel at `device_pixel_ratio`,
/// then convert back to logical pixels.
fn snap_to_physical_pixel(value_px: f32, device_pixel_ratio: f32) -> f32 {
    (value_px * device_pixel_ratio).round() / device_pixel_ratio.max(f32::EPSILON)
}

/// A Slug glyph blob placed on screen.
///
/// The placement is the minimal affine the GPU lane needs: a pen `origin_px`
/// (where the glyph's font-unit origin `(0, 0)` lands, in y-down screen pixels)
/// and a uniform `scale_px_per_unit` (font size ÷ units-per-em). The encoded
/// blob stays in font units, y-up — the adapter performs the y-flip and pixel
/// projection so the neutral crate never sees a screen coordinate.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSlugGlyph {
    /// The size-independent outline blob from `mkui-vector2d`.
    pub blob: Arc<SlugGlyph>,
    /// Screen-pixel position (y-down) of the glyph's font-unit origin. Always
    /// the caller's *unsnapped* pen position — see `cap_height_px` below.
    pub origin_px: [f32; 2],
    /// Pixels per font unit (placement scale).
    pub scale_px_per_unit: f32,
    /// Straight-alpha RGBA fill colour in `[0, 1]`.
    pub color: [f32; 4],
    /// #157 Phase 3: the text-layout caller's cap-height proxy for this
    /// glyph (its run's nominal font size), used by [`pack`] to gate the
    /// below-`SMALL_TEXT_CAP_HEIGHT_PX` baseline snap. Producers with no
    /// natural cap-height concept (hand-authored fixtures, arbitrary
    /// [`PlacedVectorPath`]/[`PlacedStroke`] fills) pass `f32::INFINITY` to
    /// opt out unambiguously — never default to a small value, which would
    /// silently enable snapping and desync a fixture's expected geometry.
    pub cap_height_px: f32,
}

/// An arbitrary [`VectorPath`] fill placed on screen.
///
/// Unlike [`PlacedSlugGlyph`], the geometry is carried un-encoded: the path is
/// lowered to the shared coverage records at [`SlugAdapter::prepare_paths`] time
/// (cubics subdivided, non-zero winding). The placement matches the glyph lane —
/// an `origin_px` where the path's own `(0, 0)` lands and a uniform
/// `scale_px_per_unit` — so a path scales with zoom exactly like a glyph. The
/// path stays in its own units, y-up; the adapter performs the y-flip.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedVectorPath {
    /// The backend-neutral fill geometry (its `fill` is treated as non-zero).
    pub path: VectorPath,
    /// Screen-pixel position (y-down) of the path's origin `(0, 0)`.
    pub origin_px: [f32; 2],
    /// Pixels per path unit (placement scale).
    pub scale_px_per_unit: f32,
    /// Straight-alpha RGBA fill colour in `[0, 1]`.
    pub color: [f32; 4],
}

/// A [`Stroke`]d centreline [`VectorPath`] placed on screen.
///
/// The stroke is expanded to a fillable outline on the CPU (`stroke_to_fill`:
/// caps, joins, dashing) and then encoded like any other fill. [`Stroke::width_px`]
/// and the dash lengths are consumed in the path's own coordinate units and
/// scaled to screen pixels by `scale_px_per_unit` alongside the geometry, so a
/// stroke's width tracks zoom like its fill.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedStroke {
    /// The centreline geometry to stroke.
    pub path: VectorPath,
    /// How to paint the centreline (width, caps, joins, dash).
    pub stroke: Stroke,
    /// Screen-pixel position (y-down) of the path's origin `(0, 0)`.
    pub origin_px: [f32; 2],
    /// Pixels per path unit (placement scale).
    pub scale_px_per_unit: f32,
    /// Straight-alpha RGBA stroke colour in `[0, 1]`.
    pub color: [f32; 4],
}

/// Band counts for encoding arbitrary paths/strokes. Glyphs carry a caller-tuned
/// [`SlugConfig`]; a general path has no such context, so the adapter fixes a
/// balanced default (finer than the 2×2 glyph golden, coarse enough to keep the
/// band tables small). Band count never changes coverage correctness — only the
/// per-pixel curve-list length — so a fixed default is safe (a caller-supplied
/// config is a future refinement, not a Sprint 8 requirement).
fn default_path_config() -> SlugConfig {
    SlugConfig::new(8, 8, 1)
}

/// Encode a placed fill into a drawable [`PlacedSlugGlyph`], or `None` when the
/// path carries no drawable geometry (empty or non-finite) so it is skipped.
fn encode_fill(placed: &PlacedVectorPath) -> Option<PlacedSlugGlyph> {
    let glyph = encode_vector_path(&placed.path, &default_path_config()).ok()?;
    Some(PlacedSlugGlyph {
        blob: Arc::new(glyph),
        origin_px: placed.origin_px,
        scale_px_per_unit: placed.scale_px_per_unit,
        color: placed.color,
        // Arbitrary path fills have no cap-height concept; opt out of the
        // Phase 3 small-text baseline snap.
        cap_height_px: f32::INFINITY,
    })
}

/// Expand a placed stroke to a fill outline and encode it, or `None` when the
/// stroke paints nothing (non-positive width, empty, or fully degenerate).
fn encode_stroke(placed: &PlacedStroke) -> Option<PlacedSlugGlyph> {
    let outline = stroke_to_fill(&placed.path, &placed.stroke);
    let glyph = encode_vector_path(&outline, &default_path_config()).ok()?;
    Some(PlacedSlugGlyph {
        blob: Arc::new(glyph),
        origin_px: placed.origin_px,
        scale_px_per_unit: placed.scale_px_per_unit,
        color: placed.color,
        // Stroked outlines have no cap-height concept either; opt out.
        cap_height_px: f32::INFINITY,
    })
}

/// GPU curve record matching the WGSL `Curve` struct (std430): three `vec2`s,
/// 24 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct GpuCurve {
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
}

/// GPU band record matching the WGSL `Band` struct (std430): two `f32` + two
/// `u32`, 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct GpuBand {
    lower: f32,
    upper: f32,
    first_curve: u32,
    curve_count: u32,
}

/// GPU per-glyph instance matching the WGSL `Glyph` struct (std430), 96 bytes.
/// Field order is chosen so the `vec4` colour leads and every member is
/// naturally aligned — `repr(C)` then matches std430 with no manual padding.
/// The trailing vertical-band trio + font-unit bounds (#157 Phase 1) let the
/// fragment shader select both the horizontal (row) and vertical (column)
/// band for a sample the same way the reference adapter's `band_transform`
/// does: a clamped index derived from the glyph's own font-unit bounds, not a
/// containment scan — so AA coverage stays correct in the dilation margin
/// just outside the exact glyph bounds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct GpuGlyph {
    color: [f32; 4],
    quad_min: [f32; 2],
    quad_max: [f32; 2],
    origin_px: [f32; 2],
    params: [f32; 2],
    curve_base: u32,
    band_base: u32,
    band_count: u32,
    index_base: u32,
    bounds_min: [f32; 2],
    bounds_max: [f32; 2],
    vband_base: u32,
    vband_count: u32,
    vindex_base: u32,
    _pad: u32,
}

/// GPU viewport uniform matching the WGSL `Viewport` struct, 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct GpuViewport {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// CPU-packed buffers for one Slug frame. Kept separate from GPU upload so the
/// packing contract is unit-testable without a device.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct PackedSlug {
    curves: Vec<GpuCurve>,
    bands: Vec<GpuBand>,
    indices: Vec<u32>,
    glyphs: Vec<GpuGlyph>,
}

/// Pack placed glyphs into shared GPU buffers. Glyphs with no curves are
/// skipped (they would emit an empty quad). Returns the flat curve/band/index
/// streams plus one [`GpuGlyph`] instance per drawable glyph. `device_pixel_ratio`
/// is the frame's physical-pixels-per-logical-pixel ratio (`1.0` when the
/// caller's pixel space has no logical/physical split) — see
/// [`half_pixel_dilation_units`].
pub(crate) fn pack(glyphs: &[PlacedSlugGlyph], device_pixel_ratio: f32) -> PackedSlug {
    let mut packed = PackedSlug::default();
    for placed in glyphs {
        let blob = placed.blob.as_ref();
        if blob.curves.is_empty() {
            continue;
        }
        let scale = placed.scale_px_per_unit.max(f32::EPSILON);
        let curve_base = packed.curves.len() as u32;
        let band_base = packed.bands.len() as u32;
        let index_base = packed.indices.len() as u32;

        for c in &blob.curves {
            packed.curves.push(GpuCurve {
                p0: [c.p0.x, c.p0.y],
                p1: [c.p1.x, c.p1.y],
                p2: [c.p2.x, c.p2.y],
            });
        }
        for band in &blob.horizontal_bands {
            packed.bands.push(GpuBand {
                lower: band.lower,
                upper: band.upper,
                first_curve: band.first_curve,
                curve_count: band.curve_count,
            });
        }
        packed
            .indices
            .extend_from_slice(&blob.horizontal_curve_indices);

        // Vertical bands + their curve-index stream follow immediately after
        // the horizontal ones in the same shared buffers (#157 Phase 1): one
        // extra base offset per glyph, no new GPU bindings.
        let vband_base = packed.bands.len() as u32;
        let vindex_base = packed.indices.len() as u32;
        for band in &blob.vertical_bands {
            packed.bands.push(GpuBand {
                lower: band.lower,
                upper: band.upper,
                first_curve: band.first_curve,
                curve_count: band.curve_count,
            });
        }
        packed
            .indices
            .extend_from_slice(&blob.vertical_curve_indices);

        // Dilated screen quad: expand the font-unit bounds by half a
        // *physical* pixel's worth of font units *before* projecting to
        // pixels (see `half_pixel_dilation_units`). Font units are y-up;
        // screen is y-down, so the glyph's y_max maps to the smaller (top)
        // screen y.
        let [ox, oy] = placed.origin_px;
        // #157 Phase 3: below the small-text cap-height threshold, snap the
        // baseline to the frame's *current* physical pixel grid — computed
        // fresh here (not cached from scene-construction time) so a DPI
        // change is reflected on the very next frame with no separate
        // invalidation path. `placed.origin_px` itself is never mutated.
        let oy = if placed.cap_height_px < SMALL_TEXT_CAP_HEIGHT_PX {
            snap_to_physical_pixel(oy, device_pixel_ratio)
        } else {
            oy
        };
        let dilation = half_pixel_dilation_units(scale, device_pixel_ratio);
        let left = ox + (blob.bounds.x_min - dilation) * scale;
        let right = ox + (blob.bounds.x_max + dilation) * scale;
        let top = oy - (blob.bounds.y_max + dilation) * scale;
        let bottom = oy - (blob.bounds.y_min - dilation) * scale;

        packed.glyphs.push(GpuGlyph {
            color: placed.color,
            quad_min: [left, top],
            quad_max: [right, bottom],
            origin_px: [ox, oy],
            params: [scale, 0.0],
            curve_base,
            band_base,
            band_count: blob.horizontal_bands.len() as u32,
            index_base,
            bounds_min: [blob.bounds.x_min, blob.bounds.y_min],
            bounds_max: [blob.bounds.x_max, blob.bounds.y_max],
            vband_base,
            vband_count: blob.vertical_bands.len() as u32,
            vindex_base,
            _pad: 0,
        });
    }
    packed
}

/// Native WGPU adapter that renders `mkui-vector2d` Slug glyph blobs.
///
/// Owns only the pipeline + bind-group layout. It holds no device, queue,
/// surface, or per-frame buffers — those are borrowed at [`prepare`] time and
/// the resulting [`PreparedSlug`] is handed back for the caller to [`draw`]
/// inside its own render pass.
///
/// [`prepare`]: SlugAdapter::prepare
/// [`draw`]: SlugAdapter::draw
#[derive(Debug)]
pub struct SlugAdapter {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl SlugAdapter {
    /// Build the Slug pipeline for `target_format`. The device is borrowed —
    /// the adapter takes no ownership of GPU lifecycle.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("slug.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Bind Group Layout"),
            entries: &[
                uniform_entry(0),
                storage_entry(1),
                storage_entry(2),
                storage_entry(3),
                storage_entry(4),
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let targets = [Some(wgpu::ColorTargetState {
            format: target_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_slug"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_slug"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    /// Pack `glyphs` and upload them as a [`PreparedSlug`] for `viewport_px`
    /// (the logical-pixel viewport the quads are projected against).
    /// `device_pixel_ratio` is the frame's physical-pixels-per-logical-pixel
    /// ratio (`1.0` for a caller with no logical/physical split, e.g. an
    /// offscreen render whose `viewport_px` already is the physical target) —
    /// see [`half_pixel_dilation_units`]. Returns `None` when there is
    /// nothing drawable (no glyph carried any curves) so the caller can skip
    /// the draw entirely. Borrows device + queue only.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_px: [f32; 2],
        device_pixel_ratio: f32,
        glyphs: &[PlacedSlugGlyph],
    ) -> Option<PreparedSlug> {
        self.prepare_packed(device, queue, viewport_px, pack(glyphs, device_pixel_ratio))
    }

    /// Encode and prepare arbitrary [`VectorPath`] fills and [`PlacedStroke`]s
    /// for `viewport_px`, reusing the exact glyph coverage pipeline. Each fill is
    /// lowered via `encode_vector_path` and each stroke via `stroke_to_fill` +
    /// `encode_vector_path` into the shared [`SlugGlyph`] records, then packed and
    /// uploaded identically to a glyph frame. `device_pixel_ratio` is as in
    /// [`Self::prepare`]. Items that paint nothing (empty, non-finite, or a
    /// non-positive stroke width) are skipped; returns `None` when nothing is
    /// drawable. Borrows device + queue only.
    pub fn prepare_paths(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_px: [f32; 2],
        device_pixel_ratio: f32,
        fills: &[PlacedVectorPath],
        strokes: &[PlacedStroke],
    ) -> Option<PreparedSlug> {
        let placed: Vec<PlacedSlugGlyph> = fills
            .iter()
            .filter_map(encode_fill)
            .chain(strokes.iter().filter_map(encode_stroke))
            .collect();
        self.prepare_packed(
            device,
            queue,
            viewport_px,
            pack(&placed, device_pixel_ratio),
        )
    }

    /// Upload already-packed records into a [`PreparedSlug`]. Shared by
    /// [`prepare`](Self::prepare) and [`prepare_paths`](Self::prepare_paths) so
    /// every lane hits the same buffer/bind-group path. Returns `None` when the
    /// pack is empty.
    fn prepare_packed(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_px: [f32; 2],
        packed: PackedSlug,
    ) -> Option<PreparedSlug> {
        if packed.glyphs.is_empty() {
            return None;
        }

        let viewport = GpuViewport {
            size: viewport_px,
            _pad: [0.0, 0.0],
        };
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Viewport"),
            contents: bytemuck::bytes_of(&viewport),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Storage bindings reject zero-length buffers, so each stream is padded
        // to at least one element. `glyphs` is already non-empty here.
        let curves = storage_buffer(device, "Slug Curves", &pad_one(packed.curves));
        let bands = storage_buffer(device, "Slug Bands", &pad_one(packed.bands));
        let indices = storage_buffer(device, "Slug Indices", &pad_one(packed.indices));
        let glyph_buffer = storage_buffer(device, "Slug Glyphs", &packed.glyphs);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mkui-vector2d-wgpu Slug Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: curves.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: bands.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: indices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: glyph_buffer.as_entire_binding(),
                },
            ],
        });

        // The queue handle is part of the borrowed-lifecycle contract even
        // though `create_buffer_init` performs the upload; touch it so the
        // signature stays honest and a future staged-upload swap is local.
        let _ = queue;

        Some(PreparedSlug {
            bind_group,
            instance_count: packed.glyphs.len() as u32,
            _buffers: [viewport_buffer, curves, bands, indices, glyph_buffer],
        })
    }

    /// Record the Slug draw into `pass`. Six vertices per glyph instance (one
    /// dilated quad). The caller owns the render pass, its clear/load/store, and
    /// submission — this only binds the pipeline and issues the draw.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, prepared: &PreparedSlug) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &prepared.bind_group, &[]);
        pass.draw(0..6, 0..prepared.instance_count);
    }
}

/// Per-frame Slug GPU resources, ready to [`SlugAdapter::draw`]. Holds its
/// buffers alive for the lifetime of the draw.
#[derive(Debug)]
pub struct PreparedSlug {
    bind_group: wgpu::BindGroup,
    instance_count: u32,
    _buffers: [wgpu::Buffer; 5],
}

impl PreparedSlug {
    /// Number of glyph instances (quads) this frame draws.
    pub fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_buffer<T: Pod>(device: &wgpu::Device, label: &str, data: &[T]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

/// Return `data`, or a single zeroed element when empty, so a storage binding
/// never sees a zero-length buffer (which wgpu rejects).
fn pad_one<T: Pod + Zeroable>(mut data: Vec<T>) -> Vec<T> {
    if data.is_empty() {
        data.push(T::zeroed());
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_vector2d::{
        encode_slug_glyph, Bounds, FillRule, PathCommand, SlugConfig, Vec2, VectorPath,
    };

    /// A closed triangle with one quadratic — the same shape mkui-vector2d's
    /// golden test uses, so the packed records are easy to reason about.
    fn triangle_blob() -> Arc<SlugGlyph> {
        let path = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(100.0, 0.0)),
                PathCommand::QuadTo {
                    control: Vec2::new(100.0, 50.0),
                    to: Vec2::new(0.0, 100.0),
                },
                PathCommand::Close,
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
        );
        Arc::new(encode_slug_glyph(&path, &SlugConfig::new(2, 2, 1)).unwrap())
    }

    fn placed(blob: Arc<SlugGlyph>) -> PlacedSlugGlyph {
        PlacedSlugGlyph {
            blob,
            origin_px: [10.0, 200.0],
            scale_px_per_unit: 0.5,
            color: [1.0, 1.0, 1.0, 1.0],
            // Opts out of the Phase 3 small-text snap — these pre-Phase-3
            // tests assert exact, unsnapped geometry.
            cap_height_px: f32::INFINITY,
        }
    }

    #[test]
    fn pack_emits_one_instance_per_drawable_glyph() {
        let packed = pack(&[placed(triangle_blob()), placed(triangle_blob())], 1.0);
        assert_eq!(packed.glyphs.len(), 2, "one instance per glyph");
        // Two glyphs, each with three curves → six curve records, and the
        // second glyph's offsets must follow the first.
        assert_eq!(packed.curves.len(), 6);
        assert_eq!(packed.glyphs[0].curve_base, 0);
        assert_eq!(packed.glyphs[1].curve_base, 3);
    }

    #[test]
    fn pack_concatenates_band_and_index_streams_with_per_glyph_bases() {
        let blob = triangle_blob();
        let single = pack(&[placed(blob.clone())], 1.0);
        let double = pack(&[placed(blob.clone()), placed(blob.clone())], 1.0);

        // The second glyph's bases pick up exactly where the first ended.
        assert_eq!(
            double.glyphs[1].band_base,
            single.bands.len() as u32,
            "band_base follows the first glyph's bands"
        );
        assert_eq!(
            double.glyphs[1].index_base,
            single.indices.len() as u32,
            "index_base follows the first glyph's indices"
        );
        // The concatenated streams are exactly two copies of the single stream.
        assert_eq!(double.bands.len(), single.bands.len() * 2);
        assert_eq!(double.indices.len(), single.indices.len() * 2);
    }

    #[test]
    fn pack_records_match_the_neutral_blob_verbatim() {
        let blob = triangle_blob();
        let packed = pack(&[placed(blob.clone())], 1.0);
        // Curve records are copied 1:1 from the neutral blob (no reinterpretation).
        for (gpu, src) in packed.curves.iter().zip(blob.curves.iter()) {
            assert_eq!(gpu.p0, [src.p0.x, src.p0.y]);
            assert_eq!(gpu.p1, [src.p1.x, src.p1.y]);
            assert_eq!(gpu.p2, [src.p2.x, src.p2.y]);
        }
        // Horizontal bands/indices land first, vertical immediately after —
        // both streams copied 1:1 from the neutral blob (#157 Phase 1).
        let h_index_count = blob.horizontal_curve_indices.len();
        assert_eq!(
            packed.indices[..h_index_count],
            blob.horizontal_curve_indices
        );
        assert_eq!(packed.indices[h_index_count..], blob.vertical_curve_indices);
        assert_eq!(
            packed.bands.len(),
            blob.horizontal_bands.len() + blob.vertical_bands.len()
        );
        assert_eq!(
            packed.glyphs[0].band_count,
            blob.horizontal_bands.len() as u32
        );
        assert_eq!(
            packed.glyphs[0].vband_base,
            blob.horizontal_bands.len() as u32
        );
        assert_eq!(
            packed.glyphs[0].vband_count,
            blob.vertical_bands.len() as u32
        );
        assert_eq!(packed.glyphs[0].vindex_base, h_index_count as u32);
    }

    #[test]
    fn dilated_quad_brackets_the_scaled_glyph_bounds() {
        let p = placed(triangle_blob());
        let packed = pack(std::slice::from_ref(&p), 1.0);
        let g = packed.glyphs[0];
        let scale = p.scale_px_per_unit;
        // Half a physical pixel at this scale (#157 Phase 2).
        let dilation_px = half_pixel_dilation_units(scale, 1.0) * scale;
        assert!(
            (dilation_px - 0.5).abs() < 1e-4,
            "half-pixel dilation must be ~0.5 physical px regardless of scale, got {dilation_px}"
        );
        // x: origin + bounds*scale, dilated outward by half a physical pixel.
        assert_eq!(g.quad_min[0], p.origin_px[0] + 0.0 * scale - dilation_px);
        assert_eq!(g.quad_max[0], p.origin_px[0] + 100.0 * scale + dilation_px);
        // y is flipped: font y_max → top (smaller screen y) → quad_min.y.
        assert_eq!(g.quad_min[1], p.origin_px[1] - 100.0 * scale - dilation_px);
        assert_eq!(g.quad_max[1], p.origin_px[1] - 0.0 * scale + dilation_px);
        assert!(
            g.quad_min[1] < g.quad_max[1],
            "top must be above bottom in y-down screen space"
        );
    }

    #[test]
    fn dilation_stays_half_a_physical_pixel_across_device_pixel_ratios() {
        // Codex R1 review of #161: dilating by a flat amount in the caller's
        // *logical*-pixel space (mkui-wgpu's real windowed renderer projects
        // Slug quads against the logical viewport, #97) would shrink to less
        // than half a physical pixel at 1x and grow past it at 2x/3x. The
        // dilation in *logical* px must shrink as `device_pixel_ratio` grows
        // so the *physical* dilation stays exactly 0.5 at every ratio.
        let scale = 37.0; // an arbitrary, non-round placement scale.
        for device_pixel_ratio in [1.0_f32, 1.5, 2.0, 3.0] {
            let logical_dilation_px = half_pixel_dilation_units(scale, device_pixel_ratio) * scale;
            let physical_dilation_px = logical_dilation_px * device_pixel_ratio;
            assert!(
                (physical_dilation_px - 0.5).abs() < 1e-4,
                "device_pixel_ratio={device_pixel_ratio}: physical dilation must stay \
                 ~0.5px, got {physical_dilation_px} (logical {logical_dilation_px})"
            );
        }
    }

    // ---- Phase 3: small-text baseline snap (#157 step 6) -------------------

    // dame-rubric.md § Phase 3 (N): the snap function itself, tested in
    // isolation with exact control over the sub-pixel sweep — this is the
    // literal claim ("100 sub-pixel offsets across one physical-pixel period
    // group into exactly 2 piecewise-constant cells, split at the period's
    // midpoint").
    #[test]
    fn snap_to_physical_pixel_is_piecewise_constant_over_one_period() {
        let base = 10.0f32;
        let mut values = Vec::with_capacity(100);
        for i in 0..100 {
            let t = i as f32 / 100.0;
            values.push(snap_to_physical_pixel(base + t, 1.0));
        }
        let mut distinct = values.clone();
        distinct.dedup();
        assert_eq!(
            distinct,
            vec![10.0, 11.0],
            "expected exactly 2 cells (nearest-pixel snap), got {distinct:?}"
        );
        for (i, &v) in values.iter().enumerate() {
            let t = i as f32 / 100.0;
            let expected = if t < 0.5 { 10.0 } else { 11.0 };
            assert_eq!(
                v, expected,
                "offset {t} landed in the wrong cell (value {v})"
            );
        }
    }

    #[test]
    fn snap_to_physical_pixel_scales_grid_with_device_pixel_ratio() {
        for dpr in [1.0f32, 1.5, 2.0, 3.0] {
            let base = 10.0f32;
            let mut values = Vec::with_capacity(100);
            for i in 0..100 {
                // Sweep one physical-pixel period, expressed in logical px.
                let t = (i as f32 / 100.0) / dpr;
                values.push(snap_to_physical_pixel(base + t, dpr));
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert_eq!(
                distinct.len(),
                2,
                "device_pixel_ratio {dpr}: expected 2 cells, got {distinct:?}"
            );
            let delta = distinct[1] - distinct[0];
            assert!(
                (delta - 1.0 / dpr).abs() < 1e-4,
                "device_pixel_ratio {dpr}: cell delta {delta} should be one physical \
                 pixel (1/{dpr} logical px)"
            );
        }
    }

    fn placed_with_cap_height(
        blob: Arc<SlugGlyph>,
        origin_px: [f32; 2],
        cap_height_px: f32,
    ) -> PlacedSlugGlyph {
        PlacedSlugGlyph {
            blob,
            origin_px,
            scale_px_per_unit: 0.5,
            color: [1.0, 1.0, 1.0, 1.0],
            cap_height_px,
        }
    }

    #[test]
    fn pack_snaps_small_text_baseline_and_leaves_origin_px_untouched() {
        let blob = triangle_blob();
        // font_size_px 12 < SMALL_TEXT_CAP_HEIGHT_PX (16) — must snap.
        let p = placed_with_cap_height(blob, [10.0, 200.3], 12.0);
        let packed = pack(std::slice::from_ref(&p), 1.0);
        // quad_max.y is origin.y - bounds.y_min*scale + dilation; back out the
        // effective baseline by comparing against the unsnapped-baseline
        // dilation math at the *snapped* value (200.3 -> 200.0 at 1x DPR).
        let scale = p.scale_px_per_unit;
        let dilation_px = half_pixel_dilation_units(scale, 1.0) * scale;
        let g = packed.glyphs[0];
        assert_eq!(
            g.quad_max[1],
            200.0 - 0.0 * scale + dilation_px,
            "baseline 200.3 must snap to 200.0 at 1x DPR for small text"
        );
        // `pack` takes `&[PlacedSlugGlyph]` by reference — the caller's own
        // struct must stay unsnapped so re-packing at a different DPR next
        // frame re-derives the snap fresh rather than compounding.
        assert_eq!(
            p.origin_px[1], 200.3,
            "pack() must not mutate the caller's PlacedSlugGlyph"
        );
    }

    #[test]
    fn pack_does_not_snap_text_at_or_above_cap_height_threshold() {
        let blob = triangle_blob();
        // font_size_px 16 == SMALL_TEXT_CAP_HEIGHT_PX — must NOT snap (`<`, not `<=`).
        let p = placed_with_cap_height(blob, [10.0, 200.3], 16.0);
        let packed = pack(std::slice::from_ref(&p), 1.0);
        let scale = p.scale_px_per_unit;
        let dilation_px = half_pixel_dilation_units(scale, 1.0) * scale;
        let g = packed.glyphs[0];
        assert_eq!(
            g.quad_max[1],
            200.3 - 0.0 * scale + dilation_px,
            "16px text (== threshold) must pass through unsnapped"
        );
    }

    #[test]
    fn pack_re_derives_the_snap_fresh_at_a_new_device_pixel_ratio_each_call() {
        // Codex round 1 of the Phase 3 PR review: an earlier revision baked
        // the snap into the baseline at scene-construction time using a
        // caller-supplied DPR that could go stale (e.g. a window moved to a
        // different-DPI monitor). Packing the *same* unsnapped
        // `PlacedSlugGlyph` at two different device_pixel_ratios must
        // produce two independently-correct snaps, not a compounded one.
        let blob = triangle_blob();
        let p = placed_with_cap_height(blob, [10.0, 200.3], 12.0);
        let scale = p.scale_px_per_unit;

        let packed_1x = pack(std::slice::from_ref(&p), 1.0);
        let dilation_1x = half_pixel_dilation_units(scale, 1.0) * scale;
        assert_eq!(
            packed_1x.glyphs[0].quad_max[1],
            200.0 - 0.0 * scale + dilation_1x,
            "1x DPR: 200.3 snaps to 200.0"
        );

        let packed_2x = pack(std::slice::from_ref(&p), 2.0);
        let dilation_2x = half_pixel_dilation_units(scale, 2.0) * scale;
        // 200.3 at 2x physical grid: 200.3*2=400.6 -> round 401 -> /2 = 200.5.
        assert_eq!(
            packed_2x.glyphs[0].quad_max[1],
            200.5 - 0.0 * scale + dilation_2x,
            "2x DPR: 200.3 snaps to 200.5 (independently, from the same \
             unsnapped input — no compounding from the 1x call above)"
        );
    }

    #[test]
    fn empty_curve_glyph_is_skipped() {
        let empty = Arc::new(SlugGlyph {
            revision: 1,
            bounds: GlyphBounds {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 0.0,
                y_max: 0.0,
            },
            curves: Vec::new(),
            horizontal_bands: Vec::new(),
            horizontal_curve_indices: Vec::new(),
            vertical_bands: Vec::new(),
            vertical_curve_indices: Vec::new(),
        });
        let packed = pack(&[placed(empty)], 1.0);
        assert!(packed.glyphs.is_empty(), "a curve-less glyph emits no quad");
    }

    #[test]
    fn gpu_structs_have_the_std430_byte_sizes() {
        // Lock the layout the WGSL std430 structs assume.
        assert_eq!(std::mem::size_of::<GpuCurve>(), 24);
        assert_eq!(std::mem::size_of::<GpuBand>(), 16);
        assert_eq!(std::mem::size_of::<GpuGlyph>(), 96);
        assert_eq!(std::mem::size_of::<GpuViewport>(), 16);
    }

    #[test]
    fn pad_one_replaces_empty_but_preserves_nonempty() {
        assert_eq!(pad_one(Vec::<u32>::new()), vec![0u32]);
        assert_eq!(pad_one(vec![7u32, 9u32]), vec![7u32, 9u32]);
    }

    #[test]
    fn slug_wgsl_parses_and_validates() {
        // Static naga validation catches WGSL errors without a GPU, so a
        // shader typo fails in the default `test` job rather than only on the
        // Lavapipe GPU runner.
        use wgpu::naga;
        let module =
            naga::front::wgsl::parse_str(include_str!("slug.wgsl")).expect("Slug WGSL must parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("Slug WGSL must pass naga validation");
    }

    // ---- VectorPath + Stroke lanes (#138) -----------------------------------

    use mkui_vector2d::Stroke;

    fn fill_triangle() -> PlacedVectorPath {
        PlacedVectorPath {
            path: VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(100.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(0.0, 100.0)),
                    PathCommand::Close,
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            origin_px: [10.0, 200.0],
            scale_px_per_unit: 0.5,
            color: [1.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn fill_encodes_and_packs_like_a_glyph() {
        let placed = encode_fill(&fill_triangle()).expect("triangle fill encodes");
        let packed = pack(&[placed], 1.0);
        assert_eq!(packed.glyphs.len(), 1, "one instance for one fill");
        // A triangle flattens to three straight edges (all line sentinels).
        assert_eq!(packed.curves.len(), 3);
        for c in &packed.curves {
            assert_eq!(
                c.p1, c.p2,
                "a straight edge uses the duplicated-endpoint sentinel"
            );
        }
    }

    #[test]
    fn cubic_fill_is_accepted_where_the_glyph_lane_would_reject_it() {
        // The glyph encoder rejects cubics; the VectorPath lane subdivides them.
        let cubic = PlacedVectorPath {
            path: VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                    PathCommand::CubicTo {
                        control1: Vec2::new(0.0, 100.0),
                        control2: Vec2::new(100.0, 100.0),
                        to: Vec2::new(100.0, 0.0),
                    },
                    PathCommand::Close,
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            ..fill_triangle()
        };
        let placed = encode_fill(&cubic).expect("cubic fill encodes via subdivision");
        assert!(!placed.blob.curves.is_empty());
    }

    #[test]
    fn empty_and_non_finite_fills_are_skipped() {
        let empty = PlacedVectorPath {
            path: VectorPath::new(
                vec![PathCommand::MoveTo(Vec2::ZERO), PathCommand::Close],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            ..fill_triangle()
        };
        assert!(encode_fill(&empty).is_none(), "an empty path draws nothing");

        let nan = PlacedVectorPath {
            path: VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::ZERO),
                    PathCommand::LineTo(Vec2::new(f32::NAN, 1.0)),
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            ..fill_triangle()
        };
        assert!(encode_fill(&nan).is_none(), "a non-finite path is rejected");
    }

    #[test]
    fn stroke_lane_encodes_a_line_into_coverage_records() {
        let stroke = PlacedStroke {
            path: VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(50.0, 0.0)),
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            stroke: Stroke::new(4.0),
            origin_px: [0.0, 0.0],
            scale_px_per_unit: 1.0,
            color: [0.0, 0.0, 1.0, 1.0],
        };
        let placed = encode_stroke(&stroke).expect("stroked line encodes");
        let packed = pack(&[placed], 1.0);
        assert_eq!(packed.glyphs.len(), 1);
        // A butt-capped line strokes to a rectangle → four straight edges.
        assert!(!packed.curves.is_empty());
        assert!(!packed.bands.is_empty());
    }

    #[test]
    fn zero_width_stroke_is_skipped() {
        let stroke = PlacedStroke {
            path: VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(50.0, 0.0)),
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            ),
            stroke: Stroke::new(0.0),
            origin_px: [0.0, 0.0],
            scale_px_per_unit: 1.0,
            color: [0.0, 0.0, 1.0, 1.0],
        };
        assert!(
            encode_stroke(&stroke).is_none(),
            "a zero-width stroke paints nothing"
        );
    }

    #[test]
    fn fill_and_stroke_placements_carry_their_screen_transform() {
        // Encoding preserves the placement verbatim (origin/scale/colour) so a
        // path lane projects through the same GPU quad math as a glyph.
        let f = fill_triangle();
        let placed = encode_fill(&f).unwrap();
        assert_eq!(placed.origin_px, f.origin_px);
        assert_eq!(placed.scale_px_per_unit, f.scale_px_per_unit);
        assert_eq!(placed.color, f.color);
    }
}
