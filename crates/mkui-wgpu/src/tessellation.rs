//! Scene → triangle tessellation.
//!
//! Text rasterization is delegated to an [`Arc<dyn TextSystem>`] from
//! [`mkui_text`]; this module no longer owns the 5×7 bitmap table. The
//! tessellator asks the text system to lay out each `Text` primitive into
//! [`LayoutRun`]s, then rasterizes each glyph into an `Alpha` `GlyphImage`
//! and emits one quad per non-zero source pixel.

use std::f32::consts::PI;

use mkui_text::{
    BitmapTextSystem, FontId, GlyphFormat, HintingMode, LayoutSpec, TextAlign as MkuiTextAlign,
    TextSystem, VariationSettings,
};

use crate::types::{
    Color, CornerRadii, GuiTriangle, Icon, Insets, Point, Primitive, Quad, Rect, Scene, Shadow,
    Size, Text, TextAlign,
};

/// Number of straight-line segments approximating each 90° corner arc of a
/// rounded [`Quad`] (fill and stroke alike) — issue #212's "low-segment
/// triangle fan per corner". Enough to read as rounded at typical UI radii
/// (4-16px buttons/cards/badges) without inflating triangle counts for every
/// quad on screen; the zero-radius fast path below pays none of this cost.
const CORNER_ARC_SEGMENTS: usize = 8;

/// Round `value_px` to the nearest physical pixel at `device_pixel_ratio`,
/// then convert back to logical pixels. A local copy of the same snap used
/// by `mkui-vector2d-wgpu`'s Slug lane (Phase 3) — duplicated rather than
/// shared across crates because this module has no `slug`-feature
/// dependency on that crate, and the function is two lines.
fn snap_to_physical_pixel(value_px: f32, device_pixel_ratio: f32) -> f32 {
    (value_px * device_pixel_ratio).round() / device_pixel_ratio.max(f32::EPSILON)
}

pub fn tessellate_scene(scene: &Scene) -> Vec<GuiTriangle> {
    let system = BitmapTextSystem::new();
    tessellate_scene_with_text(scene, &system)
}

/// Tessellate `scene` using an explicit text system. Renderers that hold
/// their own `Arc<dyn TextSystem>` (so they can swap implementations
/// between sprints) call this directly rather than the convenience wrapper.
/// `device_pixel_ratio` is `1.0` (no logical/physical split) — callers with
/// a real per-frame DPR use [`tessellate_primitives`] directly, as the
/// windowed `Renderer::render` does.
pub fn tessellate_scene_with_text(scene: &Scene, text_system: &dyn TextSystem) -> Vec<GuiTriangle> {
    tessellate_primitives(&scene.primitives, text_system, 1.0)
}

/// Tessellate a contiguous slice of primitives. The ordered render path (#66)
/// calls this per [`crate::RenderCommand`] run so each command's geometry is
/// emitted in scene order; `tessellate_scene_with_text` is the whole-scene
/// convenience over the full primitive list.
///
/// `device_pixel_ratio` is the frame's physical-pixels-per-logical-pixel
/// ratio (`1.0` when the caller's pixel space has no logical/physical
/// split) — every bitmap glyph position is snapped to the device-pixel grid
/// (#157 Phase 4 variant B, Codex plan step 8: "snap every glyph to device
/// pixels" — unconditional, unlike Phase 3's Slug baseline snap, which
/// gates on a small-text threshold specifically to protect Phase 1/2's
/// large-text adapter-parity fixtures; the bitmap lane has no equivalent
/// large-text case to protect, since it carries no adapter-comparison
/// fixtures at all). This function re-tessellates fresh from the
/// declarative `Scene` on every call (the windowed renderer calls it once
/// per frame), so — unlike Phase 3's first, reverted cut of the Slug
/// baseline snap — there is no scene-construction-time caching for a DPI
/// change to go stale against.
pub fn tessellate_primitives(
    primitives: &[Primitive],
    text_system: &dyn TextSystem,
    device_pixel_ratio: f32,
) -> Vec<GuiTriangle> {
    let mut triangles = Vec::new();
    for primitive in primitives {
        match primitive {
            Primitive::Shadow(shadow) => tessellate_shadow(&mut triangles, *shadow),
            Primitive::Quad(quad) => tessellate_quad(&mut triangles, *quad),
            Primitive::Text(text) => {
                tessellate_text(&mut triangles, text, text_system, device_pixel_ratio)
            }
            Primitive::Icon(icon) => tessellate_icon(&mut triangles, icon),
            // Slug glyphs are not tessellated to triangles — they draw on the
            // Slug coverage lane (#66). The ordered render path routes them
            // there directly, so the triangle tessellator skips them.
            #[cfg(feature = "slug")]
            Primitive::SlugGlyph(_) => {}
            // Vector paths (#139) draw on the same Slug coverage lane, not
            // tessellated to triangles either.
            #[cfg(feature = "slug")]
            Primitive::VectorPath(_) => {}
        }
    }
    triangles
}

