//! Backend-agnostic theme contracts.
//!
//! `mkui-core` describes themes as abstract values: a [`ThemeMode`] (light /
//! dark / system) and a [`ColorTheme`] catalog entry. Backends are
//! responsible for translating these into platform-specific colors, CSS
//! classes, or terminal styles — those translations live in `mkui-web`,
//! `mkui-console`, `mkui-native`, etc., never here.

/// Theme mode for applications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Color themes (matches the shadcn UI catalog).
#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "default" => Some(ColorTheme::Default),
            "blue" => Some(ColorTheme::Blue),
            "green" => Some(ColorTheme::Green),
            "amber" => Some(ColorTheme::Amber),
            "rose" => Some(ColorTheme::Rose),
            "purple" => Some(ColorTheme::Purple),
            "orange" => Some(ColorTheme::Orange),
            "teal" => Some(ColorTheme::Teal),
            "mono" => Some(ColorTheme::Mono),
            "scaled" => Some(ColorTheme::Scaled),
            "red" => Some(ColorTheme::Red),
            "yellow" => Some(ColorTheme::Yellow),
            "violet" => Some(ColorTheme::Violet),
            _ => None,
        }
    }

    pub fn all() -> Vec<ColorTheme> {
        vec![
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
        ]
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

    #[test]
    fn color_theme_round_trips_through_string() {
        for theme in ColorTheme::all() {
            let s = theme.to_class().trim_start_matches("theme-");
            assert_eq!(ColorTheme::from_str(s), Some(theme));
        }
    }

    #[test]
    fn theme_default_is_system_default() {
        let theme = Theme::default();
        assert_eq!(theme.mode, ThemeMode::System);
        assert_eq!(theme.color, ColorTheme::Default);
    }
}
