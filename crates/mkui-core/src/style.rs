use std::fmt;

/// Style classes and utilities (Tailwind-like)
#[derive(Debug, Clone, Default)]
pub struct StyleClass {
    classes: Vec<String>,
}

impl StyleClass {
    pub fn new() -> Self {
        Self { classes: Vec::new() }
    }

    pub fn push_class(mut self, class: &str) -> Self {
        self.classes.push(class.to_string());
        self
    }
}

impl fmt::Display for StyleClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.classes.join(" "))
    }
}

/// Builder pattern for styles
pub struct Style;

impl Style {
    pub fn class(class: &str) -> StyleClass {
        StyleClass::new().push_class(class)
    }
}
