//! Document IR the formatter lowers the CST into before rendering.

/// Placeholder doc algebra — the real IR lands with later issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    /// Verbatim text.
    Text(String),
}
