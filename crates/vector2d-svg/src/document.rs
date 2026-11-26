//! Tree walk: roxmltree DOM → sanitized-subset allowlisting → a flat,
//! render-ready [`Document`] of [`mkui_vector2d::VectorPath`]s (#201).
//!
//! ## XML safety
//!
//! `roxmltree::ParsingOptions { allow_dtd: false, .. }` already refuses a
//! `DOCTYPE` outright; [`parse_svg`] also pre-scans for the literal
//! `<!DOCTYPE` token so the rejection is a typed [`crate::SvgError`] rather
//! than an opaque XML parse failure either way. Beyond that: every
//! processing instruction, every non-whitespace text node, every attribute
//! in a foreign namespace (e.g. `xlink:href`), and every element outside
//! the allowlist below is rejected while walking the tree — nothing is
//! silently dropped or guessed at.
//!
//! ## Two passes
//!
//! 1. `collect_gradients` recursively finds every `<linearGradient>`/
//!    `<radialGradient>` (anywhere in the document, not only inside
//!    `<defs>` — SVG allows either) so a shape may forward-reference a
//!    gradient defined later in document order. It fully validates those
//!    elements' own attributes/children/text/PI content — `walk_shapes`
//!    skips back over any gradient subtree it meets without re-checking it.
//! 2. `walk_shapes` walks the renderable tree (`svg`/`g`/`defs`/shapes),
//!    tracking inherited `fill`/`stroke`/accumulated opacity/accumulated
//!    CTM, and lowers each shape leaf to a [`Shape`].
//!
//! ## Scope decisions (see the issue's literal attribute list: `fill stroke
//! opacity transform`)
//!
//! - `stroke-width` is **not** an accepted attribute — every stroke uses
//!   [`mkui_vector2d::Stroke::default`] (SVG's own default: 1px, butt cap,
//!   miter join). A document that sets `stroke-width` explicitly is
//!   rejected as outside the subset, not silently ignored.
//! - `fill-rule` is **not** accepted either — every fill uses
//!   [`mkui_vector2d::FillRule::NonZero`] (also the SVG default). The issue
//!   only commits to covering both rules "if `fill-rule` is added to the
//!   subset"; it wasn't, so this stays out. Both are candidates for a
//!   follow-up issue if a representative asset needs them.
//! - `gradientUnits`/`gradientTransform` are not accepted — every gradient
//!   is `objectBoundingBox` (the SVG default), resolved against the
//!   painted shape's own bounding box in `resolve_gradient`.
//! - Group `opacity` is approximated as multiplicative per-descendant alpha
//!   (accumulated down the tree and folded into each leaf's resolved paint
//!   color), not true offscreen group compositing. Overlapping same-group
//!   siblings will look slightly different from a spec-exact renderer;
//!   documented here rather than silently assumed.
//! - Gradient rendering itself is not wired to any GPU adapter yet —
//!   `PlacedVectorPath`/`PlacedStroke` (`mkui-wgpu`) only carry a solid
//!   `Color`. This crate still parses and resolves gradients into
//!   [`crate::gradient::Gradient`] data so the acceptance criterion's
//!   "every element/attribute in the subset" is met at the parser level;
//!   GPU gradient shading is deferred to a follow-up issue.

use std::collections::{HashMap, HashSet};

use mkui_vector2d::{Affine2, Bounds, FillRule, PathCommand, Stroke, Vec2, VectorPath};

use crate::color::Color;
use crate::error::SvgError;
use crate::gradient::{normalize_stops, Gradient, GradientKind, Stop};
use crate::number::{parse_number_list, parse_number_or_percent};
use crate::paint::{parse_paint_ref, Paint, PaintRef};
use crate::path_data::parse_path_data;
use crate::transform::{compose, parse_transform_list};

const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// Anti-DoS cap on total XML node count (`roxmltree::ParsingOptions::nodes_limit`).
/// Generous for any real icon/portrait asset, small enough to bound parse
/// work on hostile input.
const MAX_XML_NODES: u32 = 100_000;

/// Anti-stack-overflow cap on `<g>`/`<defs>`/gradient nesting depth. Both
/// [`collect_gradients`] and [`walk_shapes`] recurse once per level with no
/// other bound — [`MAX_XML_NODES`] caps breadth, not depth, so a document
/// that is one long `<g><g><g>...` chain needs its own explicit limit.
/// 256 comfortably exceeds any plausible authored nesting.
const MAX_NESTING_DEPTH: u32 = 256;

/// A parsed, render-ready sanitized-subset document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The root `<svg>` element's declared size/viewBox, if any — carried
    /// through for a host's own placement math; this crate applies no
    /// implicit viewBox→viewport scale itself (no `preserveAspectRatio`
    /// support, see the module docs).
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub view_box: Option<ViewBox>,
    /// Every renderable shape, in document order. A shape inside `<defs>`
    /// is validated but never appears here.
    pub shapes: Vec<Shape>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    pub min_x: f32,
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
}

/// One lowered, paintable shape. `path` is already in
/// `mkui-vector2d`'s y-up convention — see the crate-level y-flip note in
/// [`crate::transform`].
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub path: VectorPath,
    pub fill: Option<Paint>,
    pub stroke: Option<(Stroke, Paint)>,
}

