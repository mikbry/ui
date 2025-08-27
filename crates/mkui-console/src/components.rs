use mkui_core::headless::{
    Toggle as HeadlessToggle, ToggleBuilder, 
    Button as HeadlessButton, ButtonBuilder, ButtonVariant, ButtonSize,
    Text as HeadlessText, TextBuilder, TextVariant, TextWeight, TextAlign,
    Focusable, KeyboardInteractable
};

/// Theme mode for console components
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
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
    Red,
    Yellow,
    Violet,
}

/// Theme configuration for console rendering
#[derive(Clone, Debug)]
pub struct ConsoleTheme {
    pub mode: ThemeMode,
    pub color: ColorTheme,
}

impl Default for ConsoleTheme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            color: ColorTheme::Default,
        }
    }
}

impl ConsoleTheme {
    pub fn new(mode: ThemeMode, color: ColorTheme) -> Self {
        Self { mode, color }
    }
    
    /// Get primary color for the theme
    pub fn primary_color(&self) -> Color {
        match (&self.mode, &self.color) {
            (ThemeMode::Light, ColorTheme::Default) => Color::Black,
            (ThemeMode::Dark, ColorTheme::Default) => Color::White,
            (_, ColorTheme::Blue) => Color::Blue,
            (_, ColorTheme::Green) => Color::Green,
            (_, ColorTheme::Amber) => Color::Yellow,
            (_, ColorTheme::Rose) => Color::Magenta,
            (_, ColorTheme::Purple) => Color::Magenta,
            (_, ColorTheme::Orange) => Color::Red,
            (_, ColorTheme::Teal) => Color::Cyan,
            (_, ColorTheme::Mono) => Color::Gray,
            (_, ColorTheme::Red) => Color::Red,
            (_, ColorTheme::Yellow) => Color::Yellow,
            (_, ColorTheme::Violet) => Color::Magenta,
        }
    }
    
    /// Get foreground color for the theme
    pub fn foreground_color(&self) -> Color {
        match self.mode {
            ThemeMode::Light => Color::Black,
            ThemeMode::Dark => Color::White,
        }
    }
    
    /// Get muted foreground color
    pub fn muted_foreground_color(&self) -> Color {
        match self.mode {
            ThemeMode::Light => Color::Gray,
            ThemeMode::Dark => Color::DarkGray,
        }
    }
    
    /// Get destructive color
    pub fn destructive_color(&self) -> Color {
        Color::Red
    }
    
    /// Get ring/focus color
    pub fn ring_color(&self) -> Color {
        self.primary_color()
    }
}

/// Console rendering wrapper for headless Toggle
pub struct ConsoleToggle {
    inner: HeadlessToggle,
    label: String,
    theme: ConsoleTheme,
}

impl ConsoleToggle {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            inner: ToggleBuilder::new().build(),
            label: label.into(),
            theme: ConsoleTheme::default(),
        }
    }
    
    pub fn with_theme(mut self, theme: ConsoleTheme) -> Self {
        self.theme = theme;
        self
    }
    
    pub fn class(self, _class: impl Into<String>) -> Self {
        // Console doesn't use CSS classes, but we keep the API compatible
        self
    }
    
    pub fn checked(mut self, checked: bool) -> Self {
        self.inner.set_checked(checked);
        self
    }
    
    pub fn toggle(&mut self) {
        self.inner.toggle();
    }
    
    pub fn is_checked(&self) -> bool {
        self.inner.is_checked()
    }
    
    pub fn render(&self) -> Paragraph {
        let symbol = if self.inner.is_checked() { 
            "☑" 
        } else { 
            "☐" 
        };
        
        let text = format!("{} {}", symbol, self.label);
        
        let style = if self.inner.is_focused() {
            Style::default()
                .fg(self.theme.ring_color())
                .add_modifier(Modifier::BOLD)
        } else if self.inner.is_disabled() {
            Style::default().fg(self.theme.muted_foreground_color())
        } else if self.inner.is_checked() {
            Style::default().fg(self.theme.primary_color())
        } else {
            Style::default().fg(self.theme.foreground_color())
        };
        
        Paragraph::new(text)
            .style(style)
            .alignment(Alignment::Left)
    }
    
    pub fn handle_key(&mut self, key: crossterm::event::KeyCode) {
        match key {
            crossterm::event::KeyCode::Char(' ') | 
            crossterm::event::KeyCode::Enter => {
                self.inner.handle_key_down(" ");
            }
            _ => {}
        }
    }
    
    pub fn focus(&mut self) {
        self.inner.focus();
    }
    
    pub fn blur(&mut self) {
        self.inner.blur();
    }
}

/// Console rendering wrapper for headless Button
pub struct ConsoleButton {
    inner: HeadlessButton,
    theme: ConsoleTheme,
}

