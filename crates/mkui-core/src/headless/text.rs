use super::traits::{Focusable, HeadlessComponent};
use crate::event::Event;
use crate::state::State;

/// State for a text component
#[derive(Debug, Clone, Default)]
pub struct TextState {
    pub focused: bool,
    pub selectable: bool,
    pub selected: bool,
}

impl State for TextState {}

/// Events for text component
#[derive(Debug, Clone)]
pub enum TextEvent {
    Focus,
    Blur,
    Select,
    Deselect,
}

impl Event for TextEvent {}

/// Text variants for styling
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TextVariant {
    Body,
    Heading1,
    Heading2,
    Heading3,
    Caption,
    Label,
    Code,
}

/// Text sizes
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TextSize {
    ExtraSmall,
    Small,
    Medium,
    Large,
    ExtraLarge,
}

/// Text weight
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TextWeight {
    Light,
    Normal,
    Medium,
    Semibold,
    Bold,
}

/// Text alignment
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

/// Headless text component with styling and selection
pub struct Text {
    state: TextState,
    content: String,
    variant: TextVariant,
    size: TextSize,
    weight: TextWeight,
    align: TextAlign,
    color: Option<String>,
}

impl Text {
    pub fn builder() -> TextBuilder {
        TextBuilder::new()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
    }

    pub fn variant(&self) -> &TextVariant {
        &self.variant
    }

    pub fn size(&self) -> &TextSize {
        &self.size
    }

    pub fn weight(&self) -> &TextWeight {
        &self.weight
    }

    pub fn align(&self) -> &TextAlign {
        &self.align
    }

    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }

    pub fn is_selectable(&self) -> bool {
        self.state.selectable
    }

    pub fn set_selectable(&mut self, selectable: bool) {
        self.state.selectable = selectable;
    }

    pub fn is_selected(&self) -> bool {
        self.state.selected
    }

    pub fn select(&mut self) {
        if self.state.selectable {
            self.state.selected = true;
        }
    }

    pub fn deselect(&mut self) {
        self.state.selected = false;
    }
}

impl HeadlessComponent for Text {
    type State = TextState;
    type Event = TextEvent;

    fn new() -> Self {
        Self {
            state: TextState::default(),
            content: String::new(),
            variant: TextVariant::Body,
            size: TextSize::Medium,
            weight: TextWeight::Normal,
            align: TextAlign::Left,
            color: None,
        }
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn handle_event(&mut self, event: Self::Event) {
        match event {
            TextEvent::Focus => self.focus(),
            TextEvent::Blur => self.blur(),
            TextEvent::Select => self.select(),
            TextEvent::Deselect => self.deselect(),
        }
    }
}

impl Focusable for Text {
    fn focus(&mut self) {
        if self.state.selectable {
            self.state.focused = true;
        }
    }

    fn blur(&mut self) {
        self.state.focused = false;
    }

    fn is_focused(&self) -> bool {
        self.state.focused
    }
}

/// Builder for Text component
pub struct TextBuilder {
    content: String,
    variant: TextVariant,
    size: TextSize,
    weight: TextWeight,
    align: TextAlign,
    color: Option<String>,
    selectable: bool,
}

impl TextBuilder {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            variant: TextVariant::Body,
            size: TextSize::Medium,
            weight: TextWeight::Normal,
            align: TextAlign::Left,
            color: None,
            selectable: false,
        }
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    pub fn weight(mut self, weight: TextWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn build(self) -> Text {
        Text {
            state: TextState {
                focused: false,
                selectable: self.selectable,
                selected: false,
            },
            content: self.content,
            variant: self.variant,
            size: self.size,
            weight: self.weight,
            align: self.align,
            color: self.color,
        }
    }
}

impl Default for TextBuilder {
    fn default() -> Self {
        Self::new()
    }
}