fn tessellate_shadow(triangles: &mut Vec<GuiTriangle>, shadow: Shadow) {
    let spread = shadow.spread.max(0.0) + shadow.blur_radius.max(0.0) * 0.35;
    push_rect(
        triangles,
        shadow.rect.expand(spread),
        shadow.color.multiply_alpha(0.6),
    );
}

fn tessellate_quad(triangles: &mut Vec<GuiTriangle>, quad: Quad) {
    let radii = clamp_corner_radii(quad.rect, quad.corner_radii);
    if corner_radii_are_zero(radii) {
        push_rect(triangles, quad.rect, quad.fill);
    } else {
        push_rounded_rect_fill(triangles, quad.rect, radii, quad.fill);
    }

    if let Some(stroke) = quad.stroke {
        let width = stroke.width.max(1.0);
        if corner_radii_are_zero(radii) {
            let top = Rect::new(quad.rect.origin, Size::new(quad.rect.size.width, width));
            let bottom = Rect::new(
                Point::new(quad.rect.origin.x, quad.rect.height_end() - width),
                Size::new(quad.rect.size.width, width),
            );
            let left = Rect::new(quad.rect.origin, Size::new(width, quad.rect.size.height));
            let right = Rect::new(
                Point::new(quad.rect.width_end() - width, quad.rect.origin.y),
                Size::new(width, quad.rect.size.height),
            );
            push_rect(triangles, top, stroke.color);
            push_rect(triangles, bottom, stroke.color);
            push_rect(triangles, left, stroke.color);
            push_rect(triangles, right, stroke.color);
        } else {
            push_rounded_rect_stroke(triangles, quad.rect, radii, width, stroke.color);
        }
    }
}

/// Clamp each corner radius to half the shorter rect dimension so opposing
/// arcs can never overlap — the per-corner generalization of the uniform
/// clamp `components::card`'s vector-path `rounded_rect_contour` applies to
/// its single `radius`.
fn clamp_corner_radii(rect: Rect, radii: CornerRadii) -> CornerRadii {
    let max_r = (rect.size.width.max(0.0).min(rect.size.height.max(0.0)) * 0.5).max(0.0);
    CornerRadii {
        top_left: radii.top_left.clamp(0.0, max_r),
        top_right: radii.top_right.clamp(0.0, max_r),
        bottom_right: radii.bottom_right.clamp(0.0, max_r),
        bottom_left: radii.bottom_left.clamp(0.0, max_r),
    }
}

fn corner_radii_are_zero(radii: CornerRadii) -> bool {
    radii.top_left <= 0.0
        && radii.top_right <= 0.0
        && radii.bottom_right <= 0.0
        && radii.bottom_left <= 0.0
}

