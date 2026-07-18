//! Formatter: CST to doc IR to rendered text.

pub mod doc;

use std::fmt;

use parser::parser::Cst;

/// Errors produced while emitting formatted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// The emitter is not implemented yet.
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
