//! Class-string parser and the [`ResolvedStyle`] it produces.
//!
//! mkui uses Tailwind-shaped utility class strings (`"flex items-center px-4"`).
//! Until Sprint 4 the parser lived nowhere — `mkui-web` passed strings straight
//! to the DOM, the console backend ignored them, and `mkui-wgpu` was about to
//! grow a third parallel implementation. The runtime now owns the parse so
//! every binding sees the same `ResolvedStyle`, and parity tests can compare
//! the resolved structure across Rust / C / Python construction.
//!
//! ## From an allowlist to scales (#269)
//!
//! The parser used to accept exactly 39 (then 44) literal tokens and reject
//! everything else — including the README's own `justify-center` example,
//! and any typo, with a hard panic at `Mkui::child`. #269 replaces that with:
//!
//! - **Scale-based matching** — a numeric utility (`gap-*`, `p-*`, `px-*`,
//!   `py-*`, `mb-*`, `mt-*`, `h-*`, `space-x-*`, `space-y-*`) is valid for
//!   *any* non-negative integer suffix, not just the handful of literals the
//!   showcase happened to use. `justify-*` is valid for any value in a
//!   defined set (`start`/`center`/`between`/`end`/`around`/`evenly`), not
//!   just the two the showcase used.
//! - **State variants** — `hover:`/`focus:`/`disabled:`/`active:` prefix a
//!   normal color utility and resolve into a per-state override (cva-style
//!   composition) rather than being tolerated as an inert no-op. See
//!   [`StateStyle`] and [`ResolvedStyle::hover_colors`].
//! - **Non-panicking fallback** — a token that still matches nothing is
//!   dropped (logged, and counted in [`ResolvedStyle::unknown_count`])
//!   instead of failing the parse. Every *other* token on the same class
//!   string still resolves normally — `StyleClass::parse` no longer
//!   fails-fast on the first typo.
//!
//! `sm:*` (a responsive breakpoint, not a component state) and
//! `transition-colors` remain documented no-ops: they parse cleanly and
//! contribute nothing to `ResolvedStyle`, same as before #269.

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

    /// Overwrite the raw class string in place, reusing the existing
    /// buffer's capacity (`clear` + `push_str`) instead of allocating a
    /// fresh `String` — mirrors `AppTree::set_text`'s no-alloc-on-write
    /// discipline. Used by [`crate::tree::AppTree::set_class`], which
    /// compares against [`Self::raw`] *before* calling this, so this
    /// method itself never needs to check for a no-op.
    pub(crate) fn set_raw(&mut self, s: &str) {
        self.raw.clear();
        self.raw.push_str(s);
    }

    /// Parse every whitespace-separated token into a [`ResolvedStyle`].
    ///
    /// This never fails (#269): a token that matches nothing recognised is
    /// dropped — logged, and counted in [`ResolvedStyle::unknown_count`] —
    /// rather than aborting the parse, so one typo on a class string can no
    /// longer swallow every other utility on the same element. The `Result`
    /// return type is kept for source compatibility with callers that
    /// propagate `ClassParseError` from elsewhere (e.g. an unregistered
    /// component type).
    pub fn parse(&self) -> Result<ResolvedStyle, ClassParseError> {
        let mut resolved = ResolvedStyle::default();
        for token in self.raw.split_whitespace() {
            apply_token(token, &mut resolved);
        }
        Ok(resolved)
    }
}

impl fmt::Display for StyleClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Background/foreground delta applied by a state variant (`hover:`,
/// `focus:`, `disabled:`, `active:`) on top of a node's base colors.
///
/// Kept deliberately small — the showcase's state variants only ever swap
/// `bg-*`/`text-*` roles — rather than a full nested `ResolvedStyle`, so a
/// state override can't recursively carry its own state overrides.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct StateStyle {
    pub background: Option<ColorRole>,
    pub foreground: Option<ColorRole>,
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
    /// `justify-center`. Main-axis centering (added for #63 — the README's
    /// own `justify-center` example previously hit `ClassParseError`, which
    /// `push_view_unchecked`'s FFI path (`alloc_node`'s
    /// `unwrap_or_default()`) swallows silently, dropping every other class
    /// on the same string with no panic and no error).
    pub justify_center: bool,
    /// `container` / `mx-auto`. Layout-container hints.
    pub container: bool,
    pub mx_auto: bool,
    pub mt_auto: bool,
    /// `hidden`. Element is removed from layout.
    pub hidden: bool,
    /// gap-* / space-x-* / space-y-* in tailwind half-rem units. Any
    /// non-negative integer scale value is accepted (#269), not just the
    /// literals the showcase happens to use.
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
    /// Raw count of Tier 2 (no-op) tokens encountered — `sm:*` and
    /// `transition-colors`. Lets parity tests assert that T2 was tolerated,
    /// not silently dropped.
    pub tier2_count: u32,
    /// Count of tokens that matched neither a literal, a scale pattern, nor
    /// a state-variant color — dropped rather than panicking (#269). A
    /// backend or test can check this to notice a typo without parsing
    /// stderr.
    pub unknown_count: u32,
    /// `hover:*` state-variant color override. `None` unless the class
    /// string carried at least one recognised `hover:` token.
    pub hover: Option<StateStyle>,
    /// `focus:*` state-variant color override.
    pub focus: Option<StateStyle>,
    /// `disabled:*` state-variant color override.
    pub disabled: Option<StateStyle>,
    /// `active:*` state-variant color override.
    pub active: Option<StateStyle>,
}

