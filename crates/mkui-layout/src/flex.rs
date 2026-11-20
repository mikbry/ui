//! The flex algorithm itself: direction (row/column), wrap, grow/shrink,
//! justify/align, gap, and min/max (today only `max-w-4xl`'s clamp — no T1
//! token sets an explicit min, but the clamp mechanism itself is generic).
//!
//! ## Model
//!
//! Every [`mkui_runtime::NodeKind::View`] (and the synthetic `Root`, and
//! [`mkui_runtime::NodeKind::Card`] — #270's card chrome is a container,
//! not a leaf) is a flex container. `resolved.flex_column`, or the absence
//! of `resolved.flex`
//! entirely (ordinary tailwind block flow — children stack one per line),
//! both mean **Column**; `resolved.flex` without `flex-col` means **Row**.
//! This mirrors real Tailwind: a plain block element's children already
//! stack vertically, which is exactly column-flex with `align-items:
//! stretch` — there is no separate "block" code path.
//!
//! `Text` / `Button` / `Custom` are always leaves; their size comes from
//! [`crate::LeafMeasure`], never computed here.
//!
//! ## Two measurement calls per subtree, not one
//!
//! A container's own box width is always exactly what its parent hands it
//! (ordinary block "fill the available width" behaviour, same as the
//! `mkui-wgpu` walker's current model) — **except** when sizing a Row
//! container's children, where each child's *main-axis* (width) flex basis
//! must default to its own max-content (shrink-to-fit) size, not the full
//! available width (otherwise `justify-between`'s two flex groups would
//! both stretch to the container width and the AC #1 header assertion in
//! `canonical_showcase_layout_lays_out_the_tree` would be meaningless). That
//! basis comes from `natural_width`, a width-only sibling of
//! `layout_container`/`layout_subtree` that answers "how wide would this
//! subtree be if nothing wrapped and nothing stretched" without building a
//! full `ComputedNode`. Column basis sizing has no equivalent shortcut —
//! block height is *always* content-derived (there's no "max-content
//! height" in CSS) — so a Column container's basis pass is a full
//! `layout_container` call with `avail_height: None`.
//!
//! ## Named tolerances and gaps (per issue #63's "own the stack" discipline)
//!
//! - **`flex-shrink` only ever shrinks `View` children.** A `Text` leaf
//!   shrinks by re-wrapping (handled naturally — it's re-measured at its
//!   final width); a `Button`/`Custom` leaf has no re-flow mechanism this
//!   engine can drive, so it never participates in the shrink pool. An
//!   overflowing line with no `View` children simply overflows (matches
//!   ordinary CSS `flex-shrink` behaviour when nothing *can* shrink enough).
//! - **`align-items: stretch` is a no-op on `Text`/`Button`/`Custom`
//!   leaves.** Only a `View`/`Root` child is re-measured at the line's/
//!   column's cross size; a leaf's content box can't stretch without
//!   clipping or distorting its own measured content.
//! - **`margin-top`/`margin-bottom` (`mt-2`, `mb-*`) only apply within a
//!   Column-direction container.** They're vertical margins; no T1 token
//!   sets a horizontal margin, and no Row-direction container in the
//!   showcase token set has a margin-bearing child, so the Row path does
//!   not thread them at all. Verify: `grep -n "mb-\|mt-2" examples/showcase-common/src/lib.rs`
//!   names only Column-direction ancestors.
//! - **`mt-auto` only absorbs free space within a Column-direction
//!   container** (the vertical main axis) — matching real flexbox, where an
//!   auto margin only absorbs space along the main axis. `flex-grow` is
//!   resolved first; `mt-auto` only sees whatever free space, if any,
//!   `flex-grow` left behind.
//! - **`flex-wrap` has no effect in a Column-direction container.**
//!   Wrapping requires a definite bound on the axis being wrapped; a
//!   column's main axis (height) is usually `auto`, and no showcase token
//!   combines `flex-wrap` with `flex-col`. Only Row direction buckets into
//!   lines.
//! - **`container` contributes no width constraint by itself** — it parses
//!   (Tier 1) but, absent a breakpoint system (explicitly out of scope,
//!   issue #63), has the same effect as ordinary block width: 100% of the
//!   available width. Only `max-w-4xl` actually clamps.
//! - **An explicit `h-*` on a Row container does not stretch cross-align
//!   children to fill it** — align-stretch only fills the content-derived
//!   line height. Not exercised by the showcase (the one `h-16` row uses
//!   `items-center`, not the stretch default).
//! - **A `View` with `flex-grow` inside a Row never inherits that row's
//!   `h-*`/avail-height as a vertical growth budget.** Growth only crosses
//!   from a container into its *same-axis* children; a Row's height is the
//!   cross axis for its direct children, never their main axis.
//! - **`Custom` nodes have no registry here.** A `Custom` leaf's size comes
//!   from [`crate::LeafMeasure::measure_custom`] (default: zero); one that
//!   [`crate::LeafMeasure::is_overlay`] reports `true` for is dropped from
//!   flow entirely, along with everything under it — this crate has no
//!   registry to recurse into an opaque extension node's children safely.