/// Walk `rect`'s boundary clockwise (screen space, `y` down), replacing each
/// corner with a [`CORNER_ARC_SEGMENTS`]-segment circular arc of the
/// matching [`CornerRadii`] member. Point order matches [`push_rect`]'s
/// `top_left → top_right → bottom_right → bottom_left` winding. A zero
/// radius degenerates that corner's whole arc to the rect corner itself
/// (`center` coincides with the corner and `radius` is `0.0`), so the
/// straight-edge case needs no special-casing in the walk itself — only the
/// zero-triangle-count fast path in [`tessellate_quad`] above needs it, for
/// performance.
fn rounded_rect_boundary(rect: Rect, radii: CornerRadii, segments: usize) -> Vec<Point> {
    let x0 = rect.origin.x;
    let y0 = rect.origin.y;
    let x1 = rect.width_end();
    let y1 = rect.height_end();

    let mut points = Vec::with_capacity(4 * (segments + 1));
    // Left-edge tangent → top-edge tangent, sweeping through the actual
    // top-left corner (up and to the left of the arc's own center).
    push_corner_arc(
        &mut points,
        Point::new(x0 + radii.top_left, y0 + radii.top_left),
        radii.top_left,
        PI,
        1.5 * PI,
        segments,
    );
    push_corner_arc(
        &mut points,
        Point::new(x1 - radii.top_right, y0 + radii.top_right),
        radii.top_right,
        1.5 * PI,
        2.0 * PI,
        segments,
    );
    push_corner_arc(
        &mut points,
        Point::new(x1 - radii.bottom_right, y1 - radii.bottom_right),
        radii.bottom_right,
        0.0,
        0.5 * PI,
        segments,
    );
    push_corner_arc(
        &mut points,
        Point::new(x0 + radii.bottom_left, y1 - radii.bottom_left),
        radii.bottom_left,
        0.5 * PI,
        PI,
        segments,
    );
    points
}

