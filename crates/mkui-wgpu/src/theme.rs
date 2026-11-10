//! Backend theme tokens and variant resolvers for mkui-wgpu.
//! `WgpuTheme` maps semantic tokens and shadcn-style variants into concrete
//! colors, typography, spacing, and component chrome.
//!
//! The shape follows [shadcn/ui] and [`miklabs/ui`] conventions: widgets take
//! a `variant` and a `size`, and the theme resolves the pair to a concrete
//! `ButtonStyle` / `SliderStyle` / etc. That keeps widget call sites
//! declarative (`button(..., ButtonVariant::Outline, ButtonSize::Sm, ...)`)
//! while all color tokens live in one place.
//!
//! State (`ButtonState::Idle` / `ButtonState::Active`) is orthogonal to
//! variant — it represents whether the widget is currently picked / selected
//! / toggled on, and is supplied by the caller each frame.
//!
//! [shadcn/ui]: https://ui.shadcn.com/
//! [`miklabs/ui`]: https://github.com/mikbry/ui

use crate::types::FontFaceId;
use crate::types::{Color, Insets, Stroke, TextAlign, TextStyle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelStyle {
    pub fill: Color,
    pub stroke: Stroke,
    pub shadow: ShadowStyle,
    pub corner_radius: f32,
    pub padding: Insets,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    pub color: Color,
    pub blur_radius: f32,
    pub spread: f32,
}

/// Root theme for the wgpu UI backend. A thin wrapper around [`ThemeTokens`]
/// — `tokens` is the *only* stored state. `panel_compact` / `panel_style` /
/// `title_style` / `body_style` / `button_style` / … are all methods that
/// derive their result from `self.tokens` on every call, never a cached
/// field, so swapping `self.tokens` (directly, or by assigning a whole new
/// `WgpuTheme`) can never leave some other field stale (issue #205 Codex
/// round 1 — an earlier revision cached `panel`/`title_style`/`body_style`
/// as plain fields computed once in `Default::default()`, which went stale
/// the moment a caller swapped only `tokens`).
///
/// The defaults target a warm editorial control-surface look; a future
/// light-mode or per-product palette swap only needs a different
/// `ThemeTokens` value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WgpuTheme {
    pub tokens: ThemeTokens,
}

/// Semantic color + geometry tokens that variant resolvers consume. Mirrors
/// the shadcn CSS variable palette (`--primary`, `--muted`, `--ring`, …) in
/// struct form — no utility-string parsing, no runtime theming layer yet.
///
/// Every color or spacing value a `WgpuTheme` resolver method
/// (`button_style`, `badge_style`, `panel_style`, …) hands a widget comes
/// from one of this struct's fields — never an inline literal — so a host
/// can swap the active palette by constructing a different `ThemeTokens`
/// and handing it to [`WgpuTheme`] (see issue #205). The only values a
/// resolver may still reach for outside `self.tokens` are
/// [`Color::TRANSPARENT`] (the "no paint" sentinel, not a themed value)
/// and widget-intrinsic size-ladder numbers (e.g. [`DotSize::diameter`])
/// that are a function of the size enum, not the palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    pub primary: Color,
    pub primary_foreground: Color,
    pub primary_ring: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub surface: Color,
    pub surface_foreground: Color,
    pub border: Color,
    pub destructive: Color,
    pub destructive_foreground: Color,
    pub secondary: Color,
    pub secondary_foreground: Color,
    pub success: Color,
    pub success_foreground: Color,
    pub warning: Color,
    pub warning_foreground: Color,
    pub card_fill: Color,
    pub card_border: Color,
    /// Base tone for body copy; `Body`/`Muted`/`Caption` text variants are
    /// this color at different alpha.
    pub body_foreground: Color,
    /// `ButtonVariant::Default`'s idle fill/label — distinct enough from
    /// `card_fill` / `muted_foreground` to warrant its own tokens rather
    /// than a derived alpha.
    pub button_default_idle_fill: Color,
    pub button_default_idle_label: Color,
    pub button_secondary_idle_fill: Color,
    pub button_ghost_idle_fill: Color,
    pub button_ghost_active_fill: Color,
    /// Dark caption color painted on top of an active/primary fill.
    pub caption_on_primary: Color,
    /// Brighter fill used by a selected [`CardStyle`] and an active
    /// [`InputStyle`] — an "elevated surface" tone.
    pub card_selected_fill: Color,
    pub input_active_fill: Color,
    /// Recessed-surface tone shared by the slider track and an idle
    /// (unfocused) text input.
    pub surface_sunken: Color,
    pub scrollbar_track: Color,
    pub swatch_stroke: Color,
    pub shadow: ShadowStyle,
    /// Named geometry scale (corner radii, hairline stroke width, panel
    /// padding) — the "spacing" half of issue #205's color-and-spacing
    /// token set.
    pub spacing: SpacingTokens,
}

