//! Ordered render commands derived from `Scene::primitives` (#66).
//!
//! Sprint 5's renderer flattened the whole scene into one triangle stream and
//! issued a single draw. The Slug glyph lane (#66) needs a *second* pipeline
//! interleaved with the UI/bitmap triangles **without** reordering the scene's
//! semantic paint order — so a Slug glyph drawn after a panel must composite on
//! top of it, and a panel drawn after a glyph must cover it.
//!
//! [`build_render_commands`] walks `Scene::primitives` once and produces an
//! ordered [`RenderCommand`] list with these invariants:
//!
//! - **Paint order is preserved exactly.** The commands visit primitives in
//!   `Scene::primitives` index order; no global regrouping by pipeline or
//!   material is performed.
//! - **Only adjacent same-lane primitives coalesce.** A run of primitives that
//!   map to the same [`RenderLane`] becomes one command; the run ends the
//!   moment the lane changes. `UI, Slug, UI` therefore yields *three* commands,
//!   never two — a pipeline switch happens at every command boundary.
//! - **Each command is a contiguous half-open index range** into
//!   `Scene::primitives`, so the renderer can re-tessellate or re-pack exactly
//!   that slice.
//!
//! Alpha blending stays load/store compatible across commands: every command
//! draws into the same render pass with the same blend state, so a later
//! command composites over an earlier one (ADR 0006 §"single-pass ordered
//! lanes").

use std::ops::Range;

use crate::types::Primitive;

/// Which GPU pipeline / lane a primitive is drawn through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderLane {
    /// Tessellated UI geometry (quads, shadows, icons) on the UI triangle
    /// pipeline.
    UiTriangles,
    /// Bitmap-rasterized text, currently tessellated onto the UI triangle
    /// pipeline (its own command so a future bitmap-specific pipeline can split
    /// cleanly).
    BitmapText,
    /// Slug vector glyphs on the `mkui-vector2d-wgpu` coverage pipeline.
    SlugGlyphs,
    /// Arbitrary vector path fills/strokes (#139) on the same
    /// `mkui-vector2d-wgpu` coverage pipeline as [`RenderLane::SlugGlyphs`].
    VectorPath,
    /// Real backdrop blur (#187) on the dedicated `render::backdrop` pass —
    /// samples the frame itself, so (unlike every other lane) the renderer
    /// must close its open triangle render pass before drawing this command
    /// and reopen a fresh one after. Not feature-gated: glass chrome has no
    /// vector-pipeline dependency.
    Backdrop,
}

/// One ordered draw command: a lane plus the contiguous `Scene::primitives`
/// index range it covers. Variant payloads are the half-open primitive range so
/// the renderer can re-derive geometry for exactly that slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderCommand {
    /// UI triangles for `primitives[range]`.
    UiTriangles(Range<usize>),
    /// Bitmap text for `primitives[range]`.
    BitmapText(Range<usize>),
    /// Slug glyphs for `primitives[range]`.
    SlugGlyphs(Range<usize>),
    /// Vector path fills/strokes (#139) for `primitives[range]`.
    VectorPath(Range<usize>),
    /// Backdrop blur primitives (#187) for `primitives[range]`.
    Backdrop(Range<usize>),
}

impl RenderCommand {
    /// The lane this command draws through.
    pub fn lane(&self) -> RenderLane {
        match self {
            RenderCommand::UiTriangles(_) => RenderLane::UiTriangles,
            RenderCommand::BitmapText(_) => RenderLane::BitmapText,
            RenderCommand::SlugGlyphs(_) => RenderLane::SlugGlyphs,
            RenderCommand::VectorPath(_) => RenderLane::VectorPath,
            RenderCommand::Backdrop(_) => RenderLane::Backdrop,
        }
    }

    /// The half-open `Scene::primitives` index range this command covers.
    pub fn primitives(&self) -> Range<usize> {
        match self {
            RenderCommand::UiTriangles(r)
            | RenderCommand::BitmapText(r)
            | RenderCommand::SlugGlyphs(r)
            | RenderCommand::VectorPath(r)
            | RenderCommand::Backdrop(r) => r.clone(),
        }
    }