impl ConsoleButton {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            inner: ButtonBuilder::new()
                .text(text)
                .build(),
            theme: ConsoleTheme::default(),
        }
    }
    
    pub fn with_theme(mut self, theme: ConsoleTheme) -> Self {
        self.theme = theme;
        self
    }
    
    pub fn class(self, _class: impl Into<String>) -> Self {
        // Console doesn't use CSS classes, but we keep the API compatible
        self
    }
    
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(variant)
            .size(self.inner.size().clone())
            .build();
        self
    }
    
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(self.inner.variant().clone())
            .size(size)
            .build();
        self
    }
    
    pub fn on_press<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(self.inner.variant().clone())
            .size(self.inner.size().clone())
            .on_click(f)
            .build();
        self
    }
    
    // Keep backwards compatibility
    pub fn on_click<F: Fn() + 'static>(self, f: F) -> Self {
        self.on_press(f)
    }
    
    pub fn click(&mut self) {
        self.inner.click();
    }
    
    pub fn render(&self) -> Paragraph {
        let text = if self.inner.is_loading() {
            format!("⏳ {}", self.inner.text())
        } else {
            format!("[ {} ]", self.inner.text())
        };
        
        let primary_color = self.theme.primary_color();
        let foreground_color = self.theme.foreground_color();
        let muted_color = self.theme.muted_foreground_color();
        let destructive_color = self.theme.destructive_color();
        
        let style = if self.inner.is_pressed() {
            Style::default()
                .fg(match self.theme.mode {
                    ThemeMode::Light => Color::White,
                    ThemeMode::Dark => Color::Black,
                })
                .bg(primary_color)
                .add_modifier(Modifier::BOLD)
        } else if self.inner.is_focused() {
            match self.inner.variant() {
                ButtonVariant::Primary => Style::default()
                    .fg(Color::White)
                    .bg(primary_color)
                    .add_modifier(Modifier::BOLD),
                ButtonVariant::Secondary => Style::default()
                    .fg(foreground_color)
                    .bg(muted_color)
                    .add_modifier(Modifier::BOLD),
                ButtonVariant::Destructive => Style::default()
                    .fg(Color::White)
                    .bg(destructive_color)
                    .add_modifier(Modifier::BOLD),
                ButtonVariant::Outline => Style::default()
                    .fg(primary_color)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
                ButtonVariant::Ghost => Style::default()
                    .fg(primary_color)
                    .add_modifier(Modifier::BOLD),
                ButtonVariant::Link => Style::default()
                    .fg(primary_color)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
            }
        } else if self.inner.is_disabled() {
            Style::default().fg(muted_color)
        } else {
            match self.inner.variant() {
                ButtonVariant::Primary => Style::default()
                    .fg(Color::White)  // Always use white text on colored background
                    .bg(primary_color),
                ButtonVariant::Secondary => Style::default()
                    .fg(match self.theme.mode {
                        ThemeMode::Light => Color::Black,
                        ThemeMode::Dark => Color::White,
                    })
                    .bg(muted_color),
                ButtonVariant::Destructive => Style::default()
                    .fg(Color::White)
                    .bg(destructive_color),
                ButtonVariant::Outline => Style::default()
                    .fg(foreground_color),
                ButtonVariant::Ghost => Style::default()
                    .fg(foreground_color),
                ButtonVariant::Link => Style::default()
                    .fg(primary_color),
            }
        };
        
        Paragraph::new(text)
            .style(style)
            .alignment(Alignment::Center)
    }
    
    pub fn handle_key(&mut self, key: crossterm::event::KeyCode) {
        match key {
            crossterm::event::KeyCode::Char(' ') | 
            crossterm::event::KeyCode::Enter => {
                self.inner.click();
            }
            _ => {}
        }
    }
    
    pub fn focus(&mut self) {
        self.inner.focus();
    }
    
    pub fn blur(&mut self) {
        self.inner.blur();
    }
}
/// Console rendering wrapper for headless Text
pub struct ConsoleText {
    inner: HeadlessText,
    theme: ConsoleTheme,
}

impl ConsoleText {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            inner: HeadlessText::new(content).build(),
            theme: ConsoleTheme::default(),
        }
    }
    
    pub fn with_theme(mut self, theme: ConsoleTheme) -> Self {
        self.theme = theme;
        self
    }
    
    pub fn class(self, _class: impl Into<String>) -> Self {
        // Console doesn't use CSS classes, but we keep the API compatible
        self
    }
    
    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.inner = TextBuilder::new()
            .content(self.inner.content())
            .variant(variant)
            .build();
        self
    }
    
    pub fn render(&self) -> Paragraph {
        let content = self.inner.content();
        
        let alignment = match self.inner.align() {
            TextAlign::Left => Alignment::Left,
            TextAlign::Center => Alignment::Center,
            TextAlign::Right => Alignment::Right,
            TextAlign::Justify => Alignment::Left,
        };
        
        let mut style = Style::default();
        
        let foreground_color = self.theme.foreground_color();
        let muted_color = self.theme.muted_foreground_color();
        
        style = match self.inner.variant() {
            TextVariant::Heading1 => style.fg(foreground_color).add_modifier(Modifier::BOLD),
            TextVariant::Heading2 => style.fg(foreground_color).add_modifier(Modifier::BOLD),
            TextVariant::Heading3 => style.fg(foreground_color).add_modifier(Modifier::BOLD),
            TextVariant::Caption => style.fg(muted_color),
            TextVariant::Label => style.fg(foreground_color),
            TextVariant::Code => style.fg(muted_color).bg(match self.theme.mode {
                ThemeMode::Light => Color::Gray,
                ThemeMode::Dark => Color::Black,
            }),
            TextVariant::Body => style.fg(foreground_color),
        };
        
        style = match self.inner.weight() {
            TextWeight::Light => style,
            TextWeight::Normal => style,
            TextWeight::Medium => style.add_modifier(Modifier::BOLD),
            TextWeight::Semibold => style.add_modifier(Modifier::BOLD),
            TextWeight::Bold => style.add_modifier(Modifier::BOLD),
        };
        
        if self.inner.is_selected() {
            style = style.bg(self.theme.primary_color()).fg(match self.theme.mode {
                ThemeMode::Light => Color::White,
                ThemeMode::Dark => Color::Black,
            });
        } else if self.inner.is_focused() {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        
        Paragraph::new(content)
            .style(style)
            .alignment(alignment)
    }
}