/// Named geometry scale consumed by [`WgpuTheme`]'s resolver methods in
/// place of inline corner-radius / stroke-width / padding literals. Values
/// mirror a small Tailwind-style scale (`rounded-sm` .. `rounded-2xl`)
/// rather than one bespoke number per widget, so most widgets share one of
/// a handful of named radii.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingTokens {
    /// Hairline border/stroke width used by nearly every stroked widget.
    pub stroke_width_hairline: f32,
    pub radius_tight: f32,
    pub radius_compact: f32,
    pub radius_card: f32,
    pub radius_relaxed: f32,
    pub radius_swatch: f32,
    pub radius_panel: f32,
    pub padding_panel: f32,
    pub radius_panel_roomy: f32,
    pub padding_panel_roomy: f32,
}

/// Two-state marker for interactive widgets. `Active` means "currently
/// selected / pressed / toggled on" (shadcn calls this `data-state=open` on
/// Toggle / Checkbox). It is orthogonal to `ButtonVariant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    Idle,
    Active,
}

/// Button look-and-feel variant, matching shadcn's six canonical variants.
///
/// - `Default` — prominent filled button (primary action).
/// - `Destructive` — reserved for delete / remove (red tint).
/// - `Outline` — transparent fill, visible border. Used by inspector chips.
/// - `Secondary` — muted filled button.
/// - `Ghost` — no fill, hover/active only. Used by inspector steppers and
///   the explorer toggle.
/// - `Link` — text-only, no chrome. Reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Default,
    Destructive,
    Outline,
    Secondary,
    Ghost,
    Link,
}

/// Button size, matching shadcn's four canonical sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Default,
    Sm,
    Lg,
    Icon,
}

/// Text semantic variant, matching the heading/body ladder in shadcn
/// typography. Resolved by [`WgpuTheme::text_style`] into a concrete
/// [`TextStyle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextVariant {
    /// Panel / card title (used as `title_style`).
    Heading,
    /// Amber-tinted subheading used inside panels (e.g. "Terrain patch").
    Subheading,
    /// Default paragraph body copy.
    Body,
    /// Dimmer, smaller body text for secondary lines (row details, etc.).
    Muted,
    /// Very small, end-aligned caption (toolbar shortcut digits).
    Caption,
}

/// Concrete, resolved button look-and-feel. Lower-level widgets
/// (`widgets::button_with`) take this directly. Higher-level widgets take
/// `(variant, size)` and resolve through [`WgpuTheme::button_style`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonStyle {
    pub idle_fill: Color,
    pub idle_stroke: Color,
    pub idle_label: Color,
    pub active_fill: Color,
    pub active_stroke: Color,
    pub active_label: Color,
    pub corner_radius: f32,
    pub stroke_width: f32,
    pub label_style: TextStyle,
}

/// Concrete, resolved slider look-and-feel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderStyle {
    pub track_fill: Color,
    pub track_stroke: Color,
    pub track_stroke_width: f32,
    pub track_corner_radius: f32,
    pub filled_fill: Color,
    pub thumb_fill: Color,
    pub thumb_stroke: Color,
    pub thumb_diameter: f32,
    /// Extra vertical padding added to the hit rect so the control is easy
    /// to grab — rendered track height stays at `rect.size.height`.
    pub hit_padding: f32,
}

/// Concrete, resolved text input look-and-feel. This is a lightweight
/// shadcn "Input" analogue for immediate-mode UI fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputStyle {
    pub fill: Color,
    pub stroke: Color,
    pub text_color: Color,
    pub text_style: TextStyle,
    pub corner_radius: f32,
    pub stroke_width: f32,
    pub padding: Insets,
    /// Background of the selection highlight drawn behind selected text
    /// (#185). Translucent so the text color remains legible on top.
    pub selection_fill: Color,
    /// Color of the caret bar and of the underline drawn beneath an
    /// in-progress IME composition (#185).
    pub caret_color: Color,
}

/// Concrete, resolved scrollbar look-and-feel. The caller supplies the
/// precomputed track and thumb rects; the widget just emits the pill quads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarStyle {
    pub track_fill: Color,
    pub thumb_fill: Color,
}

/// Concrete, resolved color swatch look-and-feel. Used by list rows and
/// material/family chips that need a tiny color sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwatchStyle {
    pub stroke: Color,
    pub stroke_width: f32,
    pub corner_radius: f32,
}

/// Rounded card chrome used by inspector rows, explorer rows, hint bars,
/// and every other boxed element that is not a full overlay panel. This is
/// the shadcn "Card" primitive in resolved form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardStyle {
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f32,
    pub corner_radius: f32,
}

/// Badge look-and-feel variant, matching shadcn's six canonical
/// non-interactive badge variants 1:1. Product-specific signals (state
/// pills, role tags, tier markers) compose a badge in downstream crates;
/// they do not extend this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BadgeVariant {
    Default,
    Destructive,
    Outline,
    Secondary,
    Ghost,
    Link,
}

