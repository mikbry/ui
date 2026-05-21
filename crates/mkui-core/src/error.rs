/// Completely abstract error type for mkui operations
#[derive(Debug)]
pub struct MkuiError {
    /// Error message
    pub message: String,
    /// Optional error kind for categorization
    pub kind: MkuiErrorKind,
}

#[derive(Debug, Clone)]
pub enum MkuiErrorKind {
    /// Initialization error (e.g., failed to create UI context)
    Initialization,
    /// Rendering error (e.g., failed to render component)
    Rendering,
    /// Input/Output error (e.g., terminal, DOM operations)
    Io,
    /// Generic/Unknown error
    Generic,
}

impl MkuiError {
    pub fn new(kind: MkuiErrorKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind,
        }
    }

    pub fn initialization(message: impl Into<String>) -> Self {
        Self::new(MkuiErrorKind::Initialization, message)
    }

    pub fn rendering(message: impl Into<String>) -> Self {
        Self::new(MkuiErrorKind::Rendering, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(MkuiErrorKind::Io, message)
    }

    pub fn generic(message: impl Into<String>) -> Self {
        Self::new(MkuiErrorKind::Generic, message)
    }
}

impl std::fmt::Display for MkuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for MkuiError {}

impl From<String> for MkuiError {
    fn from(msg: String) -> Self {
        MkuiError::generic(msg)
    }
}

impl From<&str> for MkuiError {
    fn from(msg: &str) -> Self {
        MkuiError::generic(msg)
    }
}
