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

/// Pixels of dilation added to every edge of a glyph's screen quad so the
/// anti-aliasing footprint at the ink boundary is not clipped by the quad.
const QUAD_DILATION_PX: f32 = 1.5;

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
    /// Screen-pixel position (y-down) of the glyph's font-unit origin.
    pub origin_px: [f32; 2],
    /// Pixels per font unit (placement scale).
    pub scale_px_per_unit: f32,
    /// Straight-alpha RGBA fill colour in `[0, 1]`.
    pub color: [f32; 4],
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

/// GPU per-glyph instance matching the WGSL `Glyph` struct (std430), 64 bytes.
/// Field order is chosen so the `vec4` colour leads and every member is
/// naturally aligned — `repr(C)` then matches std430 with no manual padding.
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
/// streams plus one [`GpuGlyph`] instance per drawable glyph.
pub(crate) fn pack(glyphs: &[PlacedSlugGlyph]) -> PackedSlug {
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

        // Dilated screen quad. Font units are y-up; screen is y-down, so the
        // glyph's y_max maps to the smaller (top) screen y.
        let [ox, oy] = placed.origin_px;
        let left = ox + blob.bounds.x_min * scale - QUAD_DILATION_PX;
        let right = ox + blob.bounds.x_max * scale + QUAD_DILATION_PX;
        let top = oy - blob.bounds.y_max * scale - QUAD_DILATION_PX;
        let bottom = oy - blob.bounds.y_min * scale + QUAD_DILATION_PX;

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
    /// (the logical-pixel viewport the quads are projected against). Returns
    /// `None` when there is nothing drawable (no glyph carried any curves) so
    /// the caller can skip the draw entirely. Borrows device + queue only.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_px: [f32; 2],
        glyphs: &[PlacedSlugGlyph],
    ) -> Option<PreparedSlug> {
        self.prepare_packed(device, queue, viewport_px, pack(glyphs))
    }

    /// Encode and prepare arbitrary [`VectorPath`] fills and [`PlacedStroke`]s
    /// for `viewport_px`, reusing the exact glyph coverage pipeline. Each fill is
    /// lowered via `encode_vector_path` and each stroke via `stroke_to_fill` +
    /// `encode_vector_path` into the shared [`SlugGlyph`] records, then packed and
    /// uploaded identically to a glyph frame. Items that paint nothing (empty,
    /// non-finite, or a non-positive stroke width) are skipped; returns `None`
    /// when nothing is drawable. Borrows device + queue only.
    pub fn prepare_paths(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_px: [f32; 2],
        fills: &[PlacedVectorPath],
        strokes: &[PlacedStroke],
    ) -> Option<PreparedSlug> {
        let placed: Vec<PlacedSlugGlyph> = fills
            .iter()
            .filter_map(encode_fill)
            .chain(strokes.iter().filter_map(encode_stroke))
            .collect();
        self.prepare_packed(device, queue, viewport_px, pack(&placed))
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
        }
    }

    #[test]
    fn pack_emits_one_instance_per_drawable_glyph() {
        let packed = pack(&[placed(triangle_blob()), placed(triangle_blob())]);
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
        let single = pack(&[placed(blob.clone())]);
        let double = pack(&[placed(blob.clone()), placed(blob.clone())]);

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
        let packed = pack(&[placed(blob.clone())]);
        // Curve records are copied 1:1 from the neutral blob (no reinterpretation).
        for (gpu, src) in packed.curves.iter().zip(blob.curves.iter()) {
            assert_eq!(gpu.p0, [src.p0.x, src.p0.y]);
            assert_eq!(gpu.p1, [src.p1.x, src.p1.y]);
            assert_eq!(gpu.p2, [src.p2.x, src.p2.y]);
        }
        assert_eq!(packed.indices, blob.horizontal_curve_indices);
        assert_eq!(packed.bands.len(), blob.horizontal_bands.len());
        assert_eq!(
            packed.glyphs[0].band_count,
            blob.horizontal_bands.len() as u32
        );
    }

    #[test]
    fn dilated_quad_brackets_the_scaled_glyph_bounds() {
        let p = placed(triangle_blob());
        let packed = pack(std::slice::from_ref(&p));
        let g = packed.glyphs[0];
        let scale = p.scale_px_per_unit;
        // x: origin + bounds*scale, dilated outward by QUAD_DILATION_PX.
        assert_eq!(
            g.quad_min[0],
            p.origin_px[0] + 0.0 * scale - QUAD_DILATION_PX
        );
        assert_eq!(
            g.quad_max[0],
            p.origin_px[0] + 100.0 * scale + QUAD_DILATION_PX
        );
        // y is flipped: font y_max → top (smaller screen y) → quad_min.y.
        assert_eq!(
            g.quad_min[1],
            p.origin_px[1] - 100.0 * scale - QUAD_DILATION_PX
        );
        assert_eq!(
            g.quad_max[1],
            p.origin_px[1] - 0.0 * scale + QUAD_DILATION_PX
        );
        assert!(
            g.quad_min[1] < g.quad_max[1],
            "top must be above bottom in y-down screen space"
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
        let packed = pack(&[placed(empty)]);
        assert!(packed.glyphs.is_empty(), "a curve-less glyph emits no quad");
    }

    #[test]
    fn gpu_structs_have_the_std430_byte_sizes() {
        // Lock the layout the WGSL std430 structs assume.
        assert_eq!(std::mem::size_of::<GpuCurve>(), 24);
        assert_eq!(std::mem::size_of::<GpuBand>(), 16);
        assert_eq!(std::mem::size_of::<GpuGlyph>(), 64);
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
        let packed = pack(&[placed]);
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
        let packed = pack(&[placed]);
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