/// Badge size. `Default` is ~22px tall (paired with body text); `Sm` is
/// ~14px for dense tables and list rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    Default,
    Sm,
}

/// Concrete, resolved badge look-and-feel. Atoms (`components::badge`) take
/// `(variant, size)` and resolve through [`WgpuTheme::badge_style`]; this
/// struct is the cva-style output the atom paints from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeStyle {
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f32,
    pub label_color: Color,
    pub corner_radius: f32,
    pub padding: Insets,
    pub label_style: TextStyle,
}

/// Dot status-color variant. Tokens only — application semantics (what
/// "ok" means in a given product) are decided by the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DotVariant {
    Ok,
    Warn,
    Danger,
    Neutral,
}

/// Dot diameter. `Sm` = 6px, `Md` = 8px — the two sizes downstream rows and
/// status pips need; larger pips compose with `Badge` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotSize {
    Sm,
    Md,
}

impl DotSize {
    pub fn diameter(self) -> f32 {
        match self {
            DotSize::Sm => 6.0,
            DotSize::Md => 8.0,
        }
    }
}

/// Concrete, resolved dot look-and-feel. The halo alpha is pre-resolved
/// here so the atom can decide whether to emit the ring without re-reading
/// theme tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotStyle {
    pub fill: Color,
    pub halo: Color,
    pub diameter: f32,
}

impl Default for WgpuTheme {
    fn default() -> Self {
        let spacing = SpacingTokens {
            stroke_width_hairline: 1.0,
            radius_tight: 4.0,
            radius_compact: 6.0,
            radius_card: 8.0,
            radius_relaxed: 10.0,
            radius_swatch: 3.0,
            radius_panel: 14.0,
            padding_panel: 14.0,
            radius_panel_roomy: 16.0,
            padding_panel_roomy: 16.0,
        };
        let tokens = ThemeTokens {
            primary: Color::rgba(0.94, 0.79, 0.48, 0.92),
            primary_foreground: Color::rgba(0.12, 0.09, 0.06, 1.0),
            primary_ring: Color::rgba(0.99, 0.91, 0.74, 1.0),
            muted: Color::rgba(0.18, 0.14, 0.10, 0.96),
            muted_foreground: Color::rgba(0.99, 0.95, 0.88, 0.94),
            surface: Color::rgba(0.08, 0.07, 0.06, 0.9),
            surface_foreground: Color::rgba(0.97, 0.93, 0.86, 1.0),
            border: Color::rgba(0.96, 0.93, 0.88, 0.16),
            destructive: Color::rgba(0.72, 0.28, 0.22, 0.92),
            destructive_foreground: Color::rgba(0.99, 0.93, 0.90, 1.0),
            secondary: Color::rgba(0.20, 0.17, 0.14, 0.96),
            secondary_foreground: Color::rgba(0.98, 0.94, 0.88, 0.96),
            success: Color::rgba(0.36, 0.74, 0.45, 0.96),
            success_foreground: Color::rgba(0.05, 0.18, 0.09, 1.0),
            warning: Color::rgba(0.95, 0.74, 0.30, 0.96),
            warning_foreground: Color::rgba(0.20, 0.13, 0.04, 1.0),
            card_fill: Color::rgba(0.12, 0.10, 0.09, 0.92),
            card_border: Color::rgba(0.96, 0.93, 0.88, 0.08),
            body_foreground: Color::rgba(0.92, 0.88, 0.82, 1.0),
            button_default_idle_fill: Color::rgba(0.14, 0.12, 0.11, 0.92),
            button_default_idle_label: Color::rgba(0.98, 0.94, 0.88, 0.96),
            button_secondary_idle_fill: Color::rgba(0.18, 0.15, 0.12, 0.95),
            button_ghost_idle_fill: Color::rgba(0.20, 0.16, 0.12, 0.96),
            button_ghost_active_fill: Color::rgba(0.28, 0.22, 0.16, 0.96),
            caption_on_primary: Color::rgba(0.16, 0.10, 0.04, 1.0),
            card_selected_fill: Color::rgba(0.22, 0.18, 0.13, 0.96),
            input_active_fill: Color::rgba(0.22, 0.18, 0.12, 0.98),
            surface_sunken: Color::rgba(0.08, 0.06, 0.05, 0.96),
            scrollbar_track: Color::rgba(0.04, 0.03, 0.02, 0.55),
            swatch_stroke: Color::rgba(0.0, 0.0, 0.0, 0.35),
            shadow: ShadowStyle {
                color: Color::rgba(0.0, 0.0, 0.0, 0.38),
                blur_radius: 24.0,
                spread: 0.0,
            },
            spacing,
        };
        Self { tokens }
    }
}