impl ResolvedStyle {
    /// Background/foreground override declared by a `hover:` state variant,
    /// if the class string carried one. `None` when no `hover:` token was
    /// present.
    ///
    /// A backend with no pointer device (e.g. `mkui-console`, a terminal)
    /// may treat its own equivalent interaction state — the current
    /// keyboard selection — as "hovered" and apply this override only while
    /// that state holds, rather than needing real mouse-hover tracking.
    pub fn hover_colors(&self) -> Option<(Option<ColorRole>, Option<ColorRole>)> {
        self.hover.as_ref().map(|h| (h.background, h.foreground))
    }
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

/// Parse failure surfaced by [`StyleClass::parse`]'s `Result` signature.
///
/// Since #269, an unrecognised *class* token no longer produces this error —
/// it is dropped instead (see [`ResolvedStyle::unknown_count`]). The one
/// remaining source is [`crate`]-external: `mkui-core`'s `LoweringRegistry`
/// reuses this type for "no handler registered for this component type",
/// an unrelated, genuinely fatal construction error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassParseError {
    /// No `LowerToTree` handler is registered for a component type. Carries
    /// a debug-formatted description of the offending type.
    UnknownToken(String),
}

impl fmt::Display for ClassParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownToken(t) => write!(
                f,
                "no LowerToTree handler registered for component type {t} — register it via \
                 `Mkui::register::<T>()` before calling `.child()`",
            ),
        }
    }
}

impl std::error::Error for ClassParseError {}

/// Defined value set for `justify-*` (#269 acceptance criterion: a scale
/// example). Only `center`/`between` currently resolve to a `ResolvedStyle`
/// field — `mkui-layout`'s flex engine implements start/center/between —
/// but every value here is accepted as a *valid* utility rather than an
/// unknown token, which is the generalization the issue asks for.
const JUSTIFY_VALUES: &[&str] = &["start", "center", "between", "end", "around", "evenly"];

fn apply_token(token: &str, out: &mut ResolvedStyle) {
    // State variants — cva-style composition (#269). The suffix after the
    // prefix is itself a normal color utility; it resolves into a per-state
    // override rather than the base `ResolvedStyle`.
    if let Some(rest) = token.strip_prefix("hover:") {
        apply_state_variant(token, rest, &mut out.hover, &mut out.unknown_count);
        return;
    }
    if let Some(rest) = token.strip_prefix("focus:") {
        apply_state_variant(token, rest, &mut out.focus, &mut out.unknown_count);
        return;
    }
    if let Some(rest) = token.strip_prefix("disabled:") {
        apply_state_variant(token, rest, &mut out.disabled, &mut out.unknown_count);
        return;
    }
    if let Some(rest) = token.strip_prefix("active:") {
        apply_state_variant(token, rest, &mut out.active, &mut out.unknown_count);
        return;
    }

    // Tier 2 — documented no-op tokens. Recognised so the parser does not
    // drop them as unknown; renderers (notably web) may still emit them as
    // raw CSS classes via `StyleClass::raw`. `sm:*` is a responsive
    // breakpoint, not a component state, so it stays a plain no-op rather
    // than growing a variant slot like `hover:`/`focus:`/etc above.
    if token.starts_with("sm:") || token == "transition-colors" {
        out.tier2_count += 1;
        return;
    }

    match token {
        "flex" => out.flex = true,
        "flex-1" => out.flex_grow = true,
        "flex-col" => out.flex_column = true,
        "flex-wrap" => out.flex_wrap = true,
        "items-center" => out.items_center = true,
        "container" => out.container = true,
        "mx-auto" => out.mx_auto = true,
        "mt-auto" => out.mt_auto = true,
        "hidden" => out.hidden = true,
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
        other => {
            if !try_apply_scale(other, out) {
                out.unknown_count += 1;
                eprintln!(
                    "mkui-runtime: unrecognized class token `{other}` — dropped; the \
                     element's other classes still apply (#269)"
                );
            }
        }
    }
}

