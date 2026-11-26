#![forbid(unsafe_code)]
//! # mkui-vector2d-svg — sanitized-subset SVG parser + `mkui-vector2d` lowering
//!
//! Renders host-supplied vector art (icons, portraits, ...) through
//! [`mkui_vector2d`], limited to a deliberately small, explicit SVG subset
//! (#201): shapes (`path circle ellipse rect line polyline polygon g defs`),
//! linear/radial gradients with stops, and the presentation attributes
//! `fill stroke opacity transform`. Parsing is restricted to *exactly* that
//! subset — an element or attribute outside it is a typed [`SvgError`], never
//! a best-effort guess or a silent pass-through.
//!
//! This builds on the Slug/vector substrate (`mkui-vector2d`, #137-#143,
//! #157) already in flight for component rendering; this crate is
//! specifically the sanitized-subset *parser*, not the component-level
//! vector migration.
//!
//! ## Parsing libraries
//!
//! - **XML tokenization** goes entirely through [`roxmltree`] — no
//!   hand-rolled XML lexer anywhere in this crate.
//! - **Value parsing** (numbers, the `d` path-data grammar, `transform`
//!   functions, color) is hand-written against exactly the subset's
//!   grammar in [`number`], [`path_data`], [`arc`], [`transform`], and
//!   [`color`] — this crate depends on neither `svgtypes` nor `usvg` (see
//!   the workspace `deny.toml`'s `[bans]` list, which makes that a
//!   machine-enforced invariant, not just a convention).
//!
//! ## No code execution surface
//!
//! The subset has no `<script>`, no `<image>`/`<use>`/external `href`, no
//! `<style>`/CSS, and no DTD/entity expansion
//! (`roxmltree::ParsingOptions::allow_dtd` stays `false`, and [`parse_svg`]
//! additionally pre-screens for a literal `<!DOCTYPE` token). See
//! [`document`]'s module docs for the full list of what is rejected and why.
//!
//! ## What this crate does not do
//!
//! Gradient *data* is fully parsed and resolved ([`gradient::Gradient`]),
//! but no GPU adapter in this workspace can shade one yet —
//! `mkui_wgpu::PlacedVectorPath`/`PlacedStroke` only carry a solid
//! [`color::Color`]. Wiring gradient shading into the `slug` coverage
//! pipeline is out of scope for this issue and tracked as a follow-up (see
//! the PR description's "Scope decision" section).

pub mod arc;
pub mod color;
pub mod document;
pub mod error;
pub mod gradient;
pub mod number;
pub mod paint;
pub mod path_data;
pub mod transform;

pub use color::Color;
pub use document::{parse_svg, Document, Shape, ViewBox};
pub use error::SvgError;
pub use gradient::{Gradient, GradientKind, Stop};
pub use paint::Paint;