use std::collections::HashMap;

use mkui_runtime::{AppTree, Node, NodeId, NodeKind, ResolvedStyle};

use crate::geometry::{Point, Rect, Size};
use crate::measure::LeafMeasure;

/// Tailwind's spacing scale: 1 unit = 0.25rem = 4px at the standard 16px
/// root font size. Every numeric `ResolvedStyle` field (padding, margin,
/// gap, height) is a raw Tailwind tick and multiplies by this to land in
/// logical pixels — same scale the `mkui-wgpu` walker uses today
/// (`TAILWIND_UNIT_PX` in `walker.rs`), kept here as its own constant so
/// this crate has zero dependency on that one.
const TAILWIND_UNIT_PX: f32 = 4.0;

/// `max-w-4xl` = `56rem` at the standard 16px root = 896px. The only T1
/// token with a numeric max-width today; `min`/`max` clamping in this
/// engine is otherwise generic (see the module docs).
const MAX_W_4XL_PX: f32 = 896.0;

/// The output of [`crate::compute_layout`]: absolute border-box rects keyed
/// by [`NodeId`]. A hidden subtree (`resolved.hidden`), and any subtree
/// rooted at a [`crate::LeafMeasure::is_overlay`] node, has no entry at all
/// — neither the node itself nor anything under it.
///
/// `NodeId` has no `Ord`, so this does not expose a sorted view. [`Self::iter`]
/// walks a pre-order [`Vec`] captured during the same walk that built the
/// map, giving deterministic (parent-before-children, source-sibling-order)
/// iteration without requiring one.
#[derive(Debug, Default)]
pub struct LayoutResult {
    order: Vec<NodeId>,
    rects: HashMap<NodeId, Rect>,
}

impl LayoutResult {
    /// The computed border-box rect for `id`, or `None` if `id` was hidden,
    /// inside a dropped overlay subtree, or never visited.
    pub fn get(&self, id: NodeId) -> Option<Rect> {
        self.rects.get(&id).copied()
    }

    /// Every laid-out node, pre-order (a node always precedes its
    /// descendants; siblings keep their source order).
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, Rect)> + '_ {
        self.order.iter().map(move |id| (*id, self.rects[id]))
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Entry point. `viewport` is the definite size the root box is laid out
/// against — the root's height is treated as definite (not content-driven)
/// so a top-level `flex-1` chain (page shell → main content) actually grows
/// to fill it, matching the canonical showcase tree's intent.
pub fn compute_layout(tree: &AppTree, viewport: Size, measure: &dyn LeafMeasure) -> LayoutResult {
    let root_id = tree.root();
    let computed = layout_subtree(
        tree,
        measure,
        root_id,
        viewport.width.max(0.0),
        Some(viewport.height.max(0.0)),
    );

    let mut result = LayoutResult::default();
    // The synthetic Root never gets a rect of its own — same precedent as
    // the `mkui-wgpu` walker, which starts painting from `root.children`.
    for (child_id, origin, child) in &computed.children {
        flatten(*child_id, child, *origin, &mut result);
    }
    result
}