/// Resolve the suffix of a `hover:`/`focus:`/`disabled:`/`active:` token
/// into the given state slot. Unrecognised suffixes fall back to the same
/// non-panicking drop-and-log policy as a top-level unknown token.
fn apply_state_variant(
    full_token: &str,
    rest: &str,
    slot: &mut Option<StateStyle>,
    unknown_count: &mut u32,
) {
    let delta = match rest {
        "bg-card" => StateStyle {
            background: Some(ColorRole::Card),
            foreground: None,
        },
        "text-card-foreground" => StateStyle {
            background: None,
            foreground: Some(ColorRole::CardForeground),
        },
        "text-foreground" => StateStyle {
            background: None,
            foreground: Some(ColorRole::Foreground),
        },
        "text-muted-foreground" => StateStyle {
            background: None,
            foreground: Some(ColorRole::MutedForeground),
        },
        _ => {
            *unknown_count += 1;
            eprintln!(
                "mkui-runtime: unrecognized class token `{full_token}` — dropped; the \
                 element's other classes still apply (#269)"
            );
            return;
        }
    };
    let state = slot.get_or_insert_with(StateStyle::default);
    if delta.background.is_some() {
        state.background = delta.background;
    }
    if delta.foreground.is_some() {
        state.foreground = delta.foreground;
    }
}

/// Scale-based matching (#269): a token is valid if it matches a known
/// *pattern* rather than an exact string in a fixed list. Returns `false`
/// when nothing matches, so the caller can fall back to the unknown-token
/// policy.
fn try_apply_scale(token: &str, out: &mut ResolvedStyle) -> bool {
    if let Some(value) = token.strip_prefix("justify-") {
        return if JUSTIFY_VALUES.contains(&value) {
            match value {
                "center" => out.justify_center = true,
                "between" => out.justify_between = true,
                // start/end/around/evenly: a recognised scale value with no
                // resolved-field effect yet (mkui-layout's flex engine only
                // implements start/center/between, and start is already the
                // default) — valid, not unknown.
                _ => {}
            }
            true
        } else {
            false
        };
    }

    if let Some(n) = scale_suffix(token, "gap-") {
        out.gap = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "space-x-") {
        out.space_x = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "space-y-") {
        out.space_y = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "mb-") {
        out.margin_bottom = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "mt-") {
        out.margin_top = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "px-") {
        out.padding_x = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "py-") {
        out.padding_y = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "p-") {
        out.padding = Some(n);
        return true;
    }
    if let Some(n) = scale_suffix(token, "h-") {
        out.height = Some(n);
        return true;
    }

    false
}