/// Parse a sanitized-subset SVG document.
pub fn parse_svg(xml: &str) -> Result<Document, SvgError> {
    if xml.contains("<!DOCTYPE") {
        return Err(SvgError::DoctypeRejected);
    }
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        // Anti-DoS cap on total node count — same spirit as the recursion
        // depth cap below, but for breadth (a flat forest of attribute-free
        // siblings costs no stack depth at all).
        nodes_limit: MAX_XML_NODES,
        ..roxmltree::ParsingOptions::default()
    };
    let xml_doc = roxmltree::Document::parse_with_options(xml, opts)
        .map_err(|e| SvgError::Xml(e.to_string()))?;
    // Screen the *document* node's own children for a prolog/epilog PI or
    // stray text — `<?xml-stylesheet ...?>` before `<svg>` is the common
    // position for this, not inside the root element itself.
    check_non_element_content(xml_doc.root())?;
    let root = xml_doc.root_element();
    check_element(root, "svg")?;

    let mut seen_ids = HashSet::new();
    let mut gradients = HashMap::new();
    collect_gradients(root, 0, &mut gradients, &mut seen_ids)?;

    let width = root
        .attribute("width")
        .map(|v| parse_length(v, "svg", "width"))
        .transpose()?;
    let height = root
        .attribute("height")
        .map(|v| parse_length(v, "svg", "height"))
        .transpose()?;
    let view_box = root
        .attribute("viewBox")
        .map(parse_view_box)
        .transpose()?
        .flatten();

    let mut shapes = Vec::new();
    let state = Inherited {
        fill: PaintRef::Solid(Color::BLACK),
        stroke: PaintRef::None,
        ctm: Affine2::IDENTITY,
        opacity: 1.0,
        in_defs: false,
    };
    walk_shapes(root, 0, &state, &gradients, &mut seen_ids, &mut shapes)?;

    Ok(Document {
        width,
        height,
        view_box,
        shapes,
    })
}

// ---- element/attribute allowlisting -------------------------------------

fn local_name<'a>(node: roxmltree::Node<'a, 'a>) -> &'a str {
    node.tag_name().name()
}

/// Reject a foreign-namespace element, or an element name outside the
/// subset. `expected` is used only by the root check; interior elements are
/// matched against [`allowed_attrs`]'s key set via [`allowed_attrs`] itself
/// returning `None` for an unknown name.
fn check_element<'a>(node: roxmltree::Node<'a, 'a>, expected: &str) -> Result<&'a str, SvgError> {
    let ns_ok = matches!(node.tag_name().namespace(), None | Some(SVG_NS));
    let name = local_name(node);
    if !ns_ok || name != expected {
        return Err(SvgError::UnsupportedElement(name.to_string()));
    }
    Ok(name)
}

fn allowed_attrs(element: &str) -> Option<&'static [&'static str]> {
    const PRESENTATION: &[&str] = &["id", "fill", "stroke", "opacity", "transform"];
    Some(match element {
        "svg" => &[
            "id",
            "fill",
            "stroke",
            "opacity",
            "transform",
            "width",
            "height",
            "viewBox",
        ],
        "g" | "defs" => PRESENTATION,
        "path" => &["id", "fill", "stroke", "opacity", "transform", "d"],
        "circle" => &[
            "id",
            "fill",
            "stroke",
            "opacity",
            "transform",
            "cx",
            "cy",
            "r",
        ],
        "ellipse" => &[
            "id",
            "fill",
            "stroke",
            "opacity",
            "transform",
            "cx",
            "cy",
            "rx",
            "ry",
        ],
        "rect" => &[
            "id",
            "fill",
            "stroke",
            "opacity",
            "transform",
            "x",
            "y",
            "width",
            "height",
        ],
        "line" => &[
            "id",
            "fill",
            "stroke",
            "opacity",
            "transform",
            "x1",
            "y1",
            "x2",
            "y2",
        ],
        "polyline" | "polygon" => &["id", "fill", "stroke", "opacity", "transform", "points"],
        "linearGradient" => &["id", "x1", "y1", "x2", "y2"],
        "radialGradient" => &["id", "cx", "cy", "r"],
        "stop" => &["offset", "stop-color"],
        _ => return None,
    })
}

/// Validate every attribute on `node` against `element`'s allowlist and
/// reject any attribute carrying a namespace (`xlink:href`, `xml:space`,
/// ...) outright — none of the subset's attributes are namespaced.
fn check_attributes(node: roxmltree::Node, element: &str) -> Result<(), SvgError> {
    let allowed = allowed_attrs(element).expect("caller already matched a known element");
    for attr in node.attributes() {
        if attr.namespace().is_some() || !allowed.contains(&attr.name()) {
            return Err(SvgError::UnsupportedAttribute {
                element: element.to_string(),
                attr: attr.name().to_string(),
            });
        }
    }
    Ok(())
}

/// Reject a PI/non-whitespace-text child of `node`; comments are ignored.
/// Does not descend into element children — callers do that themselves
/// (this only screens the *non-element* siblings at one level).
fn check_non_element_content(node: roxmltree::Node) -> Result<(), SvgError> {
    for child in node.children() {
        if child.is_element() {
            continue;
        }
        if child.is_pi() {
            return Err(SvgError::ProcessingInstructionRejected);
        }
        if child.is_text() && !child.text().unwrap_or("").trim().is_empty() {
            return Err(SvgError::TextContentRejected);
        }
        // Comments (and any other non-element, non-PI, non-text node kind)
        // carry no executable content and are silently accepted.
    }
    Ok(())
}

// ---- pass 1: gradients ----------------------------------------------------

#[derive(Debug, Clone)]
struct RawGradient {
    kind: RawKind,
    stops: Vec<Stop>,
}

#[derive(Debug, Clone, Copy)]
enum RawKind {
    Linear { x1: f32, y1: f32, x2: f32, y2: f32 },
    Radial { cx: f32, cy: f32, r: f32 },
}