fn flatten(id: NodeId, computed: &ComputedNode, origin: Point, out: &mut LayoutResult) {
    out.order.push(id);
    out.rects.insert(id, Rect::new(origin, computed.size));
    for (child_id, rel_origin, child) in &computed.children {
        flatten(*child_id, child, origin + *rel_origin, out);
    }
}

/// A fully laid-out subtree: its own border-box size, plus each visible
/// child's position *relative to this node's own origin* and that child's
/// own [`ComputedNode`]. Keeping positions relative (rather than writing
/// absolute rects immediately) means the row/column arrangement math — the
/// expensive part — runs exactly once per node; [`flatten`] only ever adds
/// origins on its way down.
struct ComputedNode {
    size: Size,
    children: Vec<(NodeId, Point, ComputedNode)>,
}

impl ComputedNode {
    fn leaf(size: Size) -> Self {
        Self {
            size,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Justify {
    Start,
    Center,
    Between,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Stretch,
    Center,
}

fn direction(style: &ResolvedStyle) -> Direction {
    if style.flex_column {
        Direction::Column
    } else if style.flex {
        Direction::Row
    } else {
        // Ordinary block flow: children stack one per line, same as a
        // column flex container with the default stretch cross-alignment.
        Direction::Column
    }
}

fn justify(style: &ResolvedStyle) -> Justify {
    if style.justify_center {
        Justify::Center
    } else if style.justify_between {
        Justify::Between
    } else {
        Justify::Start
    }
}

fn align(style: &ResolvedStyle) -> Align {
    if style.items_center {
        Align::Center
    } else {
        Align::Stretch
    }
}

fn gap_px(style: &ResolvedStyle, dir: Direction) -> f32 {
    let units = style
        .gap
        .or(match dir {
            Direction::Row => style.space_x,
            Direction::Column => style.space_y,
        })
        .unwrap_or(0);
    units as f32 * TAILWIND_UNIT_PX
}

fn padding_x_px(style: &ResolvedStyle) -> f32 {
    style
        .padding_x
        .or(style.padding)
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

fn padding_y_px(style: &ResolvedStyle) -> f32 {
    style
        .padding_y
        .or(style.padding)
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

fn margin_top_px(style: &ResolvedStyle) -> f32 {
    style
        .margin_top
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

fn margin_bottom_px(style: &ResolvedStyle) -> f32 {
    style
        .margin_bottom
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

/// `h-*` as an explicit pixel height, or `None` for `h-auto` / unset (both
/// mean "content-driven" — [`ResolvedStyle::height_auto`] only matters in
/// that it additionally opts out of an *inherited* definite height from an
/// ancestor's `flex-grow`, handled in [`layout_container`]).
fn explicit_height_px(style: &ResolvedStyle) -> Option<f32> {
    style.height.map(|n| n as f32 * TAILWIND_UNIT_PX)
}

fn is_container(tree: &AppTree, id: NodeId) -> bool {
    matches!(
        tree.get(id).map(|n| &n.kind),
        Some(NodeKind::View(_)) | Some(NodeKind::Card(_)) | Some(NodeKind::Root)
    )
}

/// Whether `id` participates in layout at all. `false` for a hidden node or
/// an overlay `Custom` node — neither gets a [`ComputedNode`], a rect, gap,
/// or flex-basis contribution; it is as if it were not in the tree.
fn is_visible(tree: &AppTree, measure: &dyn LeafMeasure, id: NodeId) -> bool {
    let Some(node) = tree.get(id) else {
        return false;
    };
    if node.resolved.hidden {
        return false;
    }
    if let NodeKind::Custom { type_name, props } = &node.kind {
        if measure.is_overlay(type_name, props) {
            return false;
        }
    }
    true
}

fn visible_children(tree: &AppTree, measure: &dyn LeafMeasure, node: &Node) -> Vec<NodeId> {
    node.children
        .iter()
        .copied()
        .filter(|id| is_visible(tree, measure, *id))
        .collect()
}

/// Max-content (shrink-to-fit, never-wrapped) width of `id`'s subtree. Used
/// only to compute a Row container's children's flex basis — see the
/// module docs' "Two measurement calls per subtree" section for why this
/// has no Column-direction analogue.
fn natural_width(tree: &AppTree, measure: &dyn LeafMeasure, id: NodeId) -> f32 {
    let Some(node) = tree.get(id) else {
        return 0.0;
    };
    match &node.kind {
        NodeKind::Root | NodeKind::View(_) | NodeKind::Card(_) => {
            let style = &node.resolved;
            let pad_x = padding_x_px(style);
            let kids = visible_children(tree, measure, node);
            let inner = match direction(style) {
                Direction::Row => {
                    let gap = gap_px(style, Direction::Row);
                    let sum: f32 = kids
                        .iter()
                        .map(|id| natural_width(tree, measure, *id))
                        .sum();
                    sum + gap * kids.len().saturating_sub(1) as f32
                }
                Direction::Column => kids
                    .iter()
                    .map(|id| natural_width(tree, measure, *id))
                    .fold(0.0_f32, f32::max),
            };
            let mut width = inner + pad_x * 2.0;
            if style.max_w_4xl {
                width = width.min(MAX_W_4XL_PX);
            }
            width
        }
        NodeKind::Text(t) => {
            measure
                .measure_text(&t.content, t.variant, &node.resolved, None)
                .size
                .width
        }
        NodeKind::Button(b) => {
            measure
                .measure_button(&b.label, b.variant, &node.resolved, None)
                .size
                .width
        }
        NodeKind::Custom { type_name, props } => {
            measure
                .measure_custom(type_name, props, &node.resolved, None)
                .size
                .width
        }
    }
}

/// Lay out `id` as a box given `avail_width` (its definite slot — it always
/// fills this, modulo `max-w-4xl`) and `avail_height` (`Some` only when an
/// ancestor has a definite main-size budget to hand down for `flex-grow`/
/// `mt-auto` resolution; `None` means ordinary content-driven auto height).
fn layout_subtree(
    tree: &AppTree,
    measure: &dyn LeafMeasure,
    id: NodeId,
    avail_width: f32,
    avail_height: Option<f32>,
) -> ComputedNode {
    let Some(node) = tree.get(id) else {
        return ComputedNode::leaf(Size::ZERO);
    };
    match &node.kind {
        NodeKind::Root | NodeKind::View(_) | NodeKind::Card(_) => {
            layout_container(tree, measure, node, avail_width, avail_height)
        }
        NodeKind::Text(t) => {
            let m = measure.measure_text(
                &t.content,
                t.variant,
                &node.resolved,
                Some(avail_width.max(0.0)),
            );
            ComputedNode::leaf(m.size)
        }
        NodeKind::Button(b) => {
            let m = measure.measure_button(
                &b.label,
                b.variant,
                &node.resolved,
                Some(avail_width.max(0.0)),
            );
            ComputedNode::leaf(m.size)
        }
        NodeKind::Custom { type_name, props } => {
            let m = measure.measure_custom(
                type_name,
                props,
                &node.resolved,
                Some(avail_width.max(0.0)),
            );
            ComputedNode::leaf(m.size)
        }
    }
}

fn layout_container(
    tree: &AppTree,
    measure: &dyn LeafMeasure,
    node: &Node,
    avail_width: f32,
    avail_height: Option<f32>,
) -> ComputedNode {
    let style = &node.resolved;
    let pad_x = padding_x_px(style);
    let pad_y = padding_y_px(style);

    let mut box_width = avail_width.max(0.0);
    if style.max_w_4xl {
        box_width = box_width.min(MAX_W_4XL_PX);
    }
    let inner_width = (box_width - pad_x * 2.0).max(0.0);

    // The definite main-size budget THIS box resolves to, if any: an
    // explicit `h-*` wins outright; `h-auto` opts out of whatever the
    // parent offered; otherwise an inherited `avail_height` (this node is
    // itself a `flex-grow` target, or the root call) passes through.
    let explicit = explicit_height_px(style);
    let definite_target = explicit.or(if style.height_auto {
        None
    } else {
        avail_height
    });

    let dir = direction(style);
    let (mut children, content_height) = match dir {
        Direction::Row => {
            // An explicit `h-*` is a definite cross size for a *single*
            // wrapped line — align-items (stretch or center) resolves
            // against it instead of the content-derived line height, which
            // is what makes `items-center` on an `h-16` row actually center
            // within the full 64px band. Distributing an explicit height
            // across several wrapped lines (`align-content`) is not
            // implemented (untested by the showcase token set, which never
            // combines `h-*` with `flex-wrap`); with more than one line
            // this falls back to content-derived per-line heights.
            let cross_override = explicit.map(|h| (h - pad_y * 2.0).max(0.0));
            let (children, size) =
                layout_row_children(tree, measure, node, inner_width, cross_override);
            (children, size.height)
        }
        Direction::Column => {
            let budget = definite_target.map(|h| (h - pad_y * 2.0).max(0.0));
            let (children, size) = layout_column_children(tree, measure, node, inner_width, budget);
            (children, size.height)
        }
    };

    for (_, origin, _) in children.iter_mut() {
        origin.x += pad_x;
        origin.y += pad_y;
    }

    let own_height = definite_target.unwrap_or(content_height + pad_y * 2.0);

    ComputedNode {
        size: Size::new(box_width, own_height),
        children,
    }
}

/// One visible child's precomputed box-model inputs, shared by the row and
/// column arrangement passes.
#[derive(Clone, Copy)]
struct ChildPlan {
    id: NodeId,
    is_view: bool,
    flex_grow: bool,
    mt_auto: bool,
    mx_auto: bool,
}

fn plan_children(tree: &AppTree, measure: &dyn LeafMeasure, node: &Node) -> Vec<ChildPlan> {
    visible_children(tree, measure, node)
        .into_iter()
        .map(|id| {
            let style = &tree.get(id).expect("visible child must exist").resolved;
            ChildPlan {
                id,
                is_view: is_container(tree, id),
                flex_grow: style.flex_grow,
                mt_auto: style.mt_auto,
                mx_auto: style.mx_auto,
            }
        })
        .collect()
}

/// Distribute `free` pixels of main-axis space across `basis`, growing
/// (positive `free`) or shrinking (negative `free`) in place. Growth splits
/// evenly across every `flex-grow` child; shrink splits proportionally to
/// basis size across `View` children only (see the module docs' shrink
/// tolerance) and never below zero. Returns how much of `free` was actually
/// consumed by grow/shrink (always `free` itself unless shrink bottoms out
/// at zero before using all of it).
fn distribute_main_axis(children: &[ChildPlan], basis: &mut [f32], free: f32) -> f32 {
    if free > 0.0 {
        let growers: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.flex_grow)
            .map(|(i, _)| i)
            .collect();
        if growers.is_empty() {
            return 0.0;
        }
        let share = free / growers.len() as f32;
        for i in growers {
            basis[i] += share;
        }
        free
    } else if free < 0.0 {
        let shrinkable: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_view)
            .map(|(i, _)| i)
            .collect();
        if shrinkable.is_empty() {
            return 0.0;
        }
        let shrinkable_total: f32 = shrinkable.iter().map(|&i| basis[i]).sum();
        if shrinkable_total <= 0.0 {
            return 0.0;
        }
        let overflow = -free;
        for i in shrinkable {
            let ratio = basis[i] / shrinkable_total;
            let reduction = (overflow * ratio).min(basis[i]);
            basis[i] -= reduction;
        }
        free
    } else {
        0.0
    }
}

fn layout_column_children(
    tree: &AppTree,
    measure: &dyn LeafMeasure,
    node: &Node,
    inner_width: f32,
    budget: Option<f32>,
) -> (Vec<(NodeId, Point, ComputedNode)>, Size) {
    let style = &node.resolved;
    let gap = gap_px(style, Direction::Column);
    let align_mode = align(style);
    let plans = plan_children(tree, measure, node);

    if plans.is_empty() {
        return (Vec::new(), Size::new(inner_width, 0.0));
    }

    // Basis pass: each child's cross-axis (width) slot per `align`, then its
    // natural (auto) height at that width.
    let child_widths: Vec<f32> = plans
        .iter()
        .map(|p| match align_mode {
            Align::Stretch => inner_width,
            Align::Center => natural_width(tree, measure, p.id).min(inner_width),
        })
        .collect();
    let mut computed: Vec<ComputedNode> = plans
        .iter()
        .zip(&child_widths)
        .map(|(p, &w)| layout_subtree(tree, measure, p.id, w, None))
        .collect();

    let margins: Vec<(f32, f32)> = plans
        .iter()
        .map(|p| {
            let s = &tree.get(p.id).expect("visible child must exist").resolved;
            (margin_top_px(s), margin_bottom_px(s))
        })
        .collect();

    let mut heights: Vec<f32> = computed.iter().map(|c| c.size.height).collect();
    let margin_sum: f32 = margins.iter().map(|(t, b)| t + b).sum();
    let gaps_total = gap * (plans.len().saturating_sub(1)) as f32;
    let basis_total = heights.iter().sum::<f32>() + margin_sum + gaps_total;

    let mut leftover = 0.0_f32;
    if let Some(budget) = budget {
        let free = budget - basis_total;
        let consumed = distribute_main_axis(&plans, &mut heights, free);
        leftover = free - consumed;

        // Re-resolve any child whose height actually changed, so nested
        // content (e.g. a `flex-1` descendant) sees its real final height.
        for (i, (plan, &w)) in plans.iter().zip(&child_widths).enumerate() {
            if (heights[i] - computed[i].size.height).abs() > f32::EPSILON {
                computed[i] = layout_subtree(tree, measure, plan.id, w, Some(heights[i]));
            }
        }
    }

    // `mt-auto`: flex-grow (above) is resolved first; `mt-auto` only ever
    // sees whatever positive free space growth left behind.
    let mut leading_offset = 0.0_f32;
    let mut extra_gap = 0.0_f32;
    let mut auto_margin_before: Vec<f32> = vec![0.0; plans.len()];
    if leftover > 0.0 {
        if let Some(i) = plans.iter().position(|p| p.mt_auto) {
            auto_margin_before[i] = leftover;
        } else {
            match justify(style) {
                Justify::Start => {}
                Justify::Center => leading_offset = leftover / 2.0,
                Justify::Between if plans.len() > 1 => {
                    extra_gap = leftover / (plans.len() - 1) as f32;
                }
                Justify::Between => {}
            }
        }
    }

    let mut out = Vec::with_capacity(plans.len());
    let mut cursor_y = leading_offset;
    for (i, plan) in plans.iter().enumerate() {
        cursor_y += auto_margin_before[i] + margins[i].0;
        let x = match align_mode {
            Align::Stretch if plan.mx_auto && computed[i].size.width < inner_width => {
                (inner_width - computed[i].size.width) / 2.0
            }
            Align::Stretch => 0.0,
            Align::Center => (inner_width - computed[i].size.width) / 2.0,
        };
        let child = std::mem::replace(&mut computed[i], ComputedNode::leaf(Size::ZERO));
        let h = child.size.height;
        out.push((plan.id, Point::new(x, cursor_y), child));
        cursor_y += h + margins[i].1;
        if i + 1 < plans.len() {
            cursor_y += gap + extra_gap;
        }
    }

    (out, Size::new(inner_width, cursor_y))
}

fn layout_row_children(
    tree: &AppTree,
    measure: &dyn LeafMeasure,
    node: &Node,
    inner_width: f32,
    cross_override: Option<f32>,
) -> (Vec<(NodeId, Point, ComputedNode)>, Size) {
    let style = &node.resolved;
    let gap = gap_px(style, Direction::Row);
    let align_mode = align(style);
    let plans = plan_children(tree, measure, node);

    if plans.is_empty() {
        return (Vec::new(), Size::new(inner_width, 0.0));
    }

    let basis: Vec<f32> = plans
        .iter()
        .map(|p| natural_width(tree, measure, p.id))
        .collect();

    // Bucket into lines. `flex-wrap` only applies in Row direction (see
    // module docs); without it every child is one line regardless of
    // overflow, matching ordinary `flex-nowrap` overflow behaviour.
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if style.flex_wrap {
        let mut current: Vec<usize> = Vec::new();
        let mut current_width = 0.0_f32;
        for (i, &w) in basis.iter().enumerate() {
            let next_width = if current.is_empty() {
                w
            } else {
                current_width + gap + w
            };
            if !current.is_empty() && next_width > inner_width {
                lines.push(std::mem::take(&mut current));
                current_width = w;
            } else {
                current_width = next_width;
            }
            current.push(i);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    } else {
        lines.push((0..plans.len()).collect());
    }

    let mut out = Vec::with_capacity(plans.len());
    let mut line_top = 0.0_f32;
    for (line_idx, line) in lines.iter().enumerate() {
        let mut line_basis: Vec<f32> = line.iter().map(|&i| basis[i]).collect();
        let line_plans: Vec<ChildPlan> = line.iter().map(|&i| plans[i]).collect();
        let gaps_total = gap * (line.len().saturating_sub(1)) as f32;
        let total: f32 = line_basis.iter().sum::<f32>() + gaps_total;
        let free = inner_width - total;
        distribute_main_axis(&line_plans, &mut line_basis, free);

        let mut line_computed: Vec<ComputedNode> = line_plans
            .iter()
            .zip(&line_basis)
            .map(|(p, &w)| layout_subtree(tree, measure, p.id, w, None))
            .collect();

        let natural_line_height = line_computed
            .iter()
            .map(|c| c.size.height)
            .fold(0.0_f32, f32::max);
        let line_height = if lines.len() == 1 {
            cross_override.unwrap_or(natural_line_height)
        } else {
            natural_line_height
        };

        // Cross-axis stretch: only a `View`/`Root` child is re-measured at
        // the line's cross size (see the module docs' stretch tolerance).
        if align_mode == Align::Stretch {
            for (idx, p) in line_plans.iter().enumerate() {
                if p.is_view && (line_height - line_computed[idx].size.height).abs() > f32::EPSILON
                {
                    line_computed[idx] =
                        layout_subtree(tree, measure, p.id, line_basis[idx], Some(line_height));
                }
            }
        }

        let packed_width: f32 = line_basis.iter().sum::<f32>() + gaps_total;
        let remaining = inner_width - packed_width;
        let (leading_x, extra_gap) = match justify(style) {
            Justify::Start => (0.0, 0.0),
            Justify::Center => (remaining.max(0.0) / 2.0, 0.0),
            Justify::Between if line.len() > 1 => {
                (0.0, remaining.max(0.0) / (line.len() - 1) as f32)
            }
            Justify::Between => (0.0, 0.0),
        };

        let mut cursor_x = leading_x;
        for (idx, p) in line_plans.iter().enumerate() {
            let w = line_basis[idx];
            let child = std::mem::replace(&mut line_computed[idx], ComputedNode::leaf(Size::ZERO));
            let h = child.size.height;
            let y = match align_mode {
                Align::Stretch => 0.0,
                Align::Center => (line_height - h) / 2.0,
            };
            out.push((p.id, Point::new(cursor_x, line_top + y), child));
            cursor_x += w + gap + extra_gap;
        }

        line_top += line_height;
        if line_idx + 1 < lines.len() {
            line_top += gap;
        }
    }

    (out, Size::new(inner_width, line_top))
}