    fn from_lane(lane: RenderLane, range: Range<usize>) -> Self {
        match lane {
            RenderLane::UiTriangles => RenderCommand::UiTriangles(range),
            RenderLane::BitmapText => RenderCommand::BitmapText(range),
            RenderLane::SlugGlyphs => RenderCommand::SlugGlyphs(range),
            RenderLane::VectorPath => RenderCommand::VectorPath(range),
            RenderLane::Backdrop => RenderCommand::Backdrop(range),
        }
    }
}

/// Default lane assignment: text routes to [`RenderLane::BitmapText`], every
/// other primitive to [`RenderLane::UiTriangles`].
///
/// This is the #66 native default — there is no Slug font provider yet (that is
/// #67), so no scene primitive classifies as [`RenderLane::SlugGlyphs`] through
/// this function. Callers that own a composite text system able to resolve a
/// Slug-class face pass their own classifier to [`build_render_commands`].
pub fn classify_primitive(primitive: &Primitive) -> RenderLane {
    match primitive {
        Primitive::Text(_) => RenderLane::BitmapText,
        #[cfg(feature = "slug")]
        Primitive::SlugGlyph(_) => RenderLane::SlugGlyphs,
        #[cfg(feature = "slug")]
        Primitive::VectorPath(_) => RenderLane::VectorPath,
        // Tessellated UI geometry — listed explicitly (not a wildcard) so a
        // future `Primitive` variant that forgets a match arm here fails to
        // compile instead of silently tessellating as a triangle (the #119
        // stubbed-seam bug class this issue's exhaustive-match discipline
        // guards against).
        Primitive::Shadow(_) | Primitive::Quad(_) | Primitive::Icon(_) => RenderLane::UiTriangles,
        // Real backdrop blur (#187) always gets its own lane — unlike
        // Slug/VectorPath this is not feature-gated, since it has no
        // vector-pipeline dependency, just a dedicated GPU pass.
        Primitive::Backdrop(_) => RenderLane::Backdrop,
    }
}