/// Append one corner's arc — `segments + 1` points swept from `start_angle`
/// to `end_angle` (radians, standard `x`-right/`y`-down screen convention)
/// around `center` at `radius`. `radius <= 0.0` degenerates every point to
/// `center`, the sharp-corner case.
fn push_corner_arc(
    out: &mut Vec<Point>,
    center: Point,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    segments: usize,
) {
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = start_angle + (end_angle - start_angle) * t;
        out.push(Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
}

/// Fill a rounded rect by fanning triangles from its center out to each
/// boundary edge — valid since [`rounded_rect_boundary`] is always convex.
fn push_rounded_rect_fill(
    triangles: &mut Vec<GuiTriangle>,
    rect: Rect,
    radii: CornerRadii,
    color: Color,
) {
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || color.a <= 0.0 {
        return;
    }
    let boundary = rounded_rect_boundary(rect, radii, CORNER_ARC_SEGMENTS);
    let center = Point::new(
        rect.origin.x + rect.size.width * 0.5,
        rect.origin.y + rect.size.height * 0.5,
    );
    let n = boundary.len();
    for i in 0..n {
        triangles.push(GuiTriangle {
            points: [center, boundary[i], boundary[(i + 1) % n]],
            color,
        });
    }
}

/// Stroke a rounded rect as a ring between its outer boundary and an inner
/// boundary inset by `width` on every side (radii reduced by `width` too,
/// then re-clamped) — so the stroke bends around the same corners the fill
/// does, instead of the four straight edge strips the un-rounded case still
/// uses. Outer and inner boundaries share the same per-corner segment count
/// and angle sweep, so index `i` on one always corresponds to the same
/// angular position on the other.
fn push_rounded_rect_stroke(
    triangles: &mut Vec<GuiTriangle>,
    rect: Rect,
    radii: CornerRadii,
    width: f32,
    color: Color,
) {
    if color.a <= 0.0 || width <= 0.0 {
        return;
    }
    let inset_amount = width.min(rect.size.width * 0.5).min(rect.size.height * 0.5);
    let inner_rect = rect.inset(Insets::all(inset_amount));
    let inner_radii = clamp_corner_radii(
        inner_rect,
        CornerRadii {
            top_left: (radii.top_left - inset_amount).max(0.0),
            top_right: (radii.top_right - inset_amount).max(0.0),
            bottom_right: (radii.bottom_right - inset_amount).max(0.0),
            bottom_left: (radii.bottom_left - inset_amount).max(0.0),
        },
    );

    let outer = rounded_rect_boundary(rect, radii, CORNER_ARC_SEGMENTS);
    let inner = rounded_rect_boundary(inner_rect, inner_radii, CORNER_ARC_SEGMENTS);
    let n = outer.len();
    for i in 0..n {
        let j = (i + 1) % n;
        triangles.push(GuiTriangle {
            points: [outer[i], outer[j], inner[j]],
            color,
        });
        triangles.push(GuiTriangle {
            points: [outer[i], inner[j], inner[i]],
            color,
        });
    }
}

fn tessellate_text(
    triangles: &mut Vec<GuiTriangle>,
    text: &Text,
    system: &dyn TextSystem,
    device_pixel_ratio: f32,
) {
    let line_height = text.style.line_height_px.max(1.0);
    let max_lines = ((text.rect.size.height / line_height).floor() as usize).max(1);
    // The bitmap face is the only face this renderer drives in Sprint 7, so a
    // font handle is always the reserved bitmap identity. `FontId` is opaque
    // (no public raw constructor); callers re-resolve through the text system
    // rather than forging an id from `text.style.font`.
    let spec = LayoutSpec {
        font_id: FontId::BITMAP,
        font_generation: 0,
        font_size_px: text.style.font_size_px,
        line_height_px: line_height,
        align: map_align(text.style.align),
        max_lines: Some(max_lines),
        hinting: HintingMode::None,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
    };

    let runs = system.layout(&text.content, &spec, Some(text.rect.size.width));
    for run in &runs {
        for glyph in &run.glyphs {
            // Space (0x20) has an all-zero bitmap; skip the rasterize call
            // entirely so the renderer never allocates an empty image.
            if glyph.glyph_id == ' ' as u32 {
                continue;
            }
            let key = run.cache_key(glyph);
            let image = match system.rasterize(key) {
                Ok(image) => image,
                Err(_) => continue,
            };
            if image.format != GlyphFormat::Alpha {
                continue;
            }
            let base_x = text.rect.origin.x + run.origin_x_px + glyph.x_px + image.left_px as f32;
            let base_y = text.rect.origin.y + run.origin_y_px + glyph.y_px + image.top_px as f32;
            // #157 Phase 4 variant B: snap the whole glyph cell's origin as
            // one unit (not each output pixel independently) so its 5×7
            // raster shape is preserved exactly, just aligned to the device
            // pixel grid. Unconditional — every bitmap glyph snaps,
            // regardless of font size (Codex plan step 8's literal text;
            // Codex round 1 of this PR's review correctly rejected an
            // earlier cut that gated this on the same small-text threshold
            // Phase 3 uses for Slug, which would have left the demo's own
            // 16px label unsnapped).
            let base_x = snap_to_physical_pixel(base_x, device_pixel_ratio);
            let base_y = snap_to_physical_pixel(base_y, device_pixel_ratio);
            for oy in 0..image.height_px {
                for ox in 0..image.width_px {
                    let alpha = image.data[(oy * image.width_px + ox) as usize];
                    if alpha == 0 {
                        continue;
                    }
                    push_text_cell(
                        triangles,
                        Point::new(base_x + ox as f32, base_y + oy as f32),
                        1.0,
                        text.style.color.multiply_alpha(alpha as f32 / 255.0),
                    );
                }
            }
        }
    }
}

/// Deterministic pseudo-random value in `[0.0, 1.0)` derived from `seed` —
/// the splitmix64 finalizer's bit-mixing step, not used for anything
/// cryptographic, just to spread nearby/small integer seeds (the common
/// case while `IconId`s are hand-assigned small integers) across the output
/// range so they don't collide.
fn hash_unit(seed: u64) -> f32 {
    let mut x = seed;
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    ((x >> 40) as u32 as f32) / (1u32 << 24) as f32
}

/// Placeholder icon rendering: an `IconId`-tinted inset rect — **no icon
/// atlas or rasterization pipeline exists yet** ([`IconMask`] is defined in
/// `types.rs` but nothing constructs or consumes one), so this cannot draw
/// the icon's actual artwork. What it *can* do, and didn't before #191:
/// make the placeholder vary deterministically by `icon.icon` (size and a
/// small horizontal offset, both keyed off a hash of the id), so two
/// different icons in the same HUD read as visually distinct chips rather
/// than silently rendering the identical rect regardless of which icon was
/// requested. Real icon rasterization — reading host-supplied bytes per
/// #173's sandboxed-asset-loading constraint, atlas-packing them, wiring
/// `IconMask` through — is its own, currently untracked piece of work, not
/// something this placeholder fix attempts.
fn tessellate_icon(triangles: &mut Vec<GuiTriangle>, icon: &Icon) {
    let tint = icon.tint.multiply_alpha(0.9);
    let short_side = icon.rect.size.width.min(icon.rect.size.height);
    let spread = hash_unit(icon.icon.0);
    let inset_frac = 0.15 + spread * 0.25;
    let offset_frac = (spread - 0.5) * 0.3;

    let base = icon.rect.inset(Insets::all(short_side * inset_frac));
    let mark = Rect::new(
        Point::new(base.origin.x + short_side * offset_frac, base.origin.y),
        base.size,
    );
    push_rect(triangles, mark, tint);
}

pub(crate) fn map_align(align: TextAlign) -> MkuiTextAlign {
    match align {
        TextAlign::Start => MkuiTextAlign::Start,
        TextAlign::Center => MkuiTextAlign::Center,
        TextAlign::End => MkuiTextAlign::End,
    }
}

fn push_text_cell(triangles: &mut Vec<GuiTriangle>, origin: Point, size: f32, color: Color) {
    let rect = Rect::new(origin, Size::new(size, size));
    push_rect(
        triangles,
        rect.expand(size * 0.2),
        color.multiply_alpha(0.16),
    );
    push_rect(triangles, rect, color);
}

fn push_rect(triangles: &mut Vec<GuiTriangle>, rect: Rect, color: Color) {
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || color.a <= 0.0 {
        return;
    }

    let top_left = rect.origin;
    let top_right = Point::new(rect.width_end(), rect.origin.y);
    let bottom_left = Point::new(rect.origin.x, rect.height_end());
    let bottom_right = Point::new(rect.width_end(), rect.height_end());
    triangles.push(GuiTriangle {
        points: [top_left, top_right, bottom_right],
        color,
    });
    triangles.push(GuiTriangle {
        points: [top_left, bottom_right, bottom_left],
        color,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Scene, Size, Stroke, Text};

    #[test]
    fn different_icon_ids_tessellate_to_different_triangles() {
        // Regression (#191 review): `tessellate_icon` used to ignore
        // `icon.icon` entirely, so two chips requesting different icons
        // rendered byte-identical placeholder triangles — a counter chip
        // claiming to show "icon + count" couldn't actually show a
        // different icon. The placeholder is still not real icon artwork
        // (no atlas exists), but it must now vary by id.
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(20.0, 20.0));
        let tint = Color::rgba(1.0, 1.0, 1.0, 1.0);

        let mut a = Vec::new();
        tessellate_icon(
            &mut a,
            &Icon {
                rect,
                icon: crate::types::IconId(1),
                tint,
            },
        );
        let mut b = Vec::new();
        tessellate_icon(
            &mut b,
            &Icon {
                rect,
                icon: crate::types::IconId(2),
                tint,
            },
        );

        assert!(!a.is_empty());
        assert!(!b.is_empty());
        assert_ne!(
            a, b,
            "two different IconIds must not tessellate to identical triangles"
        );
    }

    #[test]
    fn same_icon_id_tessellates_identically_across_calls() {
        // The placeholder must still be deterministic — same id, same
        // output — so it stays stable across frames (no per-frame jitter).
        let rect = Rect::new(Point::new(4.0, 4.0), Size::new(16.0, 16.0));
        let icon = Icon {
            rect,
            icon: crate::types::IconId(42),
            tint: Color::rgba(0.5, 0.5, 0.5, 1.0),
        };

        let mut first = Vec::new();
        tessellate_icon(&mut first, &icon);
        let mut second = Vec::new();
        tessellate_icon(&mut second, &icon);

        assert_eq!(first, second);
    }

    #[test]
    fn text_tessellation_produces_triangles() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, 8.0), Size::new(220.0, 24.0)),
            content: "Undo Ctrl/Cmd+Z".to_string(),
            style: WgpuTheme::default().body_style(),
        });

        let triangles = tessellate_scene(&scene);
        assert!(!triangles.is_empty());
        assert!(triangles.iter().all(|triangle| triangle.color.a > 0.0));
    }

    #[test]
    fn empty_text_yields_no_triangles() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, 8.0), Size::new(220.0, 24.0)),
            content: String::new(),
            style: WgpuTheme::default().body_style(),
        });
        assert!(tessellate_scene(&scene).is_empty());
    }

    // ---- Phase 4 variant B: bitmap device-pixel snap (#157 step 8) --------

    // dame-rubric.md § Phase 4 variant B (N): the snap function itself,
    // tested in isolation with exact control over the sub-pixel sweep —
    // the same literal claim as Phase 3's Slug baseline snap ("100
    // sub-pixel offsets across one physical-pixel period group into
    // exactly 2 piecewise-constant cells, split at the period's midpoint").
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

    /// Tessellate a single "#" glyph at `font_size_px`, `rect_origin_y`, and
    /// `device_pixel_ratio`, returning the minimum Y coordinate across every
    /// emitted triangle vertex — a stand-in for the glyph cell's overall
    /// screen position that shifts in lockstep with `base_y` regardless of
    /// which specific bits are "on" in the raster.
    fn min_triangle_y(font_size_px: f32, rect_origin_y: f32, device_pixel_ratio: f32) -> f32 {
        let system = BitmapTextSystem::new();
        let mut style = WgpuTheme::default().body_style();
        style.font_size_px = font_size_px;
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, rect_origin_y), Size::new(220.0, 40.0)),
            content: "#".to_string(),
            style,
        });
        let triangles = tessellate_primitives(&scene.primitives, &system, device_pixel_ratio);
        assert!(
            !triangles.is_empty(),
            "\"#\" must tessellate to some triangles"
        );
        triangles
            .iter()
            .flat_map(|t| t.points.iter().map(|p| p.y))
            .fold(f32::INFINITY, f32::min)
    }

    // Integration-level proof that `tessellate_text` actually wires the snap
    // in (gated correctly) through the real `Scene` -> `tessellate_primitives`
    // path, rather than the math being correct in isolation but never reached.
    #[test]
    fn bitmap_position_snap_moves_in_quantized_physical_pixel_steps_at_every_font_size() {
        // Codex round 1 of the Phase 4 PR review: an earlier cut gated this
        // snap on the same small-text threshold Phase 3 uses for Slug, which
        // left the demo's own 16px label (`examples/text`) unsnapped — Codex
        // plan step 8 says "snap every glyph to device pixels", with no
        // threshold. Sweep several sizes, explicitly including the real
        // demo label size (16px), to prove there is no such gate anymore.
        let device_pixel_ratio = 2.0f32;
        for font_size_px in [12.0f32, 16.0, 20.0, 48.0] {
            let mut values = Vec::with_capacity(100);
            for i in 0..100 {
                let rect_origin_y = 8.0 + i as f32 * 0.05;
                values.push(min_triangle_y(
                    font_size_px,
                    rect_origin_y,
                    device_pixel_ratio,
                ));
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert!(
                distinct.len() < values.len(),
                "font_size_px {font_size_px}: bitmap text position must be quantized, \
                 not continuous ({} distinct of {})",
                distinct.len(),
                values.len()
            );
            for w in distinct.windows(2) {
                let step = w[1] - w[0];
                assert!(
                    (step - 1.0 / device_pixel_ratio).abs() < 1e-4,
                    "font_size_px {font_size_px}: quantization step {step} must equal \
                     one physical pixel (1/{device_pixel_ratio} logical px)"
                );
            }
        }
    }

    // ---- #212: rounded Quad corner tessellation ---------------------------

    fn fill_triangle_area(triangle: &GuiTriangle) -> f32 {
        let [a, b, c] = triangle.points;
        ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() * 0.5
    }

    #[test]
    fn zero_corner_radii_use_the_plain_rect_fast_path() {
        let quad = Quad {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(80.0, 40.0)),
            fill: Color::rgb(1.0, 0.0, 0.0),
            corner_radii: CornerRadii::all(0.0),
            stroke: Some(Stroke {
                color: Color::rgb(0.0, 1.0, 0.0),
                width: 2.0,
            }),
        };
        let mut triangles = Vec::new();
        tessellate_quad(&mut triangles, quad);
        // 2 for the plain rect fill, plus 2 each for the stroke's 4 straight
        // strips — byte-for-byte the pre-#212 behavior.
        assert_eq!(
            triangles.len(),
            2 + 4 * 2,
            "a zero-radius quad must still use the cheap plain-rect path"
        );
    }

    #[test]
    fn rounded_quad_fill_area_matches_the_analytic_rounded_rect_area() {
        let width = 80.0;
        let height = 40.0;
        let radius = 10.0;
        let quad = Quad {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(width, height)),
            fill: Color::rgb(1.0, 1.0, 1.0),
            corner_radii: CornerRadii::all(radius),
            stroke: None,
        };
        let mut triangles = Vec::new();
        tessellate_quad(&mut triangles, quad);

        // 4 corners * (segments + 1) boundary points, fanned into that many
        // triangles.
        assert_eq!(triangles.len(), 4 * (CORNER_ARC_SEGMENTS + 1));

        let area: f32 = triangles.iter().map(fill_triangle_area).sum();
        // width*height, minus the 4 corner squares the radius carves away,
        // plus the 4 quarter-circles it replaces them with. The faceted fan
        // under-approximates a true quarter-circle sector by a tiny, fixed
        // amount at this segment count, hence the generous-but-not-vacuous
        // tolerance below.
        let analytic = width * height - 4.0 * radius * radius + PI * radius * radius;
        let relative_error = (area - analytic).abs() / analytic;
        assert!(
            relative_error < 0.01,
            "faceted area {area} should track the analytic rounded-rect area \
             {analytic} within 1% (segment count {CORNER_ARC_SEGMENTS}), got \
             {relative_error}"
        );
    }

    #[test]
    fn oversized_corner_radii_are_clamped_to_half_the_shorter_side() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(20.0, 10.0));
        let radii = clamp_corner_radii(rect, CornerRadii::all(1000.0));
        let max_r = 5.0; // half of the shorter side (10.0)
        assert_eq!(radii, CornerRadii::all(max_r));

        let boundary = rounded_rect_boundary(rect, radii, CORNER_ARC_SEGMENTS);
        for p in &boundary {
            assert!(
                (-1e-3..=20.0 + 1e-3).contains(&p.x) && (-1e-3..=10.0 + 1e-3).contains(&p.y),
                "a clamped boundary point must stay within the rect, got {p:?}"
            );
        }
    }

    #[test]
    fn asymmetric_corner_radii_only_round_the_requested_corners() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0));
        let radii = CornerRadii {
            top_left: 10.0,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        };
        let boundary = rounded_rect_boundary(rect, radii, CORNER_ARC_SEGMENTS);
        // The top-right, bottom-right and bottom-left arcs all degenerate to
        // their rect corner; only the top-left arc sweeps away from (0, 0).
        assert!(boundary.contains(&Point::new(40.0, 0.0)));
        assert!(boundary.contains(&Point::new(40.0, 40.0)));
        assert!(boundary.contains(&Point::new(0.0, 40.0)));
        assert!(
            !boundary.contains(&Point::new(0.0, 0.0)),
            "a rounded top-left corner must never touch the exact (0, 0) corner point"
        );
    }

    #[test]
    fn rounded_quad_stroke_forms_a_ring_between_outer_and_inner_boundaries() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(80.0, 40.0));
        let radius = 10.0;
        let width = 4.0;
        let mut triangles = Vec::new();
        push_rounded_rect_stroke(
            &mut triangles,
            rect,
            CornerRadii::all(radius),
            width,
            Color::rgb(0.0, 0.0, 1.0),
        );

        let n = 4 * (CORNER_ARC_SEGMENTS + 1);
        assert_eq!(
            triangles.len(),
            2 * n,
            "one quad (2 triangles) per boundary segment"
        );

        let ring_area: f32 = triangles.iter().map(fill_triangle_area).sum();
        let outer_area = 80.0 * 40.0 - 4.0 * radius * radius + PI * radius * radius;
        let inner_radius = radius - width;
        let inner_radius_sq = inner_radius * inner_radius;
        let inner_w = 80.0 - 2.0 * width;
        let inner_h = 40.0 - 2.0 * width;
        let inner_area = inner_w * inner_h - 4.0 * inner_radius_sq + PI * inner_radius_sq;
        let analytic_ring = outer_area - inner_area;
        let relative_error = (ring_area - analytic_ring).abs() / analytic_ring;
        assert!(
            relative_error < 0.02,
            "ring area {ring_area} should track outer-minus-inner area {analytic_ring} \
             within 2%, got {relative_error}"
        );
    }
}