fn collect_gradients(
    node: roxmltree::Node,
    depth: u32,
    out: &mut HashMap<String, RawGradient>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), SvgError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(SvgError::NestingTooDeep);
    }
    for child in node.children().filter(|c| c.is_element()) {
        let name = local_name(child);
        if !matches!(child.tag_name().namespace(), None | Some(SVG_NS)) {
            // Not a gradient and not our concern here — `walk_shapes` will
            // reject the foreign element itself.
            collect_gradients(child, depth + 1, out, seen_ids)?;
            continue;
        }
        match name {
            "linearGradient" | "radialGradient" => {
                let raw = parse_gradient_element(child, name)?;
                if let Some(id) = child.attribute("id") {
                    if !seen_ids.insert(id.to_string()) {
                        return Err(SvgError::DuplicateId(id.to_string()));
                    }
                    out.insert(id.to_string(), raw);
                }
            }
            _ => collect_gradients(child, depth + 1, out, seen_ids)?,
        }
    }
    Ok(())
}

fn parse_gradient_element(node: roxmltree::Node, name: &str) -> Result<RawGradient, SvgError> {
    check_attributes(node, name)?;
    check_non_element_content(node)?;

    let num = |attr: &str, default: f32| -> Result<f32, SvgError> {
        match node.attribute(attr) {
            Some(v) => parse_number_or_percent(v).ok_or_else(|| SvgError::InvalidValue {
                element: name.to_string(),
                attr: attr.to_string(),
                reason: format!("not a number or percentage: `{v}`"),
            }),
            None => Ok(default),
        }
    };

    let kind = if name == "linearGradient" {
        RawKind::Linear {
            x1: num("x1", 0.0)?,
            y1: num("y1", 0.0)?,
            x2: num("x2", 1.0)?,
            y2: num("y2", 0.0)?,
        }
    } else {
        RawKind::Radial {
            cx: num("cx", 0.5)?,
            cy: num("cy", 0.5)?,
            r: num("r", 0.5)?,
        }
    };

    let mut stops = Vec::new();
    for child in node.children() {
        if child.is_element() {
            if local_name(child) != "stop"
                || !matches!(child.tag_name().namespace(), None | Some(SVG_NS))
            {
                return Err(SvgError::UnsupportedElement(local_name(child).to_string()));
            }
            stops.push(parse_stop(child)?);
            continue;
        }
        if child.is_pi() {
            return Err(SvgError::ProcessingInstructionRejected);
        }
        if child.is_text() && !child.text().unwrap_or("").trim().is_empty() {
            return Err(SvgError::TextContentRejected);
        }
    }

    Ok(RawGradient {
        kind,
        stops: normalize_stops(stops),
    })
}

fn parse_stop(node: roxmltree::Node) -> Result<Stop, SvgError> {
    check_attributes(node, "stop")?;
    check_non_element_content(node)?;
    for child in node.children() {
        if child.is_element() {
            return Err(SvgError::UnsupportedElement(local_name(child).to_string()));
        }
    }
    let offset_str = node
        .attribute("offset")
        .ok_or_else(|| SvgError::MissingAttribute {
            element: "stop".to_string(),
            attr: "offset".to_string(),
        })?;
    let offset = parse_number_or_percent(offset_str).ok_or_else(|| SvgError::InvalidValue {
        element: "stop".to_string(),
        attr: "offset".to_string(),
        reason: format!("not a number or percentage: `{offset_str}`"),
    })?;
    let color_str = node
        .attribute("stop-color")
        .ok_or_else(|| SvgError::MissingAttribute {
            element: "stop".to_string(),
            attr: "stop-color".to_string(),
        })?;
    let color = crate::color::parse_color(color_str).ok_or_else(|| SvgError::InvalidValue {
        element: "stop".to_string(),
        attr: "stop-color".to_string(),
        reason: format!("not a recognized color: `{color_str}`"),
    })?;
    Ok(Stop { offset, color })
}

fn resolve_gradient(raw: &RawGradient, bbox: (f32, f32, f32, f32)) -> Gradient {
    let (bx, by, bw, bh) = bbox;
    let kind = match raw.kind {
        RawKind::Linear { x1, y1, x2, y2 } => GradientKind::Linear {
            x1: bx + x1 * bw,
            y1: by + y1 * bh,
            x2: bx + x2 * bw,
            y2: by + y2 * bh,
        },
        RawKind::Radial { cx, cy, r } => GradientKind::Radial {
            cx: bx + cx * bw,
            cy: by + cy * bh,
            rx: r * bw,
            ry: r * bh,
        },
    };
    Gradient {
        kind,
        stops: raw.stops.clone(),
    }
}

// ---- pass 2: shapes --------------------------------------------------------

#[derive(Clone)]
struct Inherited {
    fill: PaintRef,
    stroke: PaintRef,
    ctm: Affine2,
    opacity: f32,
    in_defs: bool,
}

