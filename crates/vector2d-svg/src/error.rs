//! Typed rejections for the sanitized-subset SVG parser (#201).
//!
//! Every rejection is a distinct variant rather than a single `String`
//! message: the acceptance criterion is "an element or attribute outside the
//! subset causes a parse rejection ... not a best-effort guess", which means
//! callers (and tests) need to assert *which* rule was violated, not just
//! that parsing failed.

/// Why a document, element, attribute, or value was rejected.
///
/// `#[non_exhaustive]` — matching `mkui_vector2d::VectorPathEncodeError`'s
/// convention — so a new rejection kind is not a breaking change for callers
/// who only match on the few variants they care about. No `Eq` derive: the
/// `f32` payload in [`SvgError::NegativeDimension`] is only `PartialEq`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SvgError {
    /// `roxmltree` could not tokenize the input at all (not well-formed XML).
    #[error("malformed XML: {0}")]
    Xml(String),
    /// The document declared a `DOCTYPE`. Rejected outright rather than
    /// parsed with `allow_dtd: true` — entity expansion has no place in a
    /// sanitized renderer input (see the module docs on XXE).
    #[error("DOCTYPE declarations are not part of the sanitized subset")]
    DoctypeRejected,
    /// A processing instruction (e.g. `<?xml-stylesheet ...?>`) appeared in
    /// the document. PIs can reference external resources and have no
    /// representation in the subset.
    #[error("processing instructions are not part of the sanitized subset")]
    ProcessingInstructionRejected,
    /// Non-whitespace text content appeared where only elements are allowed.
    /// No element in the subset has text content (no `<text>`, no `<title>`/
    /// `<desc>`), so any non-whitespace text is outside it.
    #[error("non-whitespace text content is not part of the sanitized subset")]
    TextContentRejected,
    /// An element outside `{svg g defs path circle ellipse rect line
    /// polyline polygon linearGradient radialGradient stop}`, or any element
    /// in a non-empty/foreign XML namespace.
    #[error("element `{0}` is not part of the sanitized subset")]
    UnsupportedElement(String),
    /// An attribute outside the allowed set for the element it appeared on
    /// (see [`crate::document`] for the per-element allowlist), or any
    /// attribute in a foreign namespace (e.g. `xlink:href`).
    #[error("attribute `{attr}` on `<{element}>` is not part of the sanitized subset")]
    UnsupportedAttribute { element: String, attr: String },
    /// A required attribute was missing (e.g. `<stop>` with no `offset`).
    #[error("`<{element}>` is missing required attribute `{attr}`")]
    MissingAttribute { element: String, attr: String },
    /// An attribute value did not match the hand-written grammar for its
    /// kind (number, length, path data, transform list, or color).
    #[error("`<{element}>` attribute `{attr}` has an invalid value: {reason}")]
    InvalidValue {
        element: String,
        attr: String,
        reason: String,
    },
    /// A geometric attribute that the SVG grammar permits but the sanitized
    /// subset treats as malformed input rather than silently clamping (a
    /// negative `r`/`rx`/`ry`/`width`/`height`).
    #[error("`<{element}>` attribute `{attr}` must not be negative (got {value})")]
    NegativeDimension {
        element: String,
        attr: String,
        value: f32,
    },
    /// `fill`/`stroke` referenced `url(#id)` for a gradient `id` that was
    /// never defined.
    #[error("paint references unknown gradient `#{0}`")]
    UnknownPaintReference(String),
    /// Two elements in the same document declared the same `id`. Harmless
    /// in general SVG (first-wins is common practice) but ambiguous enough
    /// that a sanitized parser rejects it rather than guessing.
    #[error("duplicate id `{0}`")]
    DuplicateId(String),
    /// `<g>`/`<defs>`/gradient nesting exceeded the anti-stack-overflow depth
    /// cap. Not a grammar violation — a document this deeply nested is
    /// almost certainly adversarial, not authored, so it is rejected rather
    /// than risking a stack overflow walking it.
    #[error("element nesting is too deep")]
    NestingTooDeep,
    /// Every individual number in `<{0}>`'s geometry parsed as finite, but
    /// computing from them (e.g. `x + width`, or an elliptical arc's
    /// radius-correction math on a near-zero radius) overflowed `f32` to
    /// NaN/Inf. Rejected here rather than left to surface downstream as an
    /// unexplained `mkui_vector2d::VectorPathEncodeError::NonFinite`.
    #[error("`<{0}>`'s computed geometry is not finite (NaN or overflow)")]
    NonFiniteGeometry(String),
}