impl WgpuTheme {
    /// Compact panel chrome (`radius_panel` / `padding_panel` — 14px by
    /// default): the baseline used by `info_list` / `titled_panel` call
    /// sites that want a tighter chrome than [`Self::panel_style`]'s
    /// roomier 16px preset. Both derive live from `self.tokens`.
    pub fn panel_compact(&self) -> PanelStyle {
        PanelStyle {
            fill: self.tokens.surface,
            stroke: Stroke {
                color: self.tokens.border,
                width: self.tokens.spacing.stroke_width_hairline,
            },
            shadow: self.tokens.shadow,
            corner_radius: self.tokens.spacing.radius_panel,
            padding: Insets::all(self.tokens.spacing.padding_panel),
        }
    }

    /// Heading-ladder text style (used as the panel title). Derives live
    /// from `self.tokens` — see the [`WgpuTheme`] doc comment.
    pub fn title_style(&self) -> TextStyle {
        TextStyle {
            font: FontFaceId(0),
            font_size_px: 15.0,
            line_height_px: 20.0,
            color: self.tokens.surface_foreground,
            align: TextAlign::Start,
        }
    }

    /// Default paragraph body text style. Derives live from `self.tokens`
    /// — see the [`WgpuTheme`] doc comment.
    pub fn body_style(&self) -> TextStyle {
        TextStyle {
            font: FontFaceId(0),
            font_size_px: 13.0,
            line_height_px: 18.0,
            color: self.tokens.body_foreground.with_alpha(0.92),
            align: TextAlign::Start,
        }
    }

    /// Baseline panel chrome (inspector, explorer, hint bar, toolbar, and
    /// other surface containers). Callers override padding / corner_radius as
    /// needed.
    pub fn panel_style(&self) -> PanelStyle {
        PanelStyle {
            fill: self.tokens.surface,
            stroke: Stroke {
                color: self.tokens.border,
                width: self.tokens.spacing.stroke_width_hairline,
            },
            shadow: self.tokens.shadow,
            corner_radius: self.tokens.spacing.radius_panel_roomy,
            padding: Insets::all(self.tokens.spacing.padding_panel_roomy),
        }
    }

    /// Rounded card chrome used for list rows and inline boxes. This is the
    /// shadcn "Card" primitive.
    pub fn card(&self) -> CardStyle {
        CardStyle {
            fill: self.tokens.card_fill,
            stroke: self.tokens.card_border,
            stroke_width: self.tokens.spacing.stroke_width_hairline,
            corner_radius: self.tokens.spacing.radius_card,
        }
    }

    /// Selected variant of [`Self::card`] — brighter fill and border so the
    /// row reads as picked.
    pub fn card_selected(&self) -> CardStyle {
        CardStyle {
            fill: self.tokens.card_selected_fill,
            stroke: self.tokens.border.multiply_alpha(3.4),
            stroke_width: self.tokens.spacing.stroke_width_hairline,
            corner_radius: self.tokens.spacing.radius_card,
        }
    }

    /// Default slider look. A "primary"-tinted fill bar on a dark track.
    pub fn slider(&self) -> SliderStyle {
        SliderStyle {
            track_fill: self.tokens.surface_sunken,
            track_stroke: self.tokens.border.with_alpha(0.18),
            track_stroke_width: self.tokens.spacing.stroke_width_hairline,
            track_corner_radius: self.tokens.spacing.radius_tight,
            filled_fill: self.tokens.primary.with_alpha(0.88),
            thumb_fill: self.tokens.primary_ring,
            thumb_stroke: self.tokens.surface,
            thumb_diameter: 14.0,
            hit_padding: 8.0,
        }
    }

    /// Text input chrome. `active` means focused for keyboard editing.
    pub fn input(&self, active: bool) -> InputStyle {
        if active {
            InputStyle {
                fill: self.tokens.input_active_fill,
                stroke: self.tokens.primary,
                text_color: self.tokens.muted_foreground.with_alpha(1.0),
                text_style: self.body_style(),
                corner_radius: self.tokens.spacing.radius_compact,
                stroke_width: self.tokens.spacing.stroke_width_hairline,
                padding: Insets::symmetric(6.0, 1.0),
                selection_fill: self.tokens.primary.multiply_alpha(0.4),
                caret_color: self.tokens.primary_ring,
            }
        } else {
            InputStyle {
                fill: self.tokens.surface_sunken,
                stroke: self.tokens.border.with_alpha(0.22),
                text_color: self.tokens.muted_foreground.with_alpha(1.0),
                text_style: self.body_style(),
                corner_radius: self.tokens.spacing.radius_compact,
                stroke_width: self.tokens.spacing.stroke_width_hairline,
                selection_fill: self.tokens.primary.multiply_alpha(0.4),
                caret_color: self.tokens.primary_ring,
                padding: Insets::symmetric(6.0, 1.0),
            }
        }
    }

    /// Default scrollbar look. Pill thumb on a faint channel.
    pub fn scrollbar(&self) -> ScrollbarStyle {
        ScrollbarStyle {
            track_fill: self.tokens.scrollbar_track,
            thumb_fill: self.tokens.border.with_alpha(0.55),
        }
    }

