/// Theme mode for applications
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Color themes matching shadcn UI
#[derive(Clone, Debug, PartialEq)]
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

/// Console-specific theme configuration
#[cfg(feature = "console")]
#[derive(Clone, Debug)]
pub struct ConsoleTheme {
    pub mode: ThemeMode,
    pub color: ColorTheme,
}

#[cfg(feature = "console")]
impl Default for ConsoleTheme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            color: ColorTheme::Default,
        }
    }
}

#[cfg(feature = "console")]
impl ConsoleTheme {
    pub fn new(mode: ThemeMode, color: ColorTheme) -> Self {
        Self { mode, color }
    }
    
    /// Get primary color for the theme
    pub fn primary_color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match (&self.mode, &self.color) {
            (ThemeMode::Light, ColorTheme::Default) => Color::Black,
            (ThemeMode::Dark, ColorTheme::Default) => Color::White,
            (ThemeMode::System, ColorTheme::Default) => Color::White, // Default to dark mode
            (_, ColorTheme::Blue) => Color::Blue,
            (_, ColorTheme::Green) => Color::Green,
            (_, ColorTheme::Amber) => Color::Yellow,
            (_, ColorTheme::Rose) => Color::Magenta,
            (_, ColorTheme::Purple) => Color::Magenta,
            (_, ColorTheme::Orange) => Color::Red,
            (_, ColorTheme::Teal) => Color::Cyan,
            (_, ColorTheme::Mono) => Color::Gray,
            (_, ColorTheme::Scaled) => Color::Gray,
            (_, ColorTheme::Red) => Color::Red,
            (_, ColorTheme::Yellow) => Color::Yellow,
            (_, ColorTheme::Violet) => Color::Magenta,
        }
    }
    
    /// Get foreground color for the theme
    pub fn foreground_color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self.mode {
            ThemeMode::Light => Color::Black,
            ThemeMode::Dark => Color::White,
            ThemeMode::System => Color::White, // Default to dark
        }
    }
    
    /// Get muted foreground color
    pub fn muted_foreground_color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self.mode {
            ThemeMode::Light => Color::Gray,
            ThemeMode::Dark => Color::DarkGray,
            ThemeMode::System => Color::DarkGray, // Default to dark
        }
    }
    
    /// Get destructive color
    pub fn destructive_color(&self) -> ratatui::style::Color {
        ratatui::style::Color::Red
    }
    
    /// Get ring/focus color
    pub fn ring_color(&self) -> ratatui::style::Color {
        self.primary_color()
    }
}