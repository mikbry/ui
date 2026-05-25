//! Variant enums shared by every binding.
//!
//! `ButtonVariant` and `TextVariant` belong to the runtime layer because every
//! binding needs to set them on a node and every renderer needs to read them.
//! `mkui-core::headless` re-exports these names so existing consumers keep
//! the `mkui_core::headless::ButtonVariant` path (no public-API churn).

use serde::{Deserialize, Serialize};

/// Visual flavour of a button node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Destructive,
    Outline,
    Ghost,
    Link,
}

impl ButtonVariant {
    /// Stable integer code used by C / Python FFI to identify a variant.
    /// Order is **load-bearing** — the C header exposes the same numbers as
    /// `MKUI_BUTTON_PRIMARY` … `MKUI_BUTTON_LINK`. Do not reorder.
    pub fn from_ffi(code: i32) -> Result<Self, ButtonVariantParseError> {
        match code {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Secondary),
            2 => Ok(Self::Destructive),
            3 => Ok(Self::Outline),
            4 => Ok(Self::Ghost),
            5 => Ok(Self::Link),
            other => Err(ButtonVariantParseError(other)),
        }
    }

    pub fn to_ffi(self) -> i32 {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
            Self::Destructive => 2,
            Self::Outline => 3,
            Self::Ghost => 4,
            Self::Link => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonVariantParseError(pub i32);

impl std::fmt::Display for ButtonVariantParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown button variant code: {}", self.0)
    }
}

impl std::error::Error for ButtonVariantParseError {}

/// Semantic role of a text node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum TextVariant {
    Body,
    Heading1,
    Heading2,
    Heading3,
    Caption,
    Label,
    Code,
}

impl TextVariant {
    /// Stable integer code used by C / Python FFI to identify a variant.
    pub fn from_ffi(code: i32) -> Result<Self, TextVariantParseError> {
        match code {
            0 => Ok(Self::Body),
            1 => Ok(Self::Heading1),
            2 => Ok(Self::Heading2),
            3 => Ok(Self::Heading3),
            4 => Ok(Self::Caption),
            5 => Ok(Self::Label),
            6 => Ok(Self::Code),
            other => Err(TextVariantParseError(other)),
        }
    }

    pub fn to_ffi(self) -> i32 {
        match self {
            Self::Body => 0,
            Self::Heading1 => 1,
            Self::Heading2 => 2,
            Self::Heading3 => 3,
            Self::Caption => 4,
            Self::Label => 5,
            Self::Code => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextVariantParseError(pub i32);

impl std::fmt::Display for TextVariantParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown text variant code: {}", self.0)
    }
}

impl std::error::Error for TextVariantParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_variant_ffi_roundtrip() {
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Secondary,
            ButtonVariant::Destructive,
            ButtonVariant::Outline,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
        ] {
            let code = variant.to_ffi();
            assert_eq!(ButtonVariant::from_ffi(code).unwrap(), variant);
        }
    }

    #[test]
    fn button_variant_ffi_rejects_unknown_code() {
        assert!(ButtonVariant::from_ffi(99).is_err());
        assert!(ButtonVariant::from_ffi(-1).is_err());
    }

    #[test]
    fn text_variant_ffi_roundtrip() {
        for variant in [
            TextVariant::Body,
            TextVariant::Heading1,
            TextVariant::Heading2,
            TextVariant::Heading3,
            TextVariant::Caption,
            TextVariant::Label,
            TextVariant::Code,
        ] {
            let code = variant.to_ffi();
            assert_eq!(TextVariant::from_ffi(code).unwrap(), variant);
        }
    }
}
