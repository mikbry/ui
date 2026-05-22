//! Theme, variants, and cva-style style resolvers.
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

use crate::types::{Color, Insets, Stroke, TextAlign, TextStyle};
use crate::types::{FontFaceId, Size};

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

/// Root theme for the HUD. Holds panel / text baseline and a small palette
/// of semantic tokens (`primary`, `muted`, `destructive`) that variant
/// resolvers combine with size tokens into concrete widget styles.
///
/// The defaults target a warm editorial control-surface look; a future
/// light-mode or per-product palette swap would only need to replace this
/// struct.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudTheme {
    pub panel: PanelStyle,
    pub title_style: TextStyle,
    pub body_style: TextStyle,
    pub tokens: ThemeTokens,
}

/// Semantic color + geometry tokens that variant resolvers consume. Mirrors
/// the shadcn CSS variable palette (`--primary`, `--muted`, `--ring`, …) in
/// struct form — no utility-string parsing, no runtime theming layer yet.
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
    pub shadow: ShadowStyle,
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
/// typography. Resolved by [`HudTheme::text_style`] into a concrete
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
/// `(variant, size)` and resolve through [`HudTheme::button_style`].
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
/// shadcn "Input" analogue for immediate-mode HUD fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputStyle {
    pub fill: Color,
    pub stroke: Color,
    pub text_color: Color,
    pub text_style: TextStyle,
    pub corner_radius: f32,
    pub stroke_width: f32,
    pub padding: Insets,
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
/// `(variant, size)` and resolve through [`HudTheme::badge_style`]; this
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

impl Default for HudTheme {
    fn default() -> Self {
        let font = FontFaceId(0);
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
            shadow: ShadowStyle {
                color: Color::rgba(0.0, 0.0, 0.0, 0.38),
                blur_radius: 24.0,
                spread: 0.0,
            },
        };
        Self {
            panel: PanelStyle {
                fill: tokens.surface,
                stroke: Stroke {
                    color: tokens.border,
                    width: 1.0,
                },
                shadow: tokens.shadow,
                corner_radius: 14.0,
                padding: Insets::all(14.0),
            },
            title_style: TextStyle {
                font,
                font_size_px: 15.0,
                line_height_px: 20.0,
                color: tokens.surface_foreground,
                align: TextAlign::Start,
            },
            body_style: TextStyle {
                font,
                font_size_px: 13.0,
                line_height_px: 18.0,
                color: Color::rgba(0.92, 0.88, 0.82, 0.92),
                align: TextAlign::Start,
            },
            tokens,
        }
    }
}

impl HudTheme {
    /// Baseline panel style for HUD overlays (inspector, explorer, hint bar,
    /// toolbar). Callers override padding / corner_radius as needed.
    pub fn hud_panel(&self) -> PanelStyle {
        PanelStyle {
            fill: self.tokens.surface,
            stroke: Stroke {
                color: self.tokens.border,
                width: 1.0,
            },
            shadow: self.tokens.shadow,
            corner_radius: 16.0,
            padding: Insets::all(16.0),
        }
    }

    /// Rounded card chrome used for list rows and inline boxes. This is the
    /// shadcn "Card" primitive.
    pub fn card(&self) -> CardStyle {
        CardStyle {
            fill: self.tokens.card_fill,
            stroke: self.tokens.card_border,
            stroke_width: 1.0,
            corner_radius: 8.0,
        }
    }

    /// Selected variant of [`Self::card`] — brighter fill and border so the
    /// row reads as picked.
    pub fn card_selected(&self) -> CardStyle {
        CardStyle {
            fill: Color::rgba(0.22, 0.18, 0.13, 0.96),
            stroke: self.tokens.border.multiply_alpha(3.4),
            stroke_width: 1.0,
            corner_radius: 8.0,
        }
    }

