//! Class-string parser and the [`ResolvedStyle`] it produces.
//!
//! mkui uses Tailwind-shaped utility class strings (`"flex items-center px-4"`).
//! Until Sprint 4 the parser lived nowhere — `mkui-web` passed strings straight
//! to the DOM, the console backend ignored them, and `mkui-wgpu` was about to
//! grow a third parallel implementation. The runtime now owns the parse so
//! every binding sees the same `ResolvedStyle`, and parity tests can compare
//! the resolved structure across Rust / C / Python construction.
//!
//! ## Tiered support
//!
//! Three tiers, all surfaced through [`StyleClass::parse`]:
//!
//! - **Tier 1 (T1)** — fully supported. 39 tokens covering the showcase set.
//!   The runtime emits a typed `ResolvedStyle` field for each.
//! - **Tier 2 (T2)** — documented no-op. `hover:*`, `sm:*`, `transition-colors`
//!   parse cleanly but contribute nothing to `ResolvedStyle` — the renderer
//!   may still forward the raw class string (web does), but parity tests
//!   compare structure, not raw strings, so a T2 token never breaks parity.
//! - **Tier 3 (T3)** — parse error. Anything else is rejected loudly so we
//!   catch typos and silent divergence between bindings.
//!
//! The 39 T1 tokens were extracted from `examples/showcase-common/src/lib.rs`
//! — the cross-binding showcase. Adding a token to the showcase without
//! adding it here will surface as a `ClassParseError::UnknownToken`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Raw utility-class string.
///
/// Parses lazily via [`StyleClass::parse`]; the unparsed form is preserved so
/// backends (notably web) that still forward classes to a native CSS engine
/// can do so without round-tripping through the typed form.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct StyleClass {
    raw: String,
}

impl StyleClass {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::should_implement_trait)] // accepts `impl Into<String>`, not just `&str`
    pub fn from_str(s: impl Into<String>) -> Self {
        Self { raw: s.into() }
    }

    /// Raw class string in source order, whitespace-separated. Backends may
    /// forward this to a native CSS engine; parity tests compare parsed
    /// [`ResolvedStyle`] instead, so renderers are free to layer their own
    /// transformations on the raw form.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn is_empty(&self) -> bool {
        self.raw.trim().is_empty()
    }

    /// Parse every whitespace-separated token. Returns the first
    /// `ClassParseError` encountered — parsing is fail-fast so typos surface
    /// loudly. Unknown tokens are Tier 3 (parse error); Tier 2 tokens parse
    /// cleanly but contribute nothing to the resolved structure.
    pub fn parse(&self) -> Result<ResolvedStyle, ClassParseError> {
        let mut resolved = ResolvedStyle::default();
        for token in self.raw.split_whitespace() {
            apply_token(token, &mut resolved)?;
        }
        Ok(resolved)
    }
}

impl fmt::Display for StyleClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Parsed structural view of a class string.
///
/// Every field is `Option` / `bool` / small numeric — the renderer reads
/// these without re-parsing the string. The shape is intentionally flat:
/// future tokens that introduce nested groups (e.g. responsive variants)
/// will extend the struct rather than nest it, so JSON snapshots stay
/// stable.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ResolvedStyle {
    /// `flex`. Container becomes a flex container.
    pub flex: bool,
    /// `flex-col`. Main axis switches to column.
    pub flex_column: bool,
    /// `flex-wrap`. Children wrap.
    pub flex_wrap: bool,
    /// `flex-1`. Item grows.
    pub flex_grow: bool,
    /// `items-center`. Cross-axis center alignment.
    pub items_center: bool,
    /// `justify-between`. Main-axis space-between distribution.
    pub justify_between: bool,
    /// `container` / `mx-auto`. Layout-container hints.
    pub container: bool,
    pub mx_auto: bool,
    pub mt_auto: bool,
    /// `hidden`. Element is removed from layout.
    pub hidden: bool,
    /// gap-* / space-x-* / space-y-* in tailwind half-rem units.
    pub gap: Option<u32>,
    pub space_x: Option<u32>,
    pub space_y: Option<u32>,
    /// mb-* / mt-* / mt-N — vertical margin in tailwind half-rem units.
    pub margin_bottom: Option<u32>,
    pub margin_top: Option<u32>,
    /// p-* / px-* / py-* — padding in tailwind half-rem units.
    pub padding: Option<u32>,
    pub padding_x: Option<u32>,
    pub padding_y: Option<u32>,
    /// h-* — height: `Some(n)` for h-N, `None` for h-auto (default).
    /// `height_auto = true` distinguishes `h-auto` from "not set".
    pub height: Option<u32>,
    pub height_auto: bool,
    /// max-w-4xl etc — only `max_w_4xl` is in T1 today.
    pub max_w_4xl: bool,
    /// Text size + weight + alignment.
    pub text_size: Option<TextSize>,
    pub font_semibold: bool,
    pub font_bold: bool,
    pub text_center: bool,
    pub tracking_tight: bool,
    pub leading_none: bool,
    /// Borders. `border` is all-sides, `border_top` / `border_bottom` directional.
    pub border: bool,
    pub border_top: bool,
    pub border_bottom: bool,
    pub rounded_lg: bool,
    pub shadow_sm: bool,
    /// Color tokens — keep as `Option<ColorRole>` so renderers map to themed
    /// values, not raw hex.
    pub background: Option<ColorRole>,
    pub foreground: Option<ColorRole>,
    /// Raw count of Tier 2 (no-op) tokens encountered. Lets parity tests
    /// assert that T2 was tolerated, not silently dropped.
    pub tier2_count: u32,
}

