pub mod component;
pub mod headless;
pub mod style;
pub mod event;
pub mod state;
pub mod components;
pub mod theme;
pub mod error;

pub mod prelude {
    // Components (high-level cross-platform components)
    pub use crate::components::{Component, Mkui, View, Text, Button};
    
    // Headless components (logic-only, no rendering)
    pub use crate::headless::{
        ButtonVariant, ButtonSize, TextVariant, TextWeight, TextAlign,
        Toggle, ToggleBuilder,
        Button as HeadlessButton, ButtonBuilder,
        Text as HeadlessText, TextBuilder,
        Focusable, KeyboardInteractable
    };
    
    // Error handling
    pub use crate::error::{MkuiError, MkuiErrorKind};
    
    // Other modules
    pub use crate::style::*;
    pub use crate::event::*;
    pub use crate::state::*;
    pub use crate::theme::*;
}