    /// Default slider look. A "primary"-tinted fill bar on a dark track.
    pub fn slider(&self) -> SliderStyle {
        SliderStyle {
            track_fill: Color::rgba(0.08, 0.06, 0.05, 0.96),
            track_stroke: Color::rgba(0.96, 0.93, 0.88, 0.18),
            track_stroke_width: 1.0,
            track_corner_radius: 4.0,
            filled_fill: Color::rgba(0.94, 0.79, 0.48, 0.88),
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
                fill: Color::rgba(0.22, 0.18, 0.12, 0.98),
                stroke: self.tokens.primary,
                text_color: Color::rgba(0.99, 0.95, 0.88, 1.0),
                text_style: self.body_style,
                corner_radius: 6.0,
                stroke_width: 1.0,
                padding: Insets::symmetric(6.0, 1.0),
            }
        } else {
            InputStyle {
                fill: Color::rgba(0.08, 0.06, 0.05, 0.96),
                stroke: Color::rgba(0.96, 0.93, 0.88, 0.22),
                text_color: Color::rgba(0.99, 0.95, 0.88, 1.0),
                text_style: self.body_style,
                corner_radius: 6.0,
                stroke_width: 1.0,
                padding: Insets::symmetric(6.0, 1.0),
            }
        }
    }

    /// Default scrollbar look. Pill thumb on a faint channel.
    pub fn scrollbar(&self) -> ScrollbarStyle {
        ScrollbarStyle {
            track_fill: Color::rgba(0.04, 0.03, 0.02, 0.55),
            thumb_fill: Color::rgba(0.96, 0.93, 0.88, 0.55),
        }
    }

    /// Default swatch chrome.
    pub fn swatch(&self) -> SwatchStyle {
        SwatchStyle {
            stroke: Color::rgba(0.0, 0.0, 0.0, 0.35),
            stroke_width: 1.0,
            corner_radius: 3.0,
        }
    }

    /// Resolve `(variant, size)` to a concrete [`ButtonStyle`]. The two axes
    /// are combined cva-style: the variant picks the color palette, the size
    /// picks corner radius / stroke width / label text style.
    pub fn button_style(&self, variant: ButtonVariant, size: ButtonSize) -> ButtonStyle {
        let (corner_radius, stroke_width, label_size) = match size {
            ButtonSize::Default => (8.0, 1.0, self.body_style),
            ButtonSize::Sm => (6.0, 1.0, self.body_style),
            ButtonSize::Lg => (10.0, 1.0, self.body_style),
            ButtonSize::Icon => (4.0, 1.0, self.body_style),
        };
        let label_style = TextStyle {
            align: TextAlign::Center,
            ..label_size
        };

        match variant {
            ButtonVariant::Default => ButtonStyle {
                idle_fill: Color::rgba(0.14, 0.12, 0.11, 0.92),
                idle_stroke: Color::rgba(0.96, 0.93, 0.88, 0.14),
                idle_label: Color::rgba(0.98, 0.94, 0.88, 0.96),
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
                idle_stroke: Color::rgba(0.96, 0.93, 0.88, 0.22),
                idle_label: self.tokens.muted_foreground,
                active_fill: self.tokens.primary.multiply_alpha(0.96),
                active_stroke: self.tokens.surface,
                active_label: self.tokens.primary_foreground,
                corner_radius: corner_radius.max(10.0),
                stroke_width,
                label_style,
            },
            ButtonVariant::Secondary => ButtonStyle {
                idle_fill: Color::rgba(0.18, 0.15, 0.12, 0.95),
                idle_stroke: Color::rgba(0.96, 0.93, 0.88, 0.24),
                idle_label: self.tokens.surface_foreground,
                active_fill: self.tokens.primary,
                active_stroke: self.tokens.primary_ring,
                active_label: self.tokens.primary_foreground,
                corner_radius: corner_radius.max(6.0),
                stroke_width,
                label_style,
            },
            ButtonVariant::Ghost => ButtonStyle {
                idle_fill: Color::rgba(0.20, 0.16, 0.12, 0.96),
                idle_stroke: Color::rgba(0.96, 0.93, 0.88, 0.22),
                idle_label: Color::rgba(0.99, 0.95, 0.88, 1.0),
                active_fill: Color::rgba(0.28, 0.22, 0.16, 0.96),
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
                idle_fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                idle_stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
                idle_label: self.tokens.primary,
                active_fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                active_stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
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
                6.0,
                Insets::symmetric(8.0, 2.0),
                TextStyle {
                    align: TextAlign::Center,
                    ..self.body_style
                },
            ),
            BadgeSize::Sm => (
                4.0,
                Insets::symmetric(6.0, 1.0),
                TextStyle {
                    font_size_px: 10.0,
                    line_height_px: 12.0,
                    align: TextAlign::Center,
                    ..self.body_style
                },
            ),
        };

        match variant {
            BadgeVariant::Default => BadgeStyle {
                fill: self.tokens.primary,
                stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke_width: 0.0,
                label_color: self.tokens.primary_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Destructive => BadgeStyle {
                fill: self.tokens.destructive,
                stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke_width: 0.0,
                label_color: self.tokens.destructive_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Outline => BadgeStyle {
                fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke: self.tokens.border,
                stroke_width: 1.0,
                label_color: self.tokens.muted_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Secondary => BadgeStyle {
                fill: self.tokens.secondary,
                stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke_width: 0.0,
                label_color: self.tokens.secondary_foreground,
                corner_radius,
                padding,
                label_style: label_text,
            },
            BadgeVariant::Ghost => BadgeStyle {
                fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
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
                fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                stroke: Color::rgba(0.0, 0.0, 0.0, 0.0),
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
            TextVariant::Heading => self.title_style,
            TextVariant::Subheading => TextStyle {
                color: self.tokens.primary_ring,
                ..self.title_style
            },
            TextVariant::Body => self.body_style,
            TextVariant::Muted => TextStyle {
                color: Color::rgba(0.92, 0.88, 0.82, 0.72),
                ..self.body_style
            },
            TextVariant::Caption => TextStyle {
                color: Color::rgba(0.92, 0.88, 0.82, 0.65),
                font_size_px: 10.0,
                line_height_px: 12.0,
                align: TextAlign::End,
                ..self.body_style
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
                color: Color::rgba(0.16, 0.10, 0.04, 0.85),
                ..base
            },
            _ => TextStyle {
                color: self.tokens.primary_foreground,
                ..base
            },
        }
    }
}

/// Identity size used when a widget needs a placeholder constraint. Not
/// exposed publicly; keeps doc examples compiling without a Size constant.
#[allow(dead_code)]
pub(crate) const ZERO_SIZE: Size = Size::new(0.0, 0.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_style_flips_fill_on_variant() {
        let theme = HudTheme::default();
        let default = theme.button_style(ButtonVariant::Default, ButtonSize::Default);
        let outline = theme.button_style(ButtonVariant::Outline, ButtonSize::Default);
        assert_ne!(
            default.idle_fill, outline.idle_fill,
            "Default and Outline should resolve to different idle fills"
        );
    }

    #[test]
    fn button_style_respects_size() {
        let theme = HudTheme::default();
        let sm = theme.button_style(ButtonVariant::Default, ButtonSize::Sm);
        let lg = theme.button_style(ButtonVariant::Default, ButtonSize::Lg);
        assert!(sm.corner_radius < lg.corner_radius);
    }

    #[test]
    fn text_variant_muted_is_dimmer_than_body() {
        let theme = HudTheme::default();
        let body = theme.text_style(TextVariant::Body);
        let muted = theme.text_style(TextVariant::Muted);
        assert!(muted.color.a < body.color.a);
    }

    #[test]
    fn card_selected_differs_from_card() {
        let theme = HudTheme::default();
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
        let theme = HudTheme::default();
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
}
