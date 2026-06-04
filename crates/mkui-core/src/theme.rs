//! Backend-agnostic theme contracts.
//!
//! `mkui-core` describes themes as abstract values: a [`ThemeMode`] (light /
//! dark / system) and a [`ColorTheme`] catalog entry. Backends are
//! responsible for translating these into platform-specific colors, CSS
//! classes, or terminal styles — those translations live in `mkui-web`,
//! `mkui-console`, `mkui-wgpu`, etc., never here.

use std::fmt;
use std::str::FromStr;

/// Theme mode for applications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Color themes (matches the shadcn UI catalog).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorTheme {
    Default,
    Blue,
    Green,
    Amber,
    Rose,
    Purple,
    Orange,
    Teal,
    Mono,
    Scaled,
    Red,
    Yellow,
    Violet,
}

impl ColorTheme {
    pub fn to_class(&self) -> &'static str {
        match self {
            ColorTheme::Default => "theme-default",
            ColorTheme::Blue => "theme-blue",
            ColorTheme::Green => "theme-green",
            ColorTheme::Amber => "theme-amber",
            ColorTheme::Rose => "theme-rose",
            ColorTheme::Purple => "theme-purple",
            ColorTheme::Orange => "theme-orange",
            ColorTheme::Teal => "theme-teal",
            ColorTheme::Mono => "theme-mono",
            ColorTheme::Scaled => "theme-scaled",
            ColorTheme::Red => "theme-red",
            ColorTheme::Yellow => "theme-yellow",
            ColorTheme::Violet => "theme-violet",
        }
    }

    pub fn all() -> &'static [ColorTheme] {
        const ALL: &[ColorTheme] = &[
            ColorTheme::Default,
            ColorTheme::Blue,
            ColorTheme::Green,
            ColorTheme::Amber,
            ColorTheme::Rose,
            ColorTheme::Purple,
            ColorTheme::Orange,
            ColorTheme::Teal,
            ColorTheme::Mono,
            ColorTheme::Scaled,
            ColorTheme::Red,
            ColorTheme::Yellow,
            ColorTheme::Violet,
        ];
        ALL
    }
}

/// Error returned by [`<ColorTheme as FromStr>::from_str`] when the input does
/// not match any known [`ColorTheme`] variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseColorThemeError {
    input: String,
}

impl ParseColorThemeError {
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl fmt::Display for ParseColorThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown color theme: {:?}", self.input)
    }
}

impl std::error::Error for ParseColorThemeError {}

impl FromStr for ColorTheme {
    type Err = ParseColorThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default" => Ok(ColorTheme::Default),
            "blue" => Ok(ColorTheme::Blue),
            "green" => Ok(ColorTheme::Green),
            "amber" => Ok(ColorTheme::Amber),
            "rose" => Ok(ColorTheme::Rose),
            "purple" => Ok(ColorTheme::Purple),
            "orange" => Ok(ColorTheme::Orange),
            "teal" => Ok(ColorTheme::Teal),
            "mono" => Ok(ColorTheme::Mono),
            "scaled" => Ok(ColorTheme::Scaled),
            "red" => Ok(ColorTheme::Red),
            "yellow" => Ok(ColorTheme::Yellow),
            "violet" => Ok(ColorTheme::Violet),
            other => Err(ParseColorThemeError {
                input: other.to_string(),
            }),
        }
    }
}

/// Bundled theme value passed across the contract.
///
/// Backends consume this and produce backend-specific styling (CSS classes
/// on web, `crossterm`/`ratatui` colors on console, sampled colors on WGPU).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub mode: ThemeMode,
    pub color: ColorTheme,
}

impl Theme {
    pub fn new(mode: ThemeMode, color: ColorTheme) -> Self {
        Self { mode, color }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            color: ColorTheme::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn color_theme_round_trips_through_string() {
        for theme in ColorTheme::all() {
            let s = theme.to_class().trim_start_matches("theme-");
            assert_eq!(ColorTheme::from_str(s).as_ref(), Ok(theme));
        }
    }

    #[test]
    fn color_theme_from_str_rejects_unknown_input() {
        let err = ColorTheme::from_str("not-a-real-theme").unwrap_err();
        assert_eq!(err.input(), "not-a-real-theme");
    }

    #[test]
    fn theme_default_is_system_default() {
        let theme = Theme::default();
        assert_eq!(theme.mode, ThemeMode::System);
        assert_eq!(theme.color, ColorTheme::Default);
    }
}