    /// Default swatch chrome.
    pub fn swatch(&self) -> SwatchStyle {
        SwatchStyle {
            stroke: self.tokens.swatch_stroke,
            stroke_width: self.tokens.spacing.stroke_width_hairline,
            corner_radius: self.tokens.spacing.radius_swatch,
        }
    }

    /// Resolve `(variant, size)` to a concrete [`ButtonStyle`]. The two axes
    /// are combined cva-style: the variant picks the color palette, the size
    /// picks corner radius / stroke width / label text style.
    pub fn button_style(&self, variant: ButtonVariant, size: ButtonSize) -> ButtonStyle {
        let hairline = self.tokens.spacing.stroke_width_hairline;
        let (corner_radius, stroke_width, label_size) = match size {
            ButtonSize::Default => (self.tokens.spacing.radius_card, hairline, self.body_style()),
            ButtonSize::Sm => (
                self.tokens.spacing.radius_compact,
                hairline,
                self.body_style(),
            ),
            ButtonSize::Lg => (
                self.tokens.spacing.radius_relaxed,
                hairline,
                self.body_style(),
            ),
            ButtonSize::Icon => (
                self.tokens.spacing.radius_tight,
                hairline,
                self.body_style(),
            ),
        };
        let label_style = TextStyle {
            align: TextAlign::Center,
            ..label_size
        };

        match variant {
            ButtonVariant::Default => ButtonStyle {
                idle_fill: self.tokens.button_default_idle_fill,
                idle_stroke: self.tokens.border.with_alpha(0.14),
                idle_label: self.tokens.button_default_idle_label,
                active_fill: self.tokens.primary,
                active_stroke: self.tokens.primary_ring,
                active_label: self.tokens.primary_foreground,
                corner_radius,
                stroke_width,
                label_style,
            },
            ButtonVariant::Destructive => ButtonStyle {
                idle_fill: self.tokens.destructive.multiply_alpha(0.6),
                idle_stroke: self.tokens.destructive,
                idle_label: self.tokens.destructive_foreground,
                active_fill: self.tokens.destructive,
                active_stroke: self.tokens.destructive_foreground,
                active_label: self.tokens.destructive_foreground,
                corner_radius,
                stroke_width,
                label_style,
            },
            ButtonVariant::Outline => ButtonStyle {
                idle_fill: self.tokens.muted,
                idle_stroke: self.tokens.border.with_alpha(0.22),
                idle_label: self.tokens.muted_foreground,
                active_fill: self.tokens.primary.multiply_alpha(0.96),
                active_stroke: self.tokens.surface,
                active_label: self.tokens.primary_foreground,
                corner_radius: corner_radius.max(self.tokens.spacing.radius_relaxed),
                stroke_width,
                label_style,
            },
            ButtonVariant::Secondary => ButtonStyle {
                idle_fill: self.tokens.button_secondary_idle_fill,
                idle_stroke: self.tokens.border.with_alpha(0.24),
                idle_label: self.tokens.surface_foreground,
                active_fill: self.tokens.primary,
                active_stroke: self.tokens.primary_ring,
                active_label: self.tokens.primary_foreground,
                corner_radius: corner_radius.max(self.tokens.spacing.radius_compact),
                stroke_width,
                label_style,
            },
            ButtonVariant::Ghost => ButtonStyle {
                idle_fill: self.tokens.button_ghost_idle_fill,
                idle_stroke: self.tokens.border.with_alpha(0.22),
                idle_label: self.tokens.muted_foreground.with_alpha(1.0),
                active_fill: self.tokens.button_ghost_active_fill,
                active_stroke: self.tokens.primary_ring,
                // Ghost keeps a dark active fill, so the label must stay on
                // a light token — `surface` would collapse to dark-on-dark
                // (the collapsed explorer toggle's "+" becomes unreadable).
                active_label: self.tokens.surface_foreground,
                corner_radius,
                stroke_width,
                label_style,
            },
            ButtonVariant::Link => ButtonStyle {
                idle_fill: Color::TRANSPARENT,
                idle_stroke: Color::TRANSPARENT,
                idle_label: self.tokens.primary,
                active_fill: Color::TRANSPARENT,
                active_stroke: Color::TRANSPARENT,
                active_label: self.tokens.primary_ring,
                corner_radius: 0.0,
                stroke_width: 0.0,
                label_style,
            },
        }
    }