fn walk_shapes(
    node: roxmltree::Node,
    depth: u32,
    state: &Inherited,
    gradients: &HashMap<String, RawGradient>,
    seen_ids: &mut HashSet<String>,
    shapes: &mut Vec<Shape>,
) -> Result<(), SvgError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(SvgError::NestingTooDeep);
    }
    let name = local_name(node);
    // `check_element` already validated `node` itself when it is the root;
    // for recursive calls the parent loop below validates each child
    // before calling back in, so by the time we're here `name` is known
    // good — this function dispatches on it.
    check_attributes(node, name)?;
    check_non_element_content(node)?;
    register_id(node, seen_ids)?;

    let local_ctm = match node.attribute("transform") {
        Some(v) => parse_transform_list(v).ok_or_else(|| SvgError::InvalidValue {
            element: name.to_string(),
            attr: "transform".to_string(),
            reason: format!("not a valid transform list: `{v}`"),
        })?,
        None => Affine2::IDENTITY,
    };
    let ctm = compose(state.ctm, local_ctm);
    if !is_finite_affine(ctm) || ctm.determinant() == 0.0 {
        // SVG: a non-invertible (here: also non-finite, which the spec
        // grammar can't even express but overflow can produce) CTM means
        // "the given element and its children are not rendered" — valid
        // input, not an error, same as a zero-size shape. Skip this
        // element's own geometry and its entire subtree.
        return Ok(());
    }

    let fill = match node.attribute("fill") {
        Some(v) => parse_paint_ref(v).ok_or_else(|| invalid_paint(name, "fill", v))?,
        None => state.fill.clone(),
    };
    let stroke = match node.attribute("stroke") {
        Some(v) => parse_paint_ref(v).ok_or_else(|| invalid_paint(name, "stroke", v))?,
        None => state.stroke.clone(),
    };
    let own_opacity = match node.attribute("opacity") {
        Some(v) => crate::number::parse_single_number(v)
            .map(|o| o.clamp(0.0, 1.0))
            .ok_or_else(|| SvgError::InvalidValue {
                element: name.to_string(),
                attr: "opacity".to_string(),
                reason: format!("not a number: `{v}`"),
            })?,
        None => 1.0,
    };
    let opacity = state.opacity * own_opacity;
    let in_defs = state.in_defs || name == "defs";

    let next = Inherited {
        fill: fill.clone(),
        stroke: stroke.clone(),
        ctm,
        opacity,
        in_defs,
    };

    match name {
        "svg" | "g" | "defs" => {
            for child in node.children().filter(|c| c.is_element()) {
                let child_name = dispatch_element_name(child)?;
                if child_name == "linearGradient" || child_name == "radialGradient" {
                    continue; // fully owned + validated by pass 1
                }
                // `svg` is only valid as the document root (see the module
                // docs) — a nested `<svg>` would otherwise fall into this
                // same container arm and have its `viewBox`/`width`/`height`
                // silently ignored, exactly the best-effort guessing the
                // issue forbids.
                if child_name == "svg" {
                    return Err(SvgError::UnsupportedElement("svg".to_string()));
                }
                walk_shapes(child, depth + 1, &next, gradients, seen_ids, shapes)?;
            }
            Ok(())
        }
        "path" | "circle" | "ellipse" | "rect" | "line" | "polyline" | "polygon" => {
            // Shape elements have no element children at all in the subset.
            if let Some(child) = node.children().find(|c| c.is_element()) {
                return Err(SvgError::UnsupportedElement(local_name(child).to_string()));
            }
            if let Some(shape) = lower_shape(node, name, &next, gradients)? {
                if !in_defs {
                    shapes.push(shape);
                }
            }
            Ok(())
        }
        _ => Err(SvgError::UnsupportedElement(name.to_string())),
    }
}

/// Validate a child's name/namespace *before* recursing, so an unsupported
/// element is reported as itself rather than bubbling up from deeper
/// attribute-allowlist logic that never gets to run.
fn dispatch_element_name<'a>(node: roxmltree::Node<'a, 'a>) -> Result<&'a str, SvgError> {
    let name = local_name(node);
    if !matches!(node.tag_name().namespace(), None | Some(SVG_NS)) || allowed_attrs(name).is_none()
    {
        return Err(SvgError::UnsupportedElement(name.to_string()));
    }
    Ok(name)
}

fn register_id(node: roxmltree::Node, seen_ids: &mut HashSet<String>) -> Result<(), SvgError> {
    if let Some(id) = node.attribute("id") {
        if !seen_ids.insert(id.to_string()) {
            return Err(SvgError::DuplicateId(id.to_string()));
        }
    }
    Ok(())
}

fn invalid_paint(element: &str, attr: &str, value: &str) -> SvgError {
    SvgError::InvalidValue {
        element: element.to_string(),
        attr: attr.to_string(),
        reason: format!("not `none`, a color, or a gradient reference: `{value}`"),
    }
}

fn lower_shape(
    node: roxmltree::Node,
    name: &str,
    state: &Inherited,
    gradients: &HashMap<String, RawGradient>,
) -> Result<Option<Shape>, SvgError> {
    let Some(commands) = shape_commands(node, name)? else {
        return Ok(None); // zero-size shape: valid, just invisible.
    };

    // Scope decision: `objectBoundingBox` gradient resolution below uses the
    // control-point convex hull, not the tight geometric bbox of the
    // flattened curve. They agree exactly for every straight-edged shape
    // (rect/line/polyline/polygon and a `path` built only from `M`/`L`/`Z`)
    // and are a close-but-not-exact superset for a curved shape (`circle`/
    // `ellipse`/an arc or Bézier `path`) — a curve's extremum can sit
    // strictly inside its control hull, so a gradient on e.g. a circle is
    // very slightly wider than spec-exact. Computing the true extrema needs
    // each curve's derivative roots; deferred as a follow-up refinement
    // rather than blocking this issue on it.
    let Some(bbox) = control_hull_bounds(&commands) else {
        return Ok(None);
    };
    let bbox_tuple = (bbox.0, bbox.1, bbox.2 - bbox.0, bbox.3 - bbox.1);

    let resolve = |paint_ref: &PaintRef| -> Result<Option<Paint>, SvgError> {
        Ok(match paint_ref {
            PaintRef::None => None,
            PaintRef::Solid(c) => Some(Paint::Solid(c.with_opacity_multiplied(state.opacity))),
            PaintRef::GradientRef(id) => {
                let raw = gradients
                    .get(id)
                    .ok_or_else(|| SvgError::UnknownPaintReference(id.clone()))?;
                let mut g = resolve_gradient(raw, bbox_tuple);
                for stop in &mut g.stops {
                    stop.color = stop.color.with_opacity_multiplied(state.opacity);
                }
                Some(Paint::Gradient(flip_gradient_y(g)))
            }
        })
    };

    let fill = resolve(&state.fill)?;
    let stroke_paint = resolve(&state.stroke)?;
    let stroke = stroke_paint.map(|p| (Stroke::default(), p));

    let flipped = flip_commands_y(&commands);
    let flipped_bounds = control_hull_bounds(&flipped).expect("non-empty, checked above");
    let path = VectorPath {
        commands: flipped,
        fill: FillRule::NonZero,
        transform: flip_ctm(state.ctm),
        bounds: Bounds::new(
            Vec2::new(flipped_bounds.0, flipped_bounds.1),
            Vec2::new(flipped_bounds.2, flipped_bounds.3),
        ),
    };

    if fill.is_none() && stroke.is_none() {
        return Ok(None);
    }
    Ok(Some(Shape { path, fill, stroke }))
}

