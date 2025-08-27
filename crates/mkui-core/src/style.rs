/// Style classes and utilities (Tailwind-like)
#[derive(Debug, Clone, Default)]
pub struct StyleClass {
    classes: Vec<String>,
}

impl StyleClass {
    pub fn new() -> Self {
        Self { classes: Vec::new() }
    }
    
    pub fn add(mut self, class: &str) -> Self {
        self.classes.push(class.to_string());
        self
    }
    
    pub fn to_string(&self) -> String {
        self.classes.join(" ")
    }
}

/// Builder pattern for styles
pub struct Style;

impl Style {
    pub fn class(class: &str) -> StyleClass {
        StyleClass::new().add(class)
    }
}