//! Formatter: doc IR and renderer (TREE-96). CST-to-doc lowering for
//! real SQL lands with TREE-97; `emit` is still a stub until then.

pub mod doc;
pub mod keywords;
mod printer;
mod quoting;

use std::fmt;

use parser::Dialect;
use parser::parser::Cst;

pub use doc::{Doc, IdentPos};

/// Maximum line width. Fixed; not part of the config surface.
pub const MAX_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Tab,
    Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
    #[default]
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdentQuoting {
    #[default]
    UnquotedWhenSafe,
    AlwaysQuoted,
}

/// The complete configuration surface of the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Tabs (default) or spaces.
    pub indent_style: IndentStyle,
    /// Width of one indent level: the space count in spaces mode, and the
    /// measured width of a tab in tab mode. Default 2.
    pub indent_width: u8,
    pub keyword_case: KeywordCase,
    pub quoting: IdentQuoting,
    /// Governs the identifier-quoting safety rules.
    pub dialect: Dialect,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            indent_style: IndentStyle::default(),
            indent_width: 2,
            keyword_case: KeywordCase::default(),
            quoting: IdentQuoting::default(),
            dialect: Dialect::default(),
        }
    }
}

/// Render a document to text.
pub fn render(doc: &Doc, options: &Options) -> String {
    printer::render(doc, options)
}

/// Errors produced while emitting formatted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// The CST-to-doc lowering is not implemented yet.
    Unimplemented,
}

impl fmt::Display for EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::Unimplemented => f.write_str("emitter not implemented"),
        }
    }
}

impl std::error::Error for EmitError {}

/// Render a CST as formatted SQL text.
pub fn emit(_cst: &Cst) -> Result<String, EmitError> {
    Err(EmitError::Unimplemented)
}