/// Build this shape's local (raw SVG-space, pre-flip, pre-CTM) geometry, or
/// `None` for a valid-but-zero-size shape (not an error — see the module
/// docs' scope decisions). A negative dimension is a hard
/// [`SvgError::NegativeDimension`].
fn shape_commands(node: roxmltree::Node, name: &str) -> Result<Option<Vec<PathCommand>>, SvgError> {
    let num = |attr: &str, default: f32| -> Result<f32, SvgError> {
        match node.attribute(attr) {
            Some(v) => {
                crate::number::parse_single_number(v).ok_or_else(|| SvgError::InvalidValue {
                    element: name.to_string(),
                    attr: attr.to_string(),
                    reason: format!("not a number: `{v}`"),
                })
            }
            None => Ok(default),
        }
    };
    let require_non_negative = |attr: &str, value: f32| -> Result<(), SvgError> {
        if value < 0.0 {
            Err(SvgError::NegativeDimension {
                element: name.to_string(),
                attr: attr.to_string(),
                value,
            })
        } else {
            Ok(())
        }
    };

    Ok(match name {
        "rect" => {
            let x = num("x", 0.0)?;
            let y = num("y", 0.0)?;
            let w = num("width", 0.0)?;
            let h = num("height", 0.0)?;
            require_non_negative("width", w)?;
            require_non_negative("height", h)?;
            if w <= 0.0 || h <= 0.0 {
                None
            } else {
                Some(vec![
                    PathCommand::MoveTo(Vec2::new(x, y)),
                    PathCommand::LineTo(Vec2::new(x + w, y)),
                    PathCommand::LineTo(Vec2::new(x + w, y + h)),
                    PathCommand::LineTo(Vec2::new(x, y + h)),
                    PathCommand::Close,
                ])
            }
        }
        "circle" => {
            let cx = num("cx", 0.0)?;
            let cy = num("cy", 0.0)?;
            let r = num("r", 0.0)?;
            require_non_negative("r", r)?;
            if r <= 0.0 {
                None
            } else {
                Some(ellipse_commands(cx, cy, r, r))
            }
        }
        "ellipse" => {
            let cx = num("cx", 0.0)?;
            let cy = num("cy", 0.0)?;
            let rx = num("rx", 0.0)?;
            let ry = num("ry", 0.0)?;
            require_non_negative("rx", rx)?;
            require_non_negative("ry", ry)?;
            if rx <= 0.0 || ry <= 0.0 {
                None
            } else {
                Some(ellipse_commands(cx, cy, rx, ry))
            }
        }
        "line" => {
            let x1 = num("x1", 0.0)?;
            let y1 = num("y1", 0.0)?;
            let x2 = num("x2", 0.0)?;
            let y2 = num("y2", 0.0)?;
            if x1 == x2 && y1 == y2 {
                None
            } else {
                Some(vec![
                    PathCommand::MoveTo(Vec2::new(x1, y1)),
                    PathCommand::LineTo(Vec2::new(x2, y2)),
                ])
            }
        }
        "polyline" | "polygon" => {
            let points_str = node.attribute("points").unwrap_or("");
            let nums = parse_number_list(points_str).ok_or_else(|| SvgError::InvalidValue {
                element: name.to_string(),
                attr: "points".to_string(),
                reason: format!("not a number list: `{points_str}`"),
            })?;
            if nums.len() < 4 || nums.len() % 2 != 0 {
                return Err(SvgError::InvalidValue {
                    element: name.to_string(),
                    attr: "points".to_string(),
                    reason: "expected an even list of at least 2 points".to_string(),
                });
            }
            let mut cmds = Vec::with_capacity(nums.len() / 2);
            cmds.push(PathCommand::MoveTo(Vec2::new(nums[0], nums[1])));
            let mut i = 2;
            while i + 1 < nums.len() {
                cmds.push(PathCommand::LineTo(Vec2::new(nums[i], nums[i + 1])));
                i += 2;
            }
            if name == "polygon" {
                cmds.push(PathCommand::Close);
            }
            Some(cmds)
        }
        "path" => {
            let d = node.attribute("d").unwrap_or("");
            let cmds = parse_path_data(d).ok_or_else(|| SvgError::InvalidValue {
                element: "path".to_string(),
                attr: "d".to_string(),
                reason: format!("does not match the path-data grammar: `{d}`"),
            })?;
            if cmds.is_empty() {
                None
            } else {
                Some(cmds)
            }
        }
        _ => unreachable!("walk_shapes only calls this for shape elements"),
    })
}

