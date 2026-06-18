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
}

impl RenderCommand {
    /// The lane this command draws through.
    pub fn lane(&self) -> RenderLane {
        match self {
            RenderCommand::UiTriangles(_) => RenderLane::UiTriangles,
            RenderCommand::BitmapText(_) => RenderLane::BitmapText,
            RenderCommand::SlugGlyphs(_) => RenderLane::SlugGlyphs,
        }
    }

    /// The half-open `Scene::primitives` index range this command covers.
    pub fn primitives(&self) -> Range<usize> {
        match self {
            RenderCommand::UiTriangles(r)
            | RenderCommand::BitmapText(r)
            | RenderCommand::SlugGlyphs(r) => r.clone(),
        }
    }

    fn from_lane(lane: RenderLane, range: Range<usize>) -> Self {
        match lane {
            RenderLane::UiTriangles => RenderCommand::UiTriangles(range),
            RenderLane::BitmapText => RenderCommand::BitmapText(range),
            RenderLane::SlugGlyphs => RenderCommand::SlugGlyphs(range),
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
        _ => RenderLane::UiTriangles,
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
            style: WgpuTheme::default().body_style,
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
    }

    #[test]
    fn classify_routes_text_to_bitmap_and_rest_to_ui() {
        assert_eq!(classify_primitive(&text()), RenderLane::BitmapText);
        assert_eq!(classify_primitive(&quad()), RenderLane::UiTriangles);
    }
}