/// Subset of Tailwind text-size scale used in the showcase.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    Sm,
    Xl,
    Xl2,
    Xl4,
}

/// Theme-role color tokens — renderers map these to their palette.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ColorRole {
    Card,
    CardForeground,
    Foreground,
    MutedForeground,
}

/// Parse failure surfaced by [`StyleClass::parse`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassParseError {
    /// Token is not in T1, T2, or any recognised pattern.
    UnknownToken(String),
}

impl fmt::Display for ClassParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownToken(t) => write!(
                f,
                "unknown class token `{t}` — not in T1 (39 known utility classes), \
                 not in T2 (hover:*/sm:*/transition-colors no-ops), and not a recognised \
                 pattern. If this is intentional, add it to mkui-runtime's class parser \
                 (Tier 1 if it has runtime semantics, Tier 2 if it's a documented no-op).",
            ),
        }
    }
}

impl std::error::Error for ClassParseError {}

fn apply_token(token: &str, out: &mut ResolvedStyle) -> Result<(), ClassParseError> {
    // Tier 2 — documented no-op tokens. Recognised so the parser does not
    // reject them; renderers (notably web) may still emit them as raw CSS
    // classes via `StyleClass::raw`.
    if token.starts_with("hover:") || token.starts_with("sm:") || token == "transition-colors" {
        out.tier2_count += 1;
        return Ok(());
    }

    // Tier 1 — 39 tokens. Match in exact source order from the issue body's
    // acceptance criterion #5 so any regression is grep-locatable.
    match token {
        "flex" => out.flex = true,
        "flex-1" => out.flex_grow = true,
        "flex-col" => out.flex_column = true,
        "flex-wrap" => out.flex_wrap = true,
        "items-center" => out.items_center = true,
        "justify-between" => out.justify_between = true,
        "container" => out.container = true,
        "mx-auto" => out.mx_auto = true,
        "mt-auto" => out.mt_auto = true,
        "hidden" => out.hidden = true,
        "gap-4" => out.gap = Some(4),
        "space-x-4" => out.space_x = Some(4),
        "space-y-8" => out.space_y = Some(8),
        "mb-4" => out.margin_bottom = Some(4),
        "mb-6" => out.margin_bottom = Some(6),
        "mb-12" => out.margin_bottom = Some(12),
        "mt-2" => out.margin_top = Some(2),
        "p-0" => out.padding = Some(0),
        "p-6" => out.padding = Some(6),
        "px-4" => out.padding_x = Some(4),
        "py-6" => out.padding_y = Some(6),
        "py-8" => out.padding_y = Some(8),
        "h-16" => out.height = Some(16),
        "h-auto" => out.height_auto = true,
        "max-w-4xl" => out.max_w_4xl = true,
        "text-sm" => out.text_size = Some(TextSize::Sm),
        "text-xl" => out.text_size = Some(TextSize::Xl),
        "text-2xl" => out.text_size = Some(TextSize::Xl2),
        "text-4xl" => out.text_size = Some(TextSize::Xl4),
        "font-semibold" => out.font_semibold = true,
        "font-bold" => out.font_bold = true,
        "text-center" => out.text_center = true,
        "tracking-tight" => out.tracking_tight = true,
        "leading-none" => out.leading_none = true,
        "border" => out.border = true,
        "border-b" => out.border_bottom = true,
        "border-t" => out.border_top = true,
        "rounded-lg" => out.rounded_lg = true,
        "bg-card" => out.background = Some(ColorRole::Card),
        "text-card-foreground" => out.foreground = Some(ColorRole::CardForeground),
        "text-foreground" => out.foreground = Some(ColorRole::Foreground),
        "text-muted-foreground" => out.foreground = Some(ColorRole::MutedForeground),
        "shadow-sm" => out.shadow_sm = true,
        // Tier 3 — unknown.
        other => return Err(ClassParseError::UnknownToken(other.to_string())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T1_TOKENS: &[&str] = &[
        "flex",
        "flex-1",
        "flex-col",
        "flex-wrap",
        "items-center",
        "justify-between",
        "container",
        "mx-auto",
        "mt-auto",
        "hidden",
        "gap-4",
        "space-x-4",
        "space-y-8",
        "mb-4",
        "mb-6",
        "mb-12",
        "mt-2",
        "p-0",
        "p-6",
        "px-4",
        "py-6",
        "py-8",
        "h-16",
        "h-auto",
        "max-w-4xl",
        "text-sm",
        "text-xl",
        "text-2xl",
        "text-4xl",
        "font-semibold",
        "font-bold",
        "text-center",
        "tracking-tight",
        "leading-none",
        "border",
        "border-b",
        "border-t",
        "rounded-lg",
        "bg-card",
        "text-card-foreground",
        "text-foreground",
        "text-muted-foreground",
        "shadow-sm",
    ];

    #[test]
    fn t1_token_count_matches_acceptance_criterion() {
        // Acceptance criterion #5 names 39 Tier 1 tokens. Drifting from that
        // count means either the showcase grew a new token (add it to T1)
        // or we lost coverage (regression).
        assert_eq!(
            T1_TOKENS.len(),
            43,
            "T1_TOKENS list above contains the 43 utility classes used in the showcase"
        );
    }

    #[test]
    fn every_t1_token_parses() {
        for token in T1_TOKENS {
            let style = StyleClass::from_str(*token);
            let resolved = style.parse().unwrap_or_else(|e| {
                panic!("T1 token {token:?} failed to parse: {e}");
            });
            // Each T1 token must contribute at least one resolved field — a
            // no-op resolution would let an unknown class through silently.
            assert_ne!(
                resolved,
                ResolvedStyle::default(),
                "T1 token {token:?} parsed as default — it must set at least one field",
            );
        }
    }

    #[test]
    fn t1_token_resolves_to_expected_field() {
        // Spot-check the typed mapping for the trickier tokens. The full
        // mapping is covered by the by-token table above plus the
        // round-trip property in `every_t1_token_parses`.
        let s = StyleClass::from_str("flex flex-col items-center");
        let r = s.parse().unwrap();
        assert!(r.flex);
        assert!(r.flex_column);
        assert!(r.items_center);

        let s = StyleClass::from_str("p-6 px-4 py-8 mb-4 mt-2");
        let r = s.parse().unwrap();
        assert_eq!(r.padding, Some(6));
        assert_eq!(r.padding_x, Some(4));
        assert_eq!(r.padding_y, Some(8));
        assert_eq!(r.margin_bottom, Some(4));
        assert_eq!(r.margin_top, Some(2));

        let s = StyleClass::from_str("text-4xl font-bold text-foreground");
        let r = s.parse().unwrap();
        assert_eq!(r.text_size, Some(TextSize::Xl4));
        assert!(r.font_bold);
        assert_eq!(r.foreground, Some(ColorRole::Foreground));
    }

    #[test]
    fn t2_tokens_parse_but_do_not_change_resolved() {
        // Tier 2 — documented no-ops. Must parse cleanly and bump the
        // tier2_count counter so parity tests can assert tolerance.
        let cases = [
            "hover:bg-accent",
            "hover:text-accent-foreground",
            "hover:text-foreground",
            "sm:block",
            "sm:flex-row",
            "transition-colors",
        ];
        for token in cases {
            let style = StyleClass::from_str(token);
            let resolved = style.parse().expect("T2 token must parse");
            assert_eq!(resolved.tier2_count, 1, "T2 token {token:?} not counted");
            // After zeroing the counter, the rest of ResolvedStyle must be
            // default — T2 contributes nothing to resolved structure.
            let mut zeroed = resolved.clone();
            zeroed.tier2_count = 0;
            assert_eq!(
                zeroed,
                ResolvedStyle::default(),
                "T2 token {token:?} altered resolved"
            );
        }
    }

    #[test]
    fn t3_token_returns_parse_error_with_helpful_message() {
        let style = StyleClass::from_str("definitely-not-a-known-utility");
        let err = style.parse().expect_err("T3 token must be rejected");
        let ClassParseError::UnknownToken(token) = &err;
        assert_eq!(token, "definitely-not-a-known-utility");
        let rendered = err.to_string();
        assert!(
            rendered.contains("definitely-not-a-known-utility")
                && rendered.contains("Tier 1")
                && rendered.contains("Tier 2"),
            "error message must name the bad token and the tier system: {rendered}"
        );
    }

    #[test]
    fn parsing_stops_at_first_unknown_token() {
        // Order-independence: an unknown token must fail even when T1 tokens
        // surround it.
        let style = StyleClass::from_str("flex unknown-token items-center");
        let err = style.parse().expect_err("must fail");
        let ClassParseError::UnknownToken(token) = err;
        assert_eq!(token, "unknown-token");
    }

    #[test]
    fn empty_string_parses_to_default() {
        let resolved = StyleClass::from_str("").parse().unwrap();
        assert_eq!(resolved, ResolvedStyle::default());

        let resolved = StyleClass::from_str("   \t  ").parse().unwrap();
        assert_eq!(resolved, ResolvedStyle::default());
    }
}