/// A 4-cubic-arc approximation of an axis-aligned ellipse, in raw SVG space.
fn ellipse_commands(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathCommand> {
    const K: f32 = 0.552_284_8;
    let (kx, ky) = (K * rx, K * ry);
    vec![
        PathCommand::MoveTo(Vec2::new(cx + rx, cy)),
        PathCommand::CubicTo {
            control1: Vec2::new(cx + rx, cy + ky),
            control2: Vec2::new(cx + kx, cy + ry),
            to: Vec2::new(cx, cy + ry),
        },
        PathCommand::CubicTo {
            control1: Vec2::new(cx - kx, cy + ry),
            control2: Vec2::new(cx - rx, cy + ky),
            to: Vec2::new(cx - rx, cy),
        },
        PathCommand::CubicTo {
            control1: Vec2::new(cx - rx, cy - ky),
            control2: Vec2::new(cx - kx, cy - ry),
            to: Vec2::new(cx, cy - ry),
        },
        PathCommand::CubicTo {
            control1: Vec2::new(cx + kx, cy - ry),
            control2: Vec2::new(cx + rx, cy - ky),
            to: Vec2::new(cx + rx, cy),
        },
        PathCommand::Close,
    ]
}

fn parse_length(v: &str, element: &str, attr: &str) -> Result<f32, SvgError> {
    let v = v.strip_suffix("px").unwrap_or(v);
    crate::number::parse_single_number(v).ok_or_else(|| SvgError::InvalidValue {
        element: element.to_string(),
        attr: attr.to_string(),
        reason: format!("not a plain number (with an optional `px` suffix): `{v}`"),
    })
}

fn parse_view_box(v: &str) -> Result<Option<ViewBox>, SvgError> {
    let nums = parse_number_list(v).ok_or_else(|| SvgError::InvalidValue {
        element: "svg".to_string(),
        attr: "viewBox".to_string(),
        reason: format!("not a 4-number list: `{v}`"),
    })?;
    match nums.as_slice() {
        [min_x, min_y, width, height] => Ok(Some(ViewBox {
            min_x: *min_x,
            min_y: *min_y,
            width: *width,
            height: *height,
        })),
        _ => Err(SvgError::InvalidValue {
            element: "svg".to_string(),
            attr: "viewBox".to_string(),
            reason: format!("expected exactly 4 numbers, got {}", nums.len()),
        }),
    }
}

/// Conservative bounding box (min_x, min_y, max_x, max_y) over every point
/// referenced by `commands` — control points included, same convention as
/// `mkui_vector2d::encode`'s `control_hull_bounds`. `None` for an empty list.
fn control_hull_bounds(commands: &[PathCommand]) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut any = false;
    let mut visit = |p: Vec2| {
        any = true;
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    };
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => visit(p),
            PathCommand::QuadTo { control, to } => {
                visit(control);
                visit(to);
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                visit(control1);
                visit(control2);
                visit(to);
            }
            PathCommand::Close => {}
            _ => {}
        }
    }
    any.then_some((min_x, min_y, max_x, max_y))
}

/// Negate the `y` of every point in `commands` — the one centralized y-flip
/// from SVG's y-down space into `mkui-vector2d`'s y-up convention (see
/// [`crate::transform`]'s module docs for why this happens here and not
/// per-value during parsing).
fn flip_commands_y(commands: &[PathCommand]) -> Vec<PathCommand> {
    let flip = |p: Vec2| Vec2::new(p.x, -p.y);
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo(p) => PathCommand::MoveTo(flip(p)),
            PathCommand::LineTo(p) => PathCommand::LineTo(flip(p)),
            PathCommand::QuadTo { control, to } => PathCommand::QuadTo {
                control: flip(control),
                to: flip(to),
            },
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => PathCommand::CubicTo {
                control1: flip(control1),
                control2: flip(control2),
                to: flip(to),
            },
            PathCommand::Close => PathCommand::Close,
            other => other,
        })
        .collect()
}

/// Whether every field of an [`Affine2`] is finite. A `transform` list built
/// only from finite, in-grammar numbers (guaranteed by [`crate::number`]'s
/// own finiteness check) can still *compose* to a non-finite CTM — e.g.
/// nested `scale(1e30)`s overflowing on multiplication — so this is checked
/// again after composition, not just at each individual number's parse.
fn is_finite_affine(m: Affine2) -> bool {
    [m.a, m.b, m.c, m.d, m.tx, m.ty]
        .into_iter()
        .all(f32::is_finite)
}

/// Conjugate an SVG-space CTM with the y-flip: `a`/`d`/`tx` are unchanged,
/// `b`/`c`/`ty` are negated (see `mkui_vector2d::path::Affine2`'s own docs
/// for the derivation this mirrors).
fn flip_ctm(m: Affine2) -> Affine2 {
    Affine2 {
        a: m.a,
        b: -m.b,
        c: -m.c,
        d: m.d,
        tx: m.tx,
        ty: -m.ty,
    }
}