    /// Resolve `(variant, size)` to a concrete [`BadgeStyle`]. Mirrors the
    /// cva-style shape of [`Self::button_style`]: variant picks colour
    /// tokens, size picks corner radius / padding / label text style.
    pub fn badge_style(&self, variant: BadgeVariant, size: BadgeSize) -> BadgeStyle {
        let (corner_radius, padding, label_text) = match size {
            BadgeSize::Default => (
                self.tokens.spacing.radius_compact,
                Insets::symmetric(8.0, 2.0),
                TextStyle {
                    align: TextAlign::Center,
                    ..self.body_style()
                },
            ),
            BadgeSize::Sm => (
                self.tokens.spacing.radius_tight,
                Insets::symmetric(6.0, 1.0),
                TextStyle {
                    font_size_px: 10.0,
                    line_height_px: 12.0,
                    align: TextAlign::Center,
                    ..self.body_style()
                },
            ),
        };

        match variant {
            BadgeVariant::Default => BadgeStyle {
                fill: self.tokens.primary,
                stroke: Color::TRANSPARENT,
                stroke_width: 0.0,
                label_color: self.tokens.primary_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Destructive => BadgeStyle {
                fill: self.tokens.destructive,
                stroke: Color::TRANSPARENT,
                stroke_width: 0.0,
                label_color: self.tokens.destructive_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Outline => BadgeStyle {
                fill: Color::TRANSPARENT,
                stroke: self.tokens.border,
                stroke_width: self.tokens.spacing.stroke_width_hairline,
                label_color: self.tokens.muted_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Secondary => BadgeStyle {
                fill: self.tokens.secondary,
                stroke: Color::TRANSPARENT,
                stroke_width: 0.0,
                label_color: self.tokens.secondary_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Ghost => BadgeStyle {
                fill: Color::TRANSPARENT,
                stroke: Color::TRANSPARENT,
                stroke_width: 0.0,
                label_color: self.tokens.muted_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            // TODO: emit a hover-underline once the SDF text path lands (the
            // current bitmap fallback in `tessellation.rs` has no underline
            // primitive). Colours + structure match shadcn's Button Link.
            BadgeVariant::Link => BadgeStyle {
                fill: Color::TRANSPARENT,
                stroke: Color::TRANSPARENT,
                stroke_width: 0.0,
                label_color: self.tokens.primary,
                corner_radius: 0.0,
                padding,
                label_style: label_text,
            },
        }
    }

    /// Resolve `(variant, size)` to a concrete [`DotStyle`]. The halo
    /// colour is the variant fill at reduced alpha so the optional ring
    /// reads as the same semantic without a second token lookup.
    pub fn dot_style(&self, variant: DotVariant, size: DotSize) -> DotStyle {
        let fill = match variant {
            DotVariant::Ok => self.tokens.success,
            DotVariant::Warn => self.tokens.warning,
            DotVariant::Danger => self.tokens.destructive,
            DotVariant::Neutral => self.tokens.muted_foreground,
        };
        DotStyle {
            fill,
            halo: fill.multiply_alpha(0.35),
            diameter: size.diameter(),
        }
    }

    /// Resolve a semantic [`TextVariant`] to a concrete [`TextStyle`].
    pub fn text_style(&self, variant: TextVariant) -> TextStyle {
        match variant {
            TextVariant::Heading => self.title_style(),
            TextVariant::Subheading => TextStyle {
                color: self.tokens.primary_ring,
                ..self.title_style()
            },
            TextVariant::Body => self.body_style(),
            TextVariant::Muted => TextStyle {
                color: self.tokens.body_foreground.with_alpha(0.72),
                ..self.body_style()
            },
            TextVariant::Caption => TextStyle {
                color: self.tokens.body_foreground.with_alpha(0.65),
                font_size_px: 10.0,
                line_height_px: 12.0,
                align: TextAlign::End,
                ..self.body_style()
            },
        }
    }

    /// Resolve a semantic `TextVariant` for the `Active` state — used when
    /// the caller flips text color with the widget's active state (e.g. the
    /// toolbar shortcut digit darkens against the active fill).
    pub fn text_style_active(&self, variant: TextVariant) -> TextStyle {
        let base = self.text_style(variant);
        match variant {
            TextVariant::Caption => TextStyle {
                color: self.tokens.caption_on_primary.with_alpha(0.85),
                ..base
            },
            _ => TextStyle {
                color: self.tokens.primary_foreground,
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_style_flips_fill_on_variant() {
        let theme = WgpuTheme::default();
        let default = theme.button_style(ButtonVariant::Default, ButtonSize::Default);
        let outline = theme.button_style(ButtonVariant::Outline, ButtonSize::Default);
        assert_ne!(
            default.idle_fill, outline.idle_fill,
            "Default and Outline should resolve to different idle fills"
        );
    }

    #[test]
    fn button_style_respects_size() {
        let theme = WgpuTheme::default();
        let sm = theme.button_style(ButtonVariant::Default, ButtonSize::Sm);
        let lg = theme.button_style(ButtonVariant::Default, ButtonSize::Lg);
        assert!(sm.corner_radius < lg.corner_radius);
    }

    #[test]
    fn text_variant_muted_is_dimmer_than_body() {
        let theme = WgpuTheme::default();
        let body = theme.text_style(TextVariant::Body);
        let muted = theme.text_style(TextVariant::Muted);
        assert!(muted.color.a < body.color.a);
    }

    #[test]
    fn card_selected_differs_from_card() {
        let theme = WgpuTheme::default();
        assert_ne!(theme.card().fill, theme.card_selected().fill);
    }

    /// Rough perceptual luma — enough to catch dark-label-on-dark-fill bugs
    /// without pulling in a real sRGB → linear pipeline for tests.
    fn luma(color: Color) -> f32 {
        0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
    }

    #[test]
    fn button_active_label_contrasts_with_active_fill() {
        // Regression: the Ghost variant used to resolve `active_label` to
        // `tokens.surface`, which is dark — putting dark text on the dark
        // `active_fill`. The collapsed explorer toggle's "+" became unreadable.
        // Every variant's active_label must land on the opposite side of
        // mid-luma from its active_fill.
        let theme = WgpuTheme::default();
        for variant in [
            ButtonVariant::Default,
            ButtonVariant::Destructive,
            ButtonVariant::Outline,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
        ] {
            let style = theme.button_style(variant, ButtonSize::Default);
            let fill_luma = luma(style.active_fill);
            let label_luma = luma(style.active_label);
            assert!(
                (fill_luma - label_luma).abs() > 0.25,
                "{variant:?}: active label luma {label_luma:.3} too close to fill luma {fill_luma:.3}"
            );
        }
    }

    /// A second, fully distinct token set — every color and every spacing
    /// value differs from [`WgpuTheme::default`]. Lives only in this test
    /// module: issue #205 asks for the swap *mechanism*, not a second
    /// built-in palette, so this "neon" set is never exposed as public API.
    fn alt_theme() -> WgpuTheme {
        let spacing = SpacingTokens {
            stroke_width_hairline: 2.0,
            radius_tight: 1.0,
            radius_compact: 2.0,
            radius_card: 3.0,
            radius_relaxed: 5.0,
            radius_swatch: 1.0,
            radius_panel: 7.0,
            padding_panel: 9.0,
            radius_panel_roomy: 11.0,
            padding_panel_roomy: 13.0,
        };
        let tokens = ThemeTokens {
            primary: Color::rgba(0.10, 0.50, 0.90, 1.0),
            primary_foreground: Color::rgba(1.0, 1.0, 1.0, 1.0),
            primary_ring: Color::rgba(0.20, 0.60, 1.0, 1.0),
            muted: Color::rgba(0.85, 0.85, 0.90, 1.0),
            muted_foreground: Color::rgba(0.10, 0.10, 0.15, 1.0),
            surface: Color::rgba(0.95, 0.95, 0.98, 1.0),
            surface_foreground: Color::rgba(0.05, 0.05, 0.08, 1.0),
            border: Color::rgba(0.60, 0.60, 0.65, 1.0),
            destructive: Color::rgba(0.95, 0.10, 0.10, 1.0),
            destructive_foreground: Color::rgba(1.0, 1.0, 1.0, 1.0),
            secondary: Color::rgba(0.80, 0.80, 0.85, 1.0),
            secondary_foreground: Color::rgba(0.05, 0.05, 0.10, 1.0),
            success: Color::rgba(0.10, 0.80, 0.30, 1.0),
            success_foreground: Color::rgba(1.0, 1.0, 1.0, 1.0),
            warning: Color::rgba(0.95, 0.60, 0.05, 1.0),
            warning_foreground: Color::rgba(0.0, 0.0, 0.0, 1.0),
            card_fill: Color::rgba(0.90, 0.92, 0.97, 1.0),
            card_border: Color::rgba(0.55, 0.55, 0.60, 1.0),
            body_foreground: Color::rgba(0.15, 0.15, 0.20, 1.0),
            button_default_idle_fill: Color::rgba(0.70, 0.75, 0.90, 1.0),
            button_default_idle_label: Color::rgba(0.02, 0.02, 0.05, 1.0),
            button_secondary_idle_fill: Color::rgba(0.65, 0.68, 0.80, 1.0),
            button_ghost_idle_fill: Color::rgba(0.88, 0.90, 0.95, 1.0),
            button_ghost_active_fill: Color::rgba(0.55, 0.60, 0.80, 1.0),
            caption_on_primary: Color::rgba(1.0, 1.0, 1.0, 1.0),
            card_selected_fill: Color::rgba(0.75, 0.80, 0.95, 1.0),
            input_active_fill: Color::rgba(0.85, 0.88, 0.98, 1.0),
            surface_sunken: Color::rgba(0.80, 0.82, 0.90, 1.0),
            scrollbar_track: Color::rgba(0.75, 0.75, 0.80, 1.0),
            swatch_stroke: Color::rgba(0.30, 0.30, 0.35, 1.0),
            shadow: ShadowStyle {
                color: Color::rgba(0.20, 0.20, 0.30, 0.50),
                blur_radius: 6.0,
                spread: 2.0,
            },
            spacing,
        };
        WgpuTheme { tokens }
    }

    /// Acceptance criterion #205.2: swapping the active `ThemeTokens` must
    /// change every resolver's output with no code change at the call
    /// site — the same `button_style`/`badge_style`/… functions, called
    /// with a different `WgpuTheme`, must paint differently.
    #[test]
    fn swapping_token_set_changes_every_resolver_output() {
        let default_theme = WgpuTheme::default();
        let swapped = alt_theme();
        assert_ne!(
            default_theme, swapped,
            "the two token sets must actually differ"
        );

        assert_ne!(default_theme.panel_style(), swapped.panel_style());
        assert_ne!(default_theme.panel_compact(), swapped.panel_compact());
        assert_ne!(default_theme.title_style(), swapped.title_style());
        assert_ne!(default_theme.body_style(), swapped.body_style());
        assert_ne!(default_theme.card(), swapped.card());
        assert_ne!(default_theme.card_selected(), swapped.card_selected());
        assert_ne!(default_theme.slider(), swapped.slider());
        assert_ne!(default_theme.input(true), swapped.input(true));
        assert_ne!(default_theme.input(false), swapped.input(false));
        assert_ne!(default_theme.scrollbar(), swapped.scrollbar());
        assert_ne!(default_theme.swatch(), swapped.swatch());

        for variant in [
            ButtonVariant::Default,
            ButtonVariant::Destructive,
            ButtonVariant::Outline,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
        ] {
            for size in [
                ButtonSize::Default,
                ButtonSize::Sm,
                ButtonSize::Lg,
                ButtonSize::Icon,
            ] {
                assert_ne!(
                    default_theme.button_style(variant, size),
                    swapped.button_style(variant, size),
                    "button_style({variant:?}, {size:?}) did not change on swap"
                );
            }
        }

        for variant in [
            BadgeVariant::Default,
            BadgeVariant::Destructive,
            BadgeVariant::Outline,
            BadgeVariant::Secondary,
            BadgeVariant::Ghost,
            BadgeVariant::Link,
        ] {
            for size in [BadgeSize::Default, BadgeSize::Sm] {
                assert_ne!(
                    default_theme.badge_style(variant, size),
                    swapped.badge_style(variant, size),
                    "badge_style({variant:?}, {size:?}) did not change on swap"
                );
            }
        }

        for variant in [
            DotVariant::Ok,
            DotVariant::Warn,
            DotVariant::Danger,
            DotVariant::Neutral,
        ] {
            for size in [DotSize::Sm, DotSize::Md] {
                assert_ne!(
                    default_theme.dot_style(variant, size),
                    swapped.dot_style(variant, size),
                    "dot_style({variant:?}, {size:?}) did not change on swap"
                );
            }
        }

        for variant in [
            TextVariant::Heading,
            TextVariant::Subheading,
            TextVariant::Body,
            TextVariant::Muted,
            TextVariant::Caption,
        ] {
            assert_ne!(
                default_theme.text_style(variant),
                swapped.text_style(variant),
                "text_style({variant:?}) did not change on swap"
            );
            assert_ne!(
                default_theme.text_style_active(variant),
                swapped.text_style_active(variant),
                "text_style_active({variant:?}) did not change on swap"
            );
        }
    }

    /// Regression (Codex round 1 on #205/PR #238): an earlier revision
    /// cached `panel` / `title_style` / `body_style` as plain `WgpuTheme`
    /// fields, computed once in `Default::default()`. Mutating only
    /// `theme.tokens` — the obvious way to "swap just the colors/spacing"
    /// — left those three stale, because nothing re-derived them. Unlike
    /// `swapping_token_set_changes_every_resolver_output` above (which
    /// swaps in a whole second `WgpuTheme` built the same way `Default`
    /// builds one), this test mutates `tokens` *in place* on a single
    /// theme, exactly as the review reproduced it, and must see every
    /// dependent style change.
    #[test]
    fn mutating_only_tokens_keeps_derived_styles_in_sync() {
        let mut theme = WgpuTheme::default();
        let before_body_color = theme.body_style().color;
        let before_title_color = theme.title_style().color;
        let before_panel_padding = theme.panel_compact().padding;

        theme.tokens.body_foreground = Color::rgba(0.0, 1.0, 0.0, 1.0);
        theme.tokens.surface_foreground = Color::rgba(1.0, 0.0, 1.0, 1.0);
        theme.tokens.spacing.padding_panel = 42.0;

        assert_ne!(
            theme.body_style().color,
            before_body_color,
            "body_style() must re-derive from the mutated body_foreground token"
        );
        assert_eq!(
            theme.body_style().color,
            theme.tokens.body_foreground.with_alpha(0.92)
        );

        assert_ne!(
            theme.title_style().color,
            before_title_color,
            "title_style() must re-derive from the mutated surface_foreground token"
        );
        assert_eq!(theme.title_style().color, theme.tokens.surface_foreground);

        assert_ne!(
            theme.panel_compact().padding,
            before_panel_padding,
            "panel_compact() must re-derive from the mutated spacing.padding_panel token"
        );
        assert_eq!(theme.panel_compact().padding, Insets::all(42.0));
    }
}