/// `token.strip_prefix(prefix)` followed by a non-negative-integer parse —
/// the common shape behind every numeric scale (`gap-*`, `p-*`, `h-*`, …).
fn scale_suffix(token: &str, prefix: &str) -> Option<u32> {
    token.strip_prefix(prefix)?.parse::<u32>().ok()
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
        "justify-center",
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
        // or we lost coverage (regression). #269 generalized *how* several
        // of these resolve (scale patterns instead of literal match arms),
        // but every one of them must still parse and still resolve — this
        // list, and the behavioral guarantee below, are unchanged.
        assert_eq!(
            T1_TOKENS.len(),
            44,
            "T1_TOKENS list above contains the 44 utility classes used in the showcase \
             (43 pre-#63, plus `justify-center` — the README's own example)"
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
            assert_eq!(
                resolved.unknown_count, 0,
                "T1 token {token:?} must not be counted as unknown",
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
    fn readme_justify_center_example_parses_with_every_token_resolved() {
        // Regression for #63 acceptance criterion #7: the README's own
        // `"flex-1 items-center justify-center"` example (README.md's Rust /
        // C++ / C / Python quickstarts) must parse cleanly, and every token
        // on the string must resolve — not just the last one checked.
        let s = StyleClass::from_str("flex-1 items-center justify-center");
        let r = s.parse().unwrap();
        assert!(r.flex_grow);
        assert!(r.items_center);
        assert!(r.justify_center);
    }

    #[test]
    fn gap_and_padding_scales_accept_any_integer_not_just_the_showcase_literals() {
        // #269: `gap-4` isn't special-cased anymore — any `gap-{n}` is.
        let s = StyleClass::from_str("gap-7 p-13 mb-100 h-42 space-x-9 space-y-3");
        let r = s.parse().unwrap();
        assert_eq!(r.gap, Some(7));
        assert_eq!(r.padding, Some(13));
        assert_eq!(r.margin_bottom, Some(100));
        assert_eq!(r.height, Some(42));
        assert_eq!(r.space_x, Some(9));
        assert_eq!(r.space_y, Some(3));
        assert_eq!(r.unknown_count, 0);
    }

    #[test]
    fn justify_value_set_accepts_values_beyond_center_and_between() {
        // #269's scale example: `justify-*` is valid for any value in a
        // defined set, not just the two literals the showcase used.
        // start/end/around/evenly are accepted (not unknown) even though
        // `mkui-layout` doesn't resolve them to a distinct field yet.
        for value in ["start", "end", "around", "evenly"] {
            let s = StyleClass::from_str(format!("justify-{value}"));
            let r = s.parse().unwrap();
            assert_eq!(
                r.unknown_count, 0,
                "justify-{value} must be a recognised scale value, not unknown",
            );
        }

        let r = StyleClass::from_str("justify-nonsense").parse().unwrap();
        assert_eq!(
            r.unknown_count, 1,
            "a value outside the defined set must still be dropped as unknown",
        );
    }

    #[test]
    fn t2_tokens_parse_but_do_not_change_resolved() {
        // Tier 2 — documented no-ops. Must parse cleanly and bump the
        // tier2_count counter so parity tests can assert tolerance.
        // `hover:*` moved off this list in #269 — it now resolves
        // structurally (see `hover_state_variant_resolves_to_a_color_override`
        // below) rather than being tolerated as inert.
        let cases = ["sm:block", "sm:flex-row", "transition-colors"];
        for token in cases {
            let style = StyleClass::from_str(token);
            let resolved = style.parse().expect("T2 token must parse");
            assert_eq!(resolved.tier2_count, 1, "T2 token {token:?} not counted");
            assert_eq!(resolved.unknown_count, 0);
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
    fn hover_state_variant_resolves_to_a_color_override() {
        // #269 acceptance criterion: at least one state variant is parsed
        // into structure a backend can apply conditionally (see
        // `mkui-console`'s selected-row rendering).
        let s = StyleClass::from_str("flex hover:bg-card hover:text-foreground");
        let r = s.parse().unwrap();
        assert!(r.flex, "a hover: token must not block its sibling tokens");
        assert_eq!(
            r.hover_colors(),
            Some((Some(ColorRole::Card), Some(ColorRole::Foreground))),
        );
        // The base (non-hover) colors are untouched by the state variant.
        assert_eq!(r.background, None);
        assert_eq!(r.foreground, None);
        assert_eq!(r.unknown_count, 0);
    }

    #[test]
    fn focus_disabled_active_variants_also_parse_structurally() {
        let s = StyleClass::from_str(
            "focus:text-foreground disabled:text-muted-foreground active:bg-card",
        );
        let r = s.parse().unwrap();
        assert_eq!(
            r.focus,
            Some(StateStyle {
                background: None,
                foreground: Some(ColorRole::Foreground),
            })
        );
        assert_eq!(
            r.disabled,
            Some(StateStyle {
                background: None,
                foreground: Some(ColorRole::MutedForeground),
            })
        );
        assert_eq!(
            r.active,
            Some(StateStyle {
                background: Some(ColorRole::Card),
                foreground: None,
            })
        );
        assert_eq!(r.unknown_count, 0);
    }

    #[test]
    fn state_variant_with_unrecognised_suffix_is_dropped_not_panicking() {
        let s = StyleClass::from_str("hover:not-a-real-utility");
        let r = s.parse().unwrap();
        assert_eq!(r.hover, None);
        assert_eq!(r.unknown_count, 1);
    }

    #[test]
    fn unknown_token_is_dropped_with_other_tokens_still_resolved() {
        // #269: the single highest-priority change. An unrecognized token
        // no longer fails the parse — it's dropped (counted), and every
        // other token on the same string still resolves, regardless of
        // whether it comes before or after the unknown one.
        let s = StyleClass::from_str("flex definitely-not-a-known-utility items-center");
        let r = s
            .parse()
            .expect("parse must never fail on an unknown token");
        assert!(r.flex, "token before the unknown one must still resolve");
        assert!(
            r.items_center,
            "token after the unknown one must still resolve"
        );
        assert_eq!(r.unknown_count, 1);
    }

    #[test]
    fn multiple_unknown_tokens_are_each_counted() {
        let s = StyleClass::from_str("nope-one flex nope-two nope-three");
        let r = s.parse().unwrap();
        assert!(r.flex);
        assert_eq!(r.unknown_count, 3);
    }

    #[test]
    fn empty_string_parses_to_default() {
        let resolved = StyleClass::from_str("").parse().unwrap();
        assert_eq!(resolved, ResolvedStyle::default());

        let resolved = StyleClass::from_str("   \t  ").parse().unwrap();
        assert_eq!(resolved, ResolvedStyle::default());
    }
}