/// Apply the same y-flip to a gradient's anchor points. Radii (`rx`/`ry`)
/// are magnitudes, not points, and are unaffected by a reflection.
fn flip_gradient_y(g: Gradient) -> Gradient {
    let kind = match g.kind {
        GradientKind::Linear { x1, y1, x2, y2 } => GradientKind::Linear {
            x1,
            y1: -y1,
            x2,
            y2: -y2,
        },
        GradientKind::Radial { cx, cy, rx, ry } => GradientKind::Radial {
            cx,
            cy: -cy,
            rx,
            ry,
        },
    };
    Gradient {
        kind,
        stops: g.stops,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_flip_round_trips_an_svg_space_rotation() {
        // SVG `rotate(90)` maps (10, 0) to (0, 10) in SVG's own y-down space
        // (standard rotation matrix, applied in a y-down frame). Flipping
        // that CTM into `mkui-vector2d`'s y-up convention and transforming
        // the identically-flipped local point must, when flipped back,
        // reproduce exactly that SVG-space answer — see `path.rs`'s own
        // "rotate(θ) is this struct's rotate(−θ)" contract.
        let svg_ctm = parse_transform_list("rotate(90)").unwrap();
        let local_point_svg = Vec2::new(10.0, 0.0);
        let local_point_yup = Vec2::new(local_point_svg.x, -local_point_svg.y);

        let yup_transform = flip_ctm(svg_ctm);
        let result_yup = yup_transform.transform_point(local_point_yup);
        let result_back_to_svg = Vec2::new(result_yup.x, -result_yup.y);

        let expected_svg = svg_ctm.transform_point(local_point_svg);
        assert!((result_back_to_svg.x - expected_svg.x).abs() < 1e-5);
        assert!((result_back_to_svg.y - expected_svg.y).abs() < 1e-5);
        assert!((expected_svg.x - 0.0).abs() < 1e-5);
        assert!((expected_svg.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn nested_group_and_local_transforms_compose_in_svg_order() {
        // `<g transform="translate(100,0)"><rect transform="scale(2)" .../></g>`
        // scales first, then translates — a point at local (5,0) lands at
        // SVG-space (110, 0), not (210, 0).
        let group = parse_transform_list("translate(100, 0)").unwrap();
        let local = parse_transform_list("scale(2)").unwrap();
        let ctm = compose(group, local);
        let p = ctm.transform_point(Vec2::new(5.0, 0.0));
        assert_eq!(p, Vec2::new(110.0, 0.0));
    }

    #[test]
    fn object_bounding_box_linear_gradient_maps_fractions_into_the_bbox() {
        let raw = RawGradient {
            kind: RawKind::Linear {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 0.0,
            },
            stops: vec![],
        };
        let g = resolve_gradient(&raw, (10.0, 20.0, 100.0, 50.0));
        assert_eq!(
            g.kind,
            GradientKind::Linear {
                x1: 10.0,
                y1: 20.0,
                x2: 110.0,
                y2: 20.0
            }
        );
    }

    #[test]
    fn object_bounding_box_radial_gradient_on_a_non_square_bbox_is_an_ellipse() {
        let raw = RawGradient {
            kind: RawKind::Radial {
                cx: 0.5,
                cy: 0.5,
                r: 0.5,
            },
            stops: vec![],
        };
        let g = resolve_gradient(&raw, (0.0, 0.0, 100.0, 50.0));
        assert_eq!(
            g.kind,
            GradientKind::Radial {
                cx: 50.0,
                cy: 25.0,
                rx: 50.0,
                ry: 25.0
            }
        );
    }

    #[test]
    fn doctype_is_rejected_before_roxmltree_ever_sees_it() {
        let xml =
            r##"<!DOCTYPE svg [<!ENTITY x "y">]><svg xmlns="http://www.w3.org/2000/svg"></svg>"##;
        assert_eq!(parse_svg(xml).unwrap_err(), SvgError::DoctypeRejected);
    }

    #[test]
    fn processing_instruction_is_rejected() {
        let xml =
            r##"<svg xmlns="http://www.w3.org/2000/svg"><?xml-stylesheet href="x.css"?></svg>"##;
        assert_eq!(
            parse_svg(xml).unwrap_err(),
            SvgError::ProcessingInstructionRejected
        );
    }

    #[test]
    fn script_element_is_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"##;
        assert_eq!(
            parse_svg(xml).unwrap_err(),
            SvgError::UnsupportedElement("script".to_string())
        );
    }

    #[test]
    fn xlink_href_is_rejected_as_a_foreign_namespace_attribute() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><rect xlink:href="#a" width="1" height="1"/></svg>"##;
        let err = parse_svg(xml).unwrap_err();
        // roxmltree resolves the `xlink:` prefix and reports the attribute's
        // *local* name (`href`), not the literal string `xlink:href` — pin
        // both fields so this test can't pass because the allowlist happened
        // to reject the unresolved literal string instead of the namespace.
        assert!(matches!(
            err,
            SvgError::UnsupportedAttribute { ref element, ref attr }
                if element.as_str() == "rect" && attr.as_str() == "href"
        ));
    }

    #[test]
    fn declaring_the_xlink_namespace_without_using_it_is_fine() {
        // Real Inkscape/Illustrator exports declare `xmlns:xlink` defensively
        // even on documents that never use an `xlink:` attribute at all.
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><rect width="1" height="1"/></svg>"##;
        assert!(parse_svg(xml).is_ok());
    }

    #[test]
    fn stray_text_content_is_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">hello</svg>"##;
        assert_eq!(parse_svg(xml).unwrap_err(), SvgError::TextContentRejected);
    }

    #[test]
    fn whitespace_only_text_is_accepted() {
        let xml = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n  <rect width=\"1\" height=\"1\"/>\n</svg>";
        assert!(parse_svg(xml).is_ok());
    }

    #[test]
    fn negative_dimension_is_rejected() {
        let xml =
            r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="-1" height="1"/></svg>"##;
        assert!(matches!(
            parse_svg(xml).unwrap_err(),
            SvgError::NegativeDimension { .. }
        ));
    }

    #[test]
    fn non_finite_geometry_and_opacity_values_are_rejected() {
        let cases = [
            r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="inf" height="1"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><circle r="NaN"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" opacity="nan"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><polyline points="0,0 1e39,0"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="inf"></svg>"##,
        ];
        for xml in cases {
            assert!(
                matches!(parse_svg(xml).unwrap_err(), SvgError::InvalidValue { .. }),
                "expected InvalidValue for: {xml}"
            );
        }
    }

    #[test]
    fn a_singular_transform_renders_nothing_rather_than_erroring() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <rect width="10" height="10" transform="matrix(0,0,0,0,0,0)"/>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert!(doc.shapes.is_empty());
    }

    #[test]
    fn a_singular_group_transform_suppresses_its_whole_subtree() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <g transform="scale(0)">
                <rect width="10" height="10"/>
                <circle r="5"/>
            </g>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert!(doc.shapes.is_empty());
    }

    #[test]
    fn processing_instruction_before_the_root_element_is_rejected() {
        // The common real-world position for `<?xml-stylesheet?>`, distinct
        // from the "inside <svg>" position the other PI test covers.
        let xml = r##"<?xml-stylesheet type="text/css" href="x.css"?><svg xmlns="http://www.w3.org/2000/svg"></svg>"##;
        assert_eq!(
            parse_svg(xml).unwrap_err(),
            SvgError::ProcessingInstructionRejected
        );
    }

    #[test]
    fn the_ordinary_xml_declaration_is_not_mistaken_for_a_rejected_pi() {
        // Every real-world SVG file starts with this; it must keep parsing.
        let xml = r##"<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/></svg>"##;
        assert!(parse_svg(xml).is_ok());
    }

    #[test]
    fn a_nested_svg_element_is_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <svg width="10" height="10"><rect width="1" height="1"/></svg>
        </svg>"##;
        assert_eq!(
            parse_svg(xml).unwrap_err(),
            SvgError::UnsupportedElement("svg".to_string())
        );
    }

    #[test]
    fn extreme_nesting_depth_is_rejected_instead_of_overflowing_the_stack() {
        let mut xml = String::from(r##"<svg xmlns="http://www.w3.org/2000/svg">"##);
        for _ in 0..10_000 {
            xml.push_str("<g>");
        }
        for _ in 0..10_000 {
            xml.push_str("</g>");
        }
        xml.push_str("</svg>");
        assert_eq!(parse_svg(&xml).unwrap_err(), SvgError::NestingTooDeep);
    }

    #[test]
    fn zero_size_rect_is_skipped_not_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="0" height="0"/></svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert!(doc.shapes.is_empty());
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <rect id="a" width="1" height="1"/>
            <rect id="a" width="1" height="1"/>
        </svg>"##;
        assert!(matches!(
            parse_svg(xml).unwrap_err(),
            SvgError::DuplicateId(id) if id == "a"
        ));
    }

    #[test]
    fn unknown_gradient_reference_is_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" fill="url(#missing)"/></svg>"##;
        assert!(matches!(
            parse_svg(xml).unwrap_err(),
            SvgError::UnknownPaintReference(id) if id == "missing"
        ));
    }

    #[test]
    fn forward_referenced_gradient_resolves() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <rect width="10" height="10" fill="url(#g)"/>
            <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0" stop-color="#000000"/>
                <stop offset="1" stop-color="#ffffff"/>
            </linearGradient>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert_eq!(doc.shapes.len(), 1);
        assert!(matches!(doc.shapes[0].fill, Some(Paint::Gradient(_))));
    }

    #[test]
    fn shapes_inside_defs_are_validated_but_not_rendered() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <defs><rect width="10" height="10"/></defs>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert!(doc.shapes.is_empty());

        let bad = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <defs><rect width="-10" height="10"/></defs>
        </svg>"##;
        assert!(matches!(
            parse_svg(bad).unwrap_err(),
            SvgError::NegativeDimension { .. }
        ));
    }

    #[test]
    fn stroke_width_attribute_is_outside_the_subset() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" stroke="#000" stroke-width="2"/></svg>"##;
        assert!(matches!(
            parse_svg(xml).unwrap_err(),
            SvgError::UnsupportedAttribute { attr, .. } if attr == "stroke-width"
        ));
    }

    #[test]
    fn style_and_class_attributes_are_rejected() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" style="fill:red"/></svg>"##;
        assert!(matches!(
            parse_svg(xml).unwrap_err(),
            SvgError::UnsupportedAttribute { attr, .. } if attr == "style"
        ));
    }

    #[test]
    fn fill_and_stroke_inherit_through_groups() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <g fill="#ff0000">
                <rect width="1" height="1"/>
            </g>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert_eq!(
            doc.shapes[0].fill,
            Some(Paint::Solid(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            }))
        );
    }

    #[test]
    fn group_opacity_multiplies_into_the_resolved_fill_alpha() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <g opacity="0.5">
                <rect width="1" height="1" fill="#000000"/>
            </g>
        </svg>"##;
        let doc = parse_svg(xml).unwrap();
        if let Some(Paint::Solid(c)) = doc.shapes[0].fill {
            assert!((c.a - 0.5).abs() < 1e-6);
        } else {
            panic!("expected a resolved solid fill");
        }
    }

    #[test]
    fn default_fill_is_black_and_default_stroke_is_none() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/></svg>"##;
        let doc = parse_svg(xml).unwrap();
        assert_eq!(doc.shapes[0].fill, Some(Paint::Solid(Color::BLACK)));
        assert!(doc.shapes[0].stroke.is_none());
    }
}