/// Build the ordered command list for `primitives`, assigning each primitive a
/// lane via `classify` and coalescing only **adjacent** same-lane primitives.
///
/// The result preserves `primitives` order exactly and never regroups globally
/// by lane — see the module docs for the full invariant list.
pub fn build_render_commands(
    primitives: &[Primitive],
    classify: impl Fn(&Primitive) -> RenderLane,
) -> Vec<RenderCommand> {
    let mut commands = Vec::new();
    let mut run_start = 0usize;
    let mut run_lane: Option<RenderLane> = None;

    for (idx, primitive) in primitives.iter().enumerate() {
        let lane = classify(primitive);
        match run_lane {
            Some(current) if current == lane => {
                // Same lane as the open run — extend it.
            }
            Some(current) => {
                // Lane changed: close the open run, open a fresh one. This is
                // the pipeline-switch boundary.
                commands.push(RenderCommand::from_lane(current, run_start..idx));
                run_start = idx;
                run_lane = Some(lane);
            }
            None => {
                run_start = idx;
                run_lane = Some(lane);
            }
        }
    }

    if let Some(current) = run_lane {
        commands.push(RenderCommand::from_lane(
            current,
            run_start..primitives.len(),
        ));
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Color, CornerRadii, Point, Quad, Rect, Size, Text};

    fn quad() -> Primitive {
        Primitive::Quad(Quad {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(10.0, 10.0)),
            fill: Color::rgba(1.0, 1.0, 1.0, 1.0),
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        })
    }

    fn text() -> Primitive {
        Primitive::Text(Text {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(50.0, 10.0)),
            content: "hi".to_string(),
            style: WgpuTheme::default().body_style(),
        })
    }

    #[test]
    fn empty_scene_yields_no_commands() {
        assert!(build_render_commands(&[], classify_primitive).is_empty());
    }

    #[test]
    fn single_lane_run_coalesces_into_one_command() {
        let prims = vec![quad(), quad(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(cmds, vec![RenderCommand::UiTriangles(0..3)]);
    }

    #[test]
    fn adjacent_text_coalesces_separately_from_quads() {
        let prims = vec![quad(), text(), text(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::BitmapText(1..3),
                RenderCommand::UiTriangles(3..4),
            ]
        );
    }

    #[test]
    fn non_adjacent_same_lane_is_not_globally_regrouped() {
        // UI, Slug, UI must stay three commands — never merged into two by
        // pulling the two UI runs together. This is the core no-global-regroup
        // invariant that keeps paint order intact.
        let prims = vec![quad(), text(), quad()];
        let classify = |p: &Primitive| match p {
            Primitive::Text(_) => RenderLane::SlugGlyphs,
            _ => RenderLane::UiTriangles,
        };
        let cmds = build_render_commands(&prims, classify);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::SlugGlyphs(1..2),
                RenderCommand::UiTriangles(2..3),
            ]
        );
    }

    #[test]
    fn commands_cover_every_primitive_in_order() {
        let prims = vec![quad(), text(), quad(), quad(), text()];
        let cmds = build_render_commands(&prims, classify_primitive);
        // The concatenated ranges must tile [0, len) with no gaps or overlap.
        let mut next = 0;
        for cmd in &cmds {
            let r = cmd.primitives();
            assert_eq!(r.start, next, "ranges must be contiguous and ordered");
            next = r.end;
        }
        assert_eq!(next, prims.len(), "ranges must cover every primitive");
    }

    #[test]
    fn lane_and_primitives_accessors_agree_with_variant() {
        let cmd = RenderCommand::SlugGlyphs(2..5);
        assert_eq!(cmd.lane(), RenderLane::SlugGlyphs);
        assert_eq!(cmd.primitives(), 2..5);

        let cmd = RenderCommand::VectorPath(3..7);
        assert_eq!(cmd.lane(), RenderLane::VectorPath);
        assert_eq!(cmd.primitives(), 3..7);
    }

    #[test]
    fn classify_routes_text_to_bitmap_and_rest_to_ui() {
        assert_eq!(classify_primitive(&text()), RenderLane::BitmapText);
        assert_eq!(classify_primitive(&quad()), RenderLane::UiTriangles);
    }

    fn backdrop() -> Primitive {
        Primitive::Backdrop(crate::types::Backdrop {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0)),
            corner_radii: CornerRadii::all(8.0),
            blur_radius: 24.0,
            tint: Color::rgba(0.0, 0.0, 0.0, 0.4),
        })
    }

    #[test]
    fn classify_routes_backdrop_to_its_own_lane() {
        assert_eq!(classify_primitive(&backdrop()), RenderLane::Backdrop);
    }

    #[test]
    fn backdrop_primitive_routes_to_its_own_lane_preserving_paint_order() {
        // UI, Backdrop, UI — same no-global-regroup invariant as the Slug
        // glyph / vector path lanes above, and not feature-gated.
        let prims = vec![quad(), backdrop(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::Backdrop(1..2),
                RenderCommand::UiTriangles(2..3),
            ]
        );
    }

    #[test]
    fn backdrop_lane_and_primitives_accessors_agree_with_variant() {
        let cmd = RenderCommand::Backdrop(4..9);
        assert_eq!(cmd.lane(), RenderLane::Backdrop);
        assert_eq!(cmd.primitives(), 4..9);
    }

    #[cfg(feature = "slug")]
    #[test]
    fn slug_glyph_primitive_routes_to_slug_lane_preserving_paint_order() {
        use mkui_vector2d::Vec2;
        use mkui_vector2d_wgpu::{GlyphBounds, PlacedSlugGlyph, SlugCurve, SlugGlyph};
        use std::sync::Arc;

        let slug = || {
            Primitive::SlugGlyph(PlacedSlugGlyph {
                blob: Arc::new(SlugGlyph {
                    revision: 1,
                    bounds: GlyphBounds {
                        x_min: 0.0,
                        y_min: 0.0,
                        x_max: 1.0,
                        y_max: 1.0,
                    },
                    curves: vec![SlugCurve {
                        p0: Vec2::new(0.0, 0.0),
                        p1: Vec2::new(1.0, 1.0),
                        p2: Vec2::new(1.0, 1.0),
                    }],
                    horizontal_bands: Vec::new(),
                    horizontal_curve_indices: Vec::new(),
                    vertical_bands: Vec::new(),
                    vertical_curve_indices: Vec::new(),
                }),
                origin_px: [0.0, 0.0],
                scale_px_per_unit: 1.0,
                color: [1.0, 1.0, 1.0, 1.0],
                cap_height_px: f32::INFINITY,
            })
        };

        // UI, Slug, UI — the real scene-carried variant must produce three
        // ordered commands on two distinct lanes, never collapsing the two UI
        // runs across the Slug command.
        let prims = vec![quad(), slug(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::SlugGlyphs(1..2),
                RenderCommand::UiTriangles(2..3),
            ]
        );
    }

    #[cfg(feature = "slug")]
    fn vector_path() -> Primitive {
        use crate::types::PlacedVectorPath;
        use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
        use std::sync::Arc;

        Primitive::VectorPath(PlacedVectorPath {
            path: Arc::new(VectorPath::new(
                vec![
                    PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(1.0, 0.0)),
                    PathCommand::LineTo(Vec2::new(0.0, 1.0)),
                    PathCommand::Close,
                ],
                FillRule::NonZero,
                Bounds::new(Vec2::ZERO, Vec2::ZERO),
            )),
            stroke: None,
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            fill_color: Some(Color::rgba(1.0, 1.0, 1.0, 1.0)),
            origin_px: [0.0, 0.0],
            scale_px_per_unit: 1.0,
        })
    }

    #[cfg(feature = "slug")]
    #[test]
    fn vector_path_primitive_routes_to_vector_path_lane_preserving_paint_order() {
        // UI, VectorPath, UI — same no-global-regroup invariant as the Slug
        // glyph test above, proven for the #139 lane.
        let prims = vec![quad(), vector_path(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::VectorPath(1..2),
                RenderCommand::UiTriangles(2..3),
            ]
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn vector_path_and_slug_glyph_are_distinct_adjacent_lanes() {
        use mkui_vector2d::Vec2;
        use mkui_vector2d_wgpu::{GlyphBounds, PlacedSlugGlyph, SlugCurve, SlugGlyph};
        use std::sync::Arc;

        let slug = Primitive::SlugGlyph(PlacedSlugGlyph {
            blob: Arc::new(SlugGlyph {
                revision: 1,
                bounds: GlyphBounds {
                    x_min: 0.0,
                    y_min: 0.0,
                    x_max: 1.0,
                    y_max: 1.0,
                },
                curves: vec![SlugCurve {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(1.0, 1.0),
                    p2: Vec2::new(1.0, 1.0),
                }],
                horizontal_bands: Vec::new(),
                horizontal_curve_indices: Vec::new(),
                vertical_bands: Vec::new(),
                vertical_curve_indices: Vec::new(),
            }),
            origin_px: [0.0, 0.0],
            scale_px_per_unit: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            cap_height_px: f32::INFINITY,
        });

        // UI, VectorPath, SlugGlyph, VectorPath, UI — five ordered commands on
        // three distinct lanes. Two different Slug-pipeline lanes (VectorPath,
        // SlugGlyphs) sitting adjacent to each other must still split into
        // separate commands — same pipeline is not a reason to coalesce
        // across a lane boundary (only *identical* adjacent lanes coalesce).
        let prims = vec![quad(), vector_path(), slug.clone(), vector_path(), quad()];
        let cmds = build_render_commands(&prims, classify_primitive);
        assert_eq!(
            cmds,
            vec![
                RenderCommand::UiTriangles(0..1),
                RenderCommand::VectorPath(1..2),
                RenderCommand::SlugGlyphs(2..3),
                RenderCommand::VectorPath(3..4),
                RenderCommand::UiTriangles(4..5),
            ]
        );

        // Non-adjacent same-lane runs (VectorPath, Slug, VectorPath) must not
        // be globally regrouped into one VectorPath command either.
        let prims_ab_a = vec![vector_path(), slug, vector_path()];
        let cmds_ab_a = build_render_commands(&prims_ab_a, classify_primitive);
        assert_eq!(
            cmds_ab_a,
            vec![
                RenderCommand::VectorPath(0..1),
                RenderCommand::SlugGlyphs(1..2),
                RenderCommand::VectorPath(2..3),
            ]
        );
    }
}